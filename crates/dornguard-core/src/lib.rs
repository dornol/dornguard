use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectModel {
    pub schema_version: u32,
    pub modules: Vec<Module>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Module {
    pub path: String,
    pub name: String,
    pub project_dir: PathBuf,
    pub source_dirs: Vec<PathBuf>,
    #[serde(default)]
    pub test_source_dirs: Vec<PathBuf>,
    #[serde(default)]
    pub compile_sources: BTreeSet<PathBuf>,
    pub kotlin_source_dirs: Vec<PathBuf>,
    pub class_dirs: Vec<PathBuf>,
    pub classpath: Vec<PathBuf>,
    pub java_target: Option<u16>,
    pub dependencies: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SourceLanguage {
    Java,
    Kotlin,
    Html,
    Template,
    Javascript,
    Jsp,
}

pub const TEMPLATE_DIR: &str = "src/main/resources/templates";

impl SourceLanguage {
    pub fn of(path: &Path) -> Option<Self> {
        match path.extension()?.to_str()? {
            "java" => Some(Self::Java),
            "kt" => Some(Self::Kotlin),
            "html" | "htm" if path.ancestors().any(|p| p.ends_with(TEMPLATE_DIR)) => {
                Some(Self::Template)
            }
            "html" | "htm" => Some(Self::Html),
            "js" | "mjs" | "cjs" => Some(Self::Javascript),
            "jsp" | "jspx" | "jspf" | "tag" | "tagx" => Some(Self::Jsp),
            _ => None,
        }
    }

    pub fn semgrep_name(self) -> &'static str {
        match self {
            Self::Java => "java",
            Self::Kotlin => "kotlin",
            Self::Html | Self::Template => "html",
            Self::Javascript => "javascript",
            Self::Jsp => "regex",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineName {
    Semgrep,
    Pmd,
    Eslint,
    Builtin,
    Archunit,
    Gradle,
}

impl EngineName {
    pub fn executable(self) -> &'static str {
        match self {
            Self::Semgrep => "semgrep",
            Self::Pmd => "pmd",
            Self::Eslint => "eslint",
            Self::Builtin => "builtin",
            Self::Archunit => "java",
            Self::Gradle => "gradlew",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Diagnostic {
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class_name: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_line: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_column: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub rule_id: String,
    pub engine: EngineName,
    pub severity: Severity,
    pub message: String,
    pub fix: String,
    pub fingerprint: Option<String>,
}

pub fn fingerprint(identity: &str) -> String {
    format!("{:x}", Sha256::digest(identity.as_bytes()))
}

pub fn valid_fingerprint(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureKind {
    EngineError,
    PreparationError,
    ConfigurationError,
}

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct Failure {
    pub kind: FailureKind,
    pub engine: Option<EngineName>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<FailureStage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub build_tool: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_type: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub modules: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub rule_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureStage {
    Engine,
    ProjectModel,
    ProjectCompile,
}

impl Failure {
    pub fn new(kind: FailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            engine: None,
            message: message.into(),
            stage: None,
            build_tool: None,
            file_count: None,
            path: None,
            error_type: None,
            modules: Vec::new(),
            rule_ids: Vec::new(),
        }
    }

    pub fn set_rules<'a>(&mut self, rules: impl IntoIterator<Item = &'a str>) {
        self.rule_ids = rules.into_iter().map(str::to_owned).collect();
        self.rule_ids.sort();
        self.rule_ids.dedup();
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Passed,
    NoChanges,
    Violations,
    ConfigurationError,
    EngineError,
    PreparationError,
}

#[derive(Clone, Copy, Default, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Scope {
    #[default]
    Full,
    Selected,
    Changed,
}

#[derive(Default)]
pub struct Outcome {
    pub diagnostics: Vec<Diagnostic>,
    pub failures: Vec<Failure>,
    pub scanned_files: BTreeSet<PathBuf>,
    pub requested_files: BTreeSet<PathBuf>,
    pub incomplete_files: BTreeSet<PathBuf>,
    pub scope: Scope,
    pub no_changes: bool,
    pub fallback_reason: Option<String>,
    pub suppressed_count: usize,
    pub organization_policy_enforced: bool,
}

impl Outcome {
    pub fn merge(&mut self, other: Self) {
        self.diagnostics.extend(other.diagnostics);
        self.failures.extend(other.failures);
        self.scanned_files.extend(other.scanned_files);
        self.requested_files.extend(other.requested_files);
        self.incomplete_files.extend(other.incomplete_files);
        self.suppressed_count += other.suppressed_count;
    }

    pub fn record_coverage<'a>(&mut self, files: impl IntoIterator<Item = &'a PathBuf>) {
        for file in files {
            self.requested_files.insert(file.clone());
            if !self.scanned_files.contains(file) {
                self.incomplete_files.insert(file.clone());
            }
        }
    }

    pub fn finish(mut self) -> CheckReport {
        self.diagnostics.sort();
        self.diagnostics.dedup();
        self.failures.sort();
        self.failures.dedup();
        let (status, exit_code) = match self.failures.last().map(|failure| failure.kind) {
            Some(FailureKind::ConfigurationError) => (Status::ConfigurationError, 2),
            Some(FailureKind::PreparationError) => (Status::PreparationError, 4),
            Some(FailureKind::EngineError) => (Status::EngineError, 3),
            None if self
                .diagnostics
                .iter()
                .any(|item| item.severity == Severity::Error) =>
            {
                (Status::Violations, 1)
            }
            None if self.no_changes => (Status::NoChanges, 0),
            None => (Status::Passed, 0),
        };
        CheckReport {
            version: 3,
            status,
            exit_code,
            complete: self.failures.is_empty(),
            scope: self.scope,
            fallback_reason: self.fallback_reason,
            files_requested: self.requested_files.union(&self.scanned_files).count(),
            files_scanned: self
                .scanned_files
                .difference(&self.incomplete_files)
                .count(),
            suppressed_count: self.suppressed_count,
            organization_policy_enforced: self.organization_policy_enforced,
            diagnostics: self.diagnostics,
            failures: self.failures,
        }
    }
}

#[derive(Serialize)]
pub struct CheckReport {
    pub version: u8,
    pub status: Status,
    pub exit_code: u8,
    pub complete: bool,
    pub scope: Scope,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    pub files_scanned: usize,
    pub files_requested: usize,
    pub suppressed_count: usize,
    pub organization_policy_enforced: bool,
    pub diagnostics: Vec<Diagnostic>,
    pub failures: Vec<Failure>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_use_policy_precedence() {
        let failures = [
            FailureKind::PreparationError,
            FailureKind::EngineError,
            FailureKind::ConfigurationError,
        ];
        let report = Outcome {
            failures: failures
                .into_iter()
                .map(|kind| Failure::new(kind, "failed"))
                .collect(),
            ..Outcome::default()
        }
        .finish();
        assert_eq!(report.exit_code, 2);
        assert!(!report.complete);
    }
}
