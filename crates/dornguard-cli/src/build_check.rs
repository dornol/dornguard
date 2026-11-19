use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use dornguard_config::{ProjectConfig, Registry, Rule, UNNECESSARY_IMPORT_RULE};
use dornguard_core::{
    Diagnostic, EngineName, FailureKind, Module, Outcome, ProjectModel, SourceLanguage,
    TEMPLATE_DIR, fingerprint,
};
use dornguard_engine::{ArchModule, Engine, Pmd, Request, check_architecture};
use dornguard_gradle::CompileModules;
use miette::{Result, bail, miette};

use crate::{changed::Changes, failure, run_source_engines, source_files};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BuildSystem {
    Gradle,
    Maven,
}

impl BuildSystem {
    pub fn detect(root: &Path) -> Self {
        let gradle_files = [
            "gradlew",
            "gradlew.bat",
            "settings.gradle",
            "settings.gradle.kts",
            "build.gradle",
            "build.gradle.kts",
        ];
        if root.join("pom.xml").is_file()
            && !gradle_files.iter().any(|file| root.join(file).is_file())
        {
            Self::Maven
        } else {
            Self::Gradle
        }
    }

    async fn collect(self, root: &Path, compile: CompileModules<'_>) -> Result<ProjectModel> {
        match self {
            Self::Gradle => dornguard_gradle::collect(root, compile).await,
            Self::Maven => {
                let compile = !matches!(compile, CompileModules::Selected([]));
                crate::maven::collect(root, compile).await
            }
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Gradle => "Gradle",
            Self::Maven => "Maven",
        }
    }
}

pub struct Check<'a> {
    pub build: BuildSystem,
    pub root: &'a Path,
    pub target: &'a Path,
    pub filter: Option<&'a str>,
    pub registry: &'a Registry,
    pub config: &'a ProjectConfig,
    pub profiles: &'a [String],
    pub global: &'a [&'a Rule],
    pub global_kotlin: bool,
    pub resolve_imports: bool,
    pub include_other_sources: bool,
    pub changes: Option<&'a Changes>,
    pub required: bool,
}

