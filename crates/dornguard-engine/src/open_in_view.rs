//! `spring.open-in-view-enabled`: a Spring Boot application with Spring Data JPA
//! that leaves `spring.jpa.open-in-view` at its default of `true`.

use std::path::{Path, PathBuf};

use dornguard_config::Rule;
use dornguard_core::{Diagnostic, EngineName, Outcome};

use crate::source::{Source, fragment_fingerprint};
use crate::{Request, display_path};

const BUILD_FILES: [&str; 3] = ["pom.xml", "build.gradle", "build.gradle.kts"];
const CONFIG_FILES: [&str; 3] = [
    "application.properties",
    "application.yml",
    "application.yaml",
];

pub(super) fn check(request: Request<'_>, rule: &Rule) -> Outcome {
    let mut outcome = Outcome::default();
    for file in request.files {
        let Ok(text) = std::fs::read_to_string(request.root.join(file)) else {
            continue;
        };
        let source = Source::new(text);
        let Some(offset) = application_annotation(source.text()) else {
            continue;
        };
        let Some(module) = module_dir(file) else {
            continue;
        };
        if !uses_data_jpa(request.root, &module)
            || open_in_view_disabled(&request.root.join(&module).join("src/main/resources"))
        {
            continue;
        }
        let Some((line, column)) = source.position(offset) else {
            continue;
        };
        let annotation = "@SpringBootApplication";
        let end = source.position(offset + annotation.len());
        outcome.diagnostics.push(Diagnostic {
            path: Some(display_path(file)),
            module: None,
            class_name: None,
            line: Some(line),
            column: Some(column),
            end_line: end.map(|(line, _)| line),
            end_column: end.map(|(_, column)| column),
            subject: None,
            rule_id: rule.id.clone(),
            engine: EngineName::Builtin,
            severity: rule.severity,
            confidence: rule.confidence,
            message: rule.description.clone(),
            fix: rule.fix.clone(),
            edit: None,
            legacy_fingerprints: Vec::new(),
            group: None,
            fingerprint: fragment_fingerprint(&format!("{annotation}\n{}", display_path(file))),
        });
    }
    outcome
}

/// The offset of a `@SpringBootApplication` annotation outside comments.
fn application_annotation(text: &str) -> Option<usize> {
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if !trimmed.starts_with("//")
            && !trimmed.starts_with('*')
            && let Some(index) = line.find("@SpringBootApplication")
            && !line[index + "@SpringBootApplication".len()..]
                .starts_with(|c: char| c.is_alphanumeric() || c == '_')
        {
            return Some(offset + index);
        }
        offset += line.len();
    }
    None
}

/// The module directory of a main source file: the part before `src/main/java`.
fn module_dir(file: &Path) -> Option<PathBuf> {
    let parts = file.components().collect::<Vec<_>>();
    let main = parts.windows(3).position(|window| {
        window[0].as_os_str() == "src"
            && window[1].as_os_str() == "main"
            && window[2].as_os_str() == "java"
    })?;
    Some(parts[..main].iter().collect())
}

/// Spring Boot enables open-in-view only with Spring Data JPA on the classpath. The
/// module's build file, or a parent build file that manages its dependencies, names it.
fn uses_data_jpa(root: &Path, module: &Path) -> bool {
    module.ancestors().any(|directory| {
        BUILD_FILES.iter().any(|name| {
            std::fs::read_to_string(root.join(directory).join(name))
                .is_ok_and(|text| text.contains("spring-boot-starter-data-jpa"))
        })
    })
}

fn open_in_view_disabled(resources: &Path) -> bool {
    CONFIG_FILES.iter().any(|name| {
        std::fs::read_to_string(resources.join(name)).is_ok_and(|text| {
            if name.ends_with(".properties") {
                properties_disable(&text)
            } else {
                yaml_disables(&text)
            }
        })
    })
}

/// Spring's relaxed binding accepts `open-in-view`, `openInView` and `open_in_view`.
fn is_open_in_view(key: &str) -> bool {
    let normalized = key
        .chars()
        .filter(|c| !matches!(c, '-' | '_'))
        .collect::<String>()
        .to_ascii_lowercase();
    normalized == "spring.jpa.openinview"
}

