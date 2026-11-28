//! Kotlin classes are final and have no no-arg constructor unless a compiler
//! plugin changes that. Spring proxies and JPA need both, so a Gradle module
//! with Spring or JPA Kotlin sources needs the matching plugin.

use std::collections::BTreeSet;
use std::path::PathBuf;

use dornguard_config::Rule;
use dornguard_core::{Diagnostic, EngineName, Module, SourceLanguage, fingerprint};
use miette::Result;

use super::Plan;
use super::sources::{collect_source_dirs, source_dirs};
use crate::SourceFiles;

pub(super) const SPRING_PLUGIN_RULE: &str = "kotlin.missing-spring-plugin";
pub(super) const JPA_PLUGIN_RULE: &str = "kotlin.missing-jpa-plugin";

/// Annotations whose classes Spring subclasses with a CGLIB proxy.
const PROXIED: [&str; 10] = [
    "Configuration",
    "SpringBootApplication",
    "Transactional",
    "Async",
    "Cacheable",
    "CacheEvict",
    "CachePut",
    "Caching",
    "Retryable",
    "Validated",
];

struct Check {
    rule_id: &'static str,
    plugins: [&'static str; 2],
    package: &'static str,
    annotations: &'static [&'static str],
    /// Whether declaring the class `open` makes the plugin unnecessary.
    open_suffices: bool,
}

const CHECKS: [Check; 2] = [
    Check {
        rule_id: SPRING_PLUGIN_RULE,
        plugins: [
            "org.jetbrains.kotlin.plugin.spring",
            "org.jetbrains.kotlin.plugin.allopen",
        ],
        package: "org.springframework",
        annotations: &PROXIED,
        open_suffices: true,
    },
    Check {
        rule_id: JPA_PLUGIN_RULE,
        plugins: [
            "org.jetbrains.kotlin.plugin.jpa",
            "org.jetbrains.kotlin.plugin.noarg",
        ],
        package: "persistence",
        annotations: &["Entity", "Embeddable", "MappedSuperclass"],
        // JPA needs a no-arg constructor, which only the plugin adds.
        open_suffices: false,
    },
];

pub(super) fn diagnostics(
    plan: &Plan<'_>,
    sources: &SourceFiles<'_>,
    modules: &[&Module],
) -> Result<Vec<Diagnostic>> {
    let mut found = Vec::new();
    for module in modules {
        let rules = plan.rules_for(&module.path);
        let active = CHECKS
            .iter()
            .filter_map(|check| {
                let rule = rules.iter().find(|rule| rule.id == check.rule_id)?;
                let applied = check
                    .plugins
                    .iter()
                    .any(|plugin| module.kotlin_compiler_plugins.iter().any(|p| p == plugin));
                (!applied).then_some((check, *rule))
            })
            .collect::<Vec<_>>();
        if active.is_empty() || module.kotlin_source_dirs.is_empty() {
            continue;
        }
        let mut files = BTreeSet::new();
        collect_source_dirs(
            "Gradle",
            sources,
            sources.root,
            SourceLanguage::Kotlin,
            &source_dirs(module, SourceLanguage::Kotlin, false),
            &mut files,
        )?;
        for (check, rule) in active {
            if let Some((path, line, annotation)) = first_use(sources, &files, check) {
                found.push(diagnostic(module, rule, path, line, annotation));
            }
        }
    }
    Ok(found)
}

/// The first annotation of `check` on a class that is not declared `open`.
fn first_use(
    sources: &SourceFiles<'_>,
    files: &BTreeSet<PathBuf>,
    check: &Check,
) -> Option<(PathBuf, u32, &'static str)> {
    for file in files {
        let Ok(text) = std::fs::read_to_string(sources.root.join(file)) else {
            continue;
        };
        if !text.contains(check.package) {
            continue;
        }
        let lines = text.lines().collect::<Vec<_>>();
        for (index, line) in lines.iter().enumerate() {
            let trimmed = line.trim_start();
            let Some(annotation) = check.annotations.iter().find(|name| {
                trimmed.strip_prefix('@').is_some_and(|rest| {
                    rest.strip_prefix(**name).is_some_and(|after| {
                        !after.starts_with(|c: char| c.is_alphanumeric() || c == '_')
                    })
                })
            }) else {
                continue;
            };
            // An explicitly open class or function needs no Spring plugin.
            let declaration = lines[index..]
                .iter()
                .find(|line| line.contains("class ") || line.contains("fun "));
            let needs_plugin = declaration.is_some_and(|line| {
                if line.contains("interface ") {
                    false
                } else if !check.open_suffices {
                    line.contains("class ")
                } else if line.contains("class ") {
                    !is_open(line)
                } else {
                    // A proxy can override the function only if its class is open too.
                    !is_open(line)
                        || enclosing_class(&lines[..index], line)
                            .is_some_and(|class| !is_open(class))
                }
            });
            if needs_plugin {
                return Some((file.clone(), index as u32 + 1, annotation));
            }
        }
    }
    None
}

/// Whether a class or function declaration line is open to subclasses. An `override`
/// member stays open unless it is `final override`.
fn is_open(line: &str) -> bool {
    line.contains("open ")
        || line.contains("abstract ")
        || (line.contains("override ") && !line.contains("final "))
}

/// The nearest class declaration above `member` that is indented less than it.
fn enclosing_class<'l>(above: &[&'l str], member: &str) -> Option<&'l str> {
    let indent = |line: &str| line.len() - line.trim_start().len();
    above
        .iter()
        .rev()
        .find(|line| line.contains("class ") && indent(line) < indent(member))
        .copied()
}

fn diagnostic(
    module: &Module,
    rule: &Rule,
    path: PathBuf,
    line: u32,
    annotation: &str,
) -> Diagnostic {
    let path = path
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/");
    Diagnostic {
        path: Some(path),
        module: Some(module.path.clone()),
        class_name: None,
        line: Some(line),
        column: None,
        end_line: None,
        end_column: None,
        subject: Some(format!("@{annotation}")),
        rule_id: rule.id.clone(),
        engine: EngineName::Gradle,
        severity: rule.severity,
        confidence: rule.confidence,
        message: format!(
            "{} Module {} uses @{annotation}.",
            rule.description, module.path
        ),
        fix: rule.fix.clone(),
        edit: None,
        legacy_fingerprints: Vec::new(),
        group: None,
        fingerprint: Some(fingerprint(&format!("{}:{}", rule.id, module.path))),
    }
}