pub async fn check(check: Check<'_>) -> Outcome {
    let Check {
        build,
        root,
        target,
        filter,
        registry,
        config,
        profiles,
        global,
        global_kotlin,
        resolve_imports,
        include_other_sources,
        mut changes,
        required,
    } = check;
    let config_error = |error| failure(FailureKind::ConfigurationError, error);
    let preparation_error = |error| failure(FailureKind::PreparationError, error);
    if !target.starts_with(root) {
        return preparation_error(miette!(
            "The check target must be inside the configuration directory"
        ));
    }
    let mut overrides = BTreeMap::new();
    let mut kotlin_modules = BTreeSet::new();
    for name in config.modules.keys() {
        match registry.resolve(config, Some(name), profiles) {
            Ok(rules) => {
                match registry.kotlin_profile_enabled(config, Some(name), profiles) {
                    Ok(enabled) => {
                        if enabled || rules.iter().any(Rule::enables_kotlin) {
                            kotlin_modules.insert(name.as_str());
                        }
                    }
                    Err(error) => return config_error(error),
                }
                overrides.insert(name.as_str(), rules);
            }
            Err(error) => return config_error(error),
        }
    }

    if build == BuildSystem::Maven {
        if global_kotlin || !kotlin_modules.is_empty() {
            return config_error(miette!("Kotlin build integration requires Gradle"));
        }
        if !config.module_dependencies.forbid.is_empty() {
            return config_error(miette!("module_dependencies.forbid requires Gradle"));
        }
    }
    let overrides = overrides
        .iter()
        .map(|(&name, rules)| (name, rules.iter().collect::<Vec<_>>()))
        .collect::<BTreeMap<_, _>>();
    let rules_for = |name: &str| overrides.get(name).map_or(global, Vec::as_slice);
    let kotlin_for = |name: &str| global_kotlin || kotlin_modules.contains(name);
    let format_kotlin =
        |name: &str| !kotlin_for(name) && rules_for(name).iter().any(|rule| rule.is_format_rule());
    let kotlin_sources = |name: &str| kotlin_for(name) || format_kotlin(name);
    let web_for = |name: &str, language: SourceLanguage| {
        rules_for(name).iter().any(|rule| rule.applies_to(language))
    };
    let needs_compile = |rules: &[&Rule]| {
        rules.iter().any(|rule| {
            rule.engine == EngineName::Archunit
                || resolve_imports && rule.id == UNNECESSARY_IMPORT_RULE
        })
    };
    let (discovery, affected, fallback_reason) = if let Some(changes_found) = changes {
        let model = match build.collect(root, CompileModules::Selected(&[])).await {
            Ok(model) => model,
            Err(error) => return preparation_error(error),
        };
        match affected_modules(
            root,
            &model.modules,
            changes_found,
            kotlin_sources,
            model
                .modules
                .iter()
                .any(|module| kotlin_sources(&module.path)),
            format_kotlin,
            web_for,
        ) {
            Ok(affected) => (Some(model), Some(affected), None),
            Err(error) => {
                changes = None;
                (Some(model), None, Some(error.to_string()))
            }
        }
    } else {
        (None, None, None)
    };
    let compile_names = if let Some(affected) = &affected {
        affected
            .iter()
            .filter(|name| needs_compile(rules_for(name)))
            .cloned()
            .collect()
    } else {
        match filter {
            Some(name) if needs_compile(rules_for(name)) => vec![name.to_owned()],
            Some(_) => Vec::new(),
            None => overrides
                .iter()
                .filter(|(_, rules)| needs_compile(rules))
                .map(|(&name, _)| name.to_owned())
                .collect(),
        }
    };
    let compile_all = affected.is_none() && filter.is_none() && needs_compile(global);
    let maven_compiled = build == BuildSystem::Maven && (compile_all || !compile_names.is_empty());
    let model = match discovery {
        Some(model) if compile_names.is_empty() && !compile_all => model,
        _ => {
            let compile = if compile_all {
                CompileModules::AllJava
            } else {
                CompileModules::Selected(&compile_names)
            };
            match build.collect(root, compile).await {
                Ok(model) => model,
                Err(error) => {
                    let mut outcome = preparation_error(error);
                    outcome.fallback_reason = fallback_reason;
                    return outcome;
                }
            }
        }
    };

    let build_name = build.name();
    let known = model
        .modules
        .iter()
        .map(|module| module.path.as_str())
        .collect::<BTreeSet<_>>();
    let dependency_paths = config
        .module_dependencies
        .forbid
        .iter()
        .flat_map(|dependency| [dependency.from.as_str(), dependency.to.as_str()]);
    for name in overrides
        .keys()
        .copied()
        .chain(filter)
        .chain(dependency_paths)
    {
        if !known.contains(name) {
            return config_error(miette!("Unknown {build_name} module: {name}"));
        }
    }

    let selected = model
        .modules
        .iter()
        .filter(|module| {
            filter.is_none_or(|filter| filter == module.path)
                && affected
                    .as_ref()
                    .is_none_or(|affected| affected.contains(&module.path))
                && (target == root
                    || target.starts_with(&module.project_dir)
                    || module.project_dir.starts_with(target)
                    || module
                        .source_dirs
                        .iter()
                        .any(|dir| target.starts_with(dir) || dir.starts_with(target))
                    || kotlin_sources(&module.path)
                        && source_dirs(module, SourceLanguage::Kotlin, format_kotlin(&module.path))
                            .iter()
                            .any(|dir| target.starts_with(dir) || dir.starts_with(target)))
        })
        .collect::<Vec<_>>();
    let selected_names = selected
        .iter()
        .map(|module| module.path.as_str())
        .collect::<BTreeSet<_>>();
    let mut source_groups = BTreeMap::new();
    let mut pmd_modules = Vec::new();
    let mut mapped_files = BTreeSet::new();
    let mut architecture = Vec::new();
    for module in selected {
        let rules = rules_for(&module.path);
        let kotlin = kotlin_for(&module.path);
        let jvm = |language| {
            language == SourceLanguage::Java || (language == SourceLanguage::Kotlin && kotlin)
        };
        let format_kotlin = format_kotlin(&module.path);
        let source_jvm =
            |language| jvm(language) || (language == SourceLanguage::Kotlin && format_kotlin);
        let mut files = match module_files(
            build_name,
            root,
            target,
            module,
            required,
            format_kotlin,
            source_jvm,
        ) {
            Ok(files) => files,
            Err(error) => return preparation_error(error),
        };
        let arch_rules = rules
            .iter()
            .copied()
            .filter(|rule| rule.engine == EngineName::Archunit)
            .collect::<Vec<_>>();
        if !arch_rules.is_empty()
            && (affected.is_some()
                || files
                    .iter()
                    .any(|path| SourceLanguage::of(path).is_some_and(jvm)))
        {
            let all_files = match module_files(build_name, root, root, module, required, false, jvm)
            {
                Ok(files) => files,
                Err(error) => return preparation_error(error),
            };
            if !all_files.is_empty() {
                architecture.push(ArchModule {
                    module,
                    rules: arch_rules,
                    files: all_files,
                    kotlin,
                });
            }
        }
        match module_files(
            build_name,
            root,
            target,
            module,
            required,
            false,
            |language| {
                !matches!(language, SourceLanguage::Java | SourceLanguage::Kotlin)
                    && web_for(&module.path, language)
            },
        ) {
            Ok(web_files) => files.extend(web_files),
            Err(error) => return preparation_error(error),
        }
        if let Some(changes) = changes {
            files.retain(|file| changes.matches(file));
        }
        let mut source_rules = rules
            .iter()
            .copied()
            .filter(|rule| {
                matches!(
                    rule.engine,
                    EngineName::Semgrep
                        | EngineName::Pmd
                        | EngineName::Eslint
                        | EngineName::Builtin
                )
            })
            .collect::<Vec<_>>();
        files.retain(|file| {
            SourceLanguage::of(file)
                .is_some_and(|language| source_rules.iter().any(|rule| rule.applies_to(language)))
        });
        if include_other_sources {
            mapped_files.extend(files.iter().cloned());
        }
        if (maven_compiled || compile_all || compile_names.contains(&module.path))
            && source_rules
                .iter()
                .any(|rule| rule.id == UNNECESSARY_IMPORT_RULE)
            && let Some(classpath) = compiled_classpath(module)
        {
            let java_files = files
                .iter()
                .filter(|path| SourceLanguage::of(path) == Some(SourceLanguage::Java))
                .cloned()
                .collect::<Vec<_>>();
            if !java_files.is_empty() {
                let pmd_rules = source_rules
                    .extract_if(.., |rule| rule.engine == EngineName::Pmd)
                    .collect::<Vec<_>>();
                pmd_modules.push((java_files, pmd_rules, classpath));
            }
        }
        add_source_group(&mut source_groups, source_rules, files);
    }
    if include_other_sources {
        let files = match source_files(root, target, required, |language| {
            global.iter().any(|rule| rule.applies_to(language))
        }) {
            Ok(files) => files,
            Err(error) => return preparation_error(error),
        };
        let files = files.into_iter().filter(|path| {
            !mapped_files.contains(path) && changes.is_none_or(|changes| changes.matches(path))
        });
        add_source_group(&mut source_groups, global.to_vec(), files);
    }

    let mut outcome = Outcome {
        fallback_reason,
        ..Outcome::default()
    };
    let dependency_checks = config
        .module_dependencies
        .forbid
        .iter()
        .filter(|dependency| selected_names.contains(dependency.from.as_str()))
        .collect::<Vec<_>>();
    if source_groups.is_empty()
        && pmd_modules.is_empty()
        && architecture.is_empty()
        && dependency_checks.is_empty()
    {
        if changes.is_some() {
            outcome.no_changes = true;
            return outcome;
        }
        outcome.merge(preparation_error(miette!(
            "No {build_name} module targets apply to this check"
        )));
        return outcome;
    }
    if !dependency_checks.is_empty() {
        let rule = match registry.rule("architecture.module-dependency") {
            Ok(rule) => rule,
            Err(error) => return config_error(error),
        };
        for dependency in dependency_checks {
            if model.modules.iter().any(|module| {
                module.path == dependency.from && module.dependencies.contains(&dependency.to)
            }) {
                let build_file = if root.join("build.gradle.kts").is_file() {
                    "build.gradle.kts"
                } else {
                    "build.gradle"
                };
                outcome.diagnostics.push(Diagnostic {
                    path: Some(build_file.into()),
                    module: None,
                    class_name: None,
                    line: None,
                    column: None,
                    rule_id: rule.id.clone(),
                    engine: EngineName::Gradle,
                    severity: rule.severity,
                    message: format!(
                        "{} {} → {}.",
                        rule.description, dependency.from, dependency.to
                    ),
                    fix: rule.fix.clone(),
                    fingerprint: Some(fingerprint(&format!(
                        "{}:{}",
                        dependency.from, dependency.to
                    ))),
                });
            }
        }
    }

    let source_checks = async {
        let mut outcome = Outcome::default();
        for (rules, files) in source_groups.into_values() {
            outcome.merge(
                run_source_engines(
                    root,
                    &files.into_iter().collect::<Vec<_>>(),
                    registry,
                    &rules,
                    &config.format,
                )
                .await,
            );
        }
        outcome
    };
    let pmd_checks = async {
        let mut outcome = Outcome::default();
        for (files, rules, classpath) in pmd_modules {
            outcome.merge(
                Pmd {
                    classpath: Some(&classpath),
                }
                .check(Request {
                    root,
                    files: &files,
                    rules: &rules,
                    registry,
                })
                .await,
            );
        }
        outcome
    };
    let (source_result, architecture_result, pmd_result) = tokio::join!(
        source_checks,
        check_architecture(root, &architecture),
        pmd_checks
    );
    outcome.merge(source_result);
    outcome.merge(architecture_result);
    outcome.merge(pmd_result);
    outcome
}