fn properties_disable(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim();
        if line.starts_with(['#', '!']) {
            return false;
        }
        let Some(split) = line.find(['=', ':']) else {
            return false;
        };
        is_open_in_view(line[..split].trim()) && line[split + 1..].trim() == "false"
    })
}

/// Read nested and dotted keys from the YAML documents that apply to every
/// profile. Documents activated by `spring.config.activate.on-profile` do not count.
fn yaml_disables(text: &str) -> bool {
    let mut disabled = false;
    for document in text.split("\n---") {
        let mut keys: Vec<(usize, String)> = Vec::new();
        let mut document_disables = false;
        let mut profile_specific = false;
        for line in document.lines() {
            let content = line.split(" #").next().unwrap_or_default();
            if content.trim().is_empty() || content.trim_start().starts_with('#') {
                continue;
            }
            let indent = content.len() - content.trim_start().len();
            let Some((key, value)) = content.trim().split_once(':') else {
                continue;
            };
            if key.starts_with('-') {
                continue;
            }
            while keys.last().is_some_and(|(level, _)| *level >= indent) {
                keys.pop();
            }
            let path = keys
                .iter()
                .map(|(_, key)| key.as_str())
                .chain([key.trim().trim_matches(['"', '\''])])
                .collect::<Vec<_>>()
                .join(".");
            let value = value.trim().trim_matches(['"', '\'']);
            if value.is_empty() {
                keys.push((indent, key.trim().trim_matches(['"', '\'']).to_owned()));
            } else if is_open_in_view(&path) && value == "false" {
                document_disables = true;
            } else if path
                .replace('_', "-")
                .eq_ignore_ascii_case("spring.config.activate.on-profile")
                || path.eq_ignore_ascii_case("spring.profiles")
            {
                profile_specific = true;
            }
        }
        disabled |= document_disables && !profile_specific;
    }
    disabled
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_reads_nested_dotted_and_relaxed_keys() {
        assert!(yaml_disables("spring:\n  jpa:\n    open-in-view: false\n"));
        assert!(yaml_disables("spring:\n  jpa.openInView: false # off\n"));
        assert!(yaml_disables("spring.jpa.open_in_view: 'false'\n"));
        assert!(!yaml_disables("spring:\n  jpa:\n    open-in-view: true\n"));
        assert!(!yaml_disables(
            "spring:\n  jpa:\n    show-sql: true\n  open-in-view: false\n"
        ));
    }

    #[test]
    fn a_profile_document_does_not_disable_it_for_every_profile() {
        let text = "spring:\n  application:\n    name: app\n---\nspring:\n  config:\n    activate:\n      on-profile: prod\n  jpa:\n    open-in-view: false\n";
        assert!(!yaml_disables(text));
    }

    #[test]
    fn properties_accept_both_separators_and_skip_comments() {
        assert!(properties_disable("spring.jpa.open-in-view=false\n"));
        assert!(properties_disable("spring.jpa.open-in-view : false\n"));
        assert!(!properties_disable("#spring.jpa.open-in-view=false\n"));
        assert!(!properties_disable("spring.jpa.open-in-view=true\n"));
    }

    #[test]
    fn the_annotation_is_found_outside_comments_and_imports() {
        let text = "import org.springframework.boot.autoconfigure.SpringBootApplication;\n// @SpringBootApplication\n@SpringBootApplication\nclass App {}\n";
        assert_eq!(
            application_annotation(text),
            Some(text.rfind("@Spring").unwrap())
        );
        assert_eq!(
            application_annotation("@SpringBootApplicationX class A {}"),
            None
        );
    }

    #[test]
    fn the_module_is_the_directory_above_main_sources() {
        assert_eq!(
            module_dir(Path::new("api/src/main/java/a/App.java")),
            Some(PathBuf::from("api"))
        );
        assert_eq!(
            module_dir(Path::new("src/main/java/App.java")),
            Some(PathBuf::new())
        );
        assert_eq!(module_dir(Path::new("src/test/java/App.java")), None);
    }
}