type SourceGroups<'a> =
    BTreeMap<Vec<(&'a str, dornguard_core::Severity)>, (Vec<&'a Rule>, BTreeSet<PathBuf>)>;

fn add_source_group<'a>(
    groups: &mut SourceGroups<'a>,
    rules: Vec<&'a Rule>,
    files: impl IntoIterator<Item = PathBuf>,
) {
    let mut files = files.into_iter().peekable();
    if rules.is_empty() || files.peek().is_none() {
        return;
    }
    let ids = rules
        .iter()
        .map(|rule| (rule.id.as_str(), rule.severity))
        .collect();
    groups
        .entry(ids)
        .or_insert_with(|| (rules, BTreeSet::new()))
        .1
        .extend(files);
}

fn compiled_classpath(module: &Module) -> Option<Vec<PathBuf>> {
    let output = module.class_dirs.iter().filter(|path| path.is_dir());
    let mut classpath = output.cloned().collect::<Vec<_>>();
    if classpath.is_empty()
        || module
            .classpath
            .iter()
            .any(|entry| !(entry.is_file() || entry.is_dir()))
    {
        return None;
    }
    let mut seen = classpath.iter().cloned().collect::<BTreeSet<_>>();
    classpath.extend(
        module
            .classpath
            .iter()
            .filter(|path| seen.insert((*path).clone()))
            .cloned(),
    );
    Some(classpath)
}

fn affected_modules(
    root: &Path,
    modules: &[Module],
    changes: &Changes,
    kotlin_for: impl Fn(&str) -> bool,
    any_kotlin: bool,
    format_kotlin: impl Fn(&str) -> bool,
    web_for: impl Fn(&str, SourceLanguage) -> bool,
) -> Result<BTreeSet<String>> {
    let mut affected = BTreeSet::new();
    for path in changes.paths() {
        let Some(language) = SourceLanguage::of(path) else {
            continue;
        };
        if language == SourceLanguage::Kotlin && !any_kotlin {
            continue;
        }
        let full = root.join(path);
        let mut matched = false;
        for module in modules.iter().filter(|module| {
            source_dirs(module, language, format_kotlin(&module.path))
                .iter()
                .any(|dir| full.starts_with(dir))
        }) {
            matched = true;
            let enabled = match language {
                SourceLanguage::Java => true,
                SourceLanguage::Kotlin => kotlin_for(&module.path),
                other => web_for(&module.path, other),
            };
            if enabled {
                affected.insert(module.path.clone());
            }
        }
        if !matched && matches!(language, SourceLanguage::Java | SourceLanguage::Kotlin) {
            bail!(
                "Changed source path does not map to a Gradle module: {}",
                path.display()
            );
        }
    }
    let mut pending = affected.iter().cloned().collect::<Vec<_>>();
    while let Some(changed) = pending.pop() {
        for module in modules
            .iter()
            .filter(|module| module.dependencies.contains(&changed))
        {
            if affected.insert(module.path.clone()) {
                pending.push(module.path.clone());
            }
        }
    }
    Ok(affected)
}

fn module_files(
    build_name: &str,
    root: &Path,
    target: &Path,
    module: &Module,
    required: bool,
    format_kotlin: bool,
    accepts: impl Fn(SourceLanguage) -> bool,
) -> Result<Vec<PathBuf>> {
    let mut files = BTreeSet::new();
    for language in [
        SourceLanguage::Java,
        SourceLanguage::Kotlin,
        SourceLanguage::Html,
        SourceLanguage::Template,
        SourceLanguage::Javascript,
        SourceLanguage::Jsp,
    ]
    .into_iter()
    .filter(|&language| accepts(language))
    {
        let dirs = source_dirs(module, language, format_kotlin);
        for dir in dirs.iter() {
            if !dir.starts_with(root) {
                bail!(
                    "{build_name} source directory is outside the project: {}",
                    dir.display()
                );
            }
            if !dir.is_dir() {
                continue;
            }
            if target.starts_with(dir) {
                files.extend(source_files(root, target, required, |file| {
                    file == language
                })?);
            } else if dir.starts_with(target) {
                files.extend(source_files(root, dir, required, |file| file == language)?);
            }
        }
    }
    Ok(files.into_iter().collect())
}

fn source_dirs(
    module: &Module,
    language: SourceLanguage,
    format_kotlin: bool,
) -> Cow<'_, [PathBuf]> {
    match language {
        SourceLanguage::Java => Cow::Borrowed(&module.source_dirs),
        SourceLanguage::Kotlin if format_kotlin => Cow::Owned(
            module
                .kotlin_source_dirs
                .iter()
                .chain(&module.source_dirs)
                .cloned()
                .chain([module.project_dir.join("src/main/kotlin")])
                .collect(),
        ),
        SourceLanguage::Kotlin => Cow::Borrowed(&module.kotlin_source_dirs),
        SourceLanguage::Template => Cow::Owned(vec![module.project_dir.join(TEMPLATE_DIR)]),
        SourceLanguage::Jsp => Cow::Owned(vec![module.project_dir.join("src/main/webapp")]),
        SourceLanguage::Html | SourceLanguage::Javascript => {
            let mut dirs = vec![
                "src/main/resources/static",
                "src/main/resources/public",
                "src/main/webapp",
            ];
            if language == SourceLanguage::Javascript {
                dirs.extend([TEMPLATE_DIR, "src/main/js", "src/main/frontend"]);
            }
            Cow::Owned(
                dirs.into_iter()
                    .map(|dir| module.project_dir.join(dir))
                    .collect(),
            )
        }
    }
}
