use std::collections::{BTreeMap, BTreeSet};
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
    /// Applied Kotlin compiler plugins that open classes or add no-arg constructors.
    #[serde(default)]
    pub kotlin_compiler_plugins: Vec<String>,
    #[serde(default)]
    pub kotlin_compiled: bool,
    pub class_dirs: Vec<PathBuf>,
    pub classpath: Vec<PathBuf>,
    pub java_target: Option<u16>,
    pub java_source: Option<u16>,
    pub java_test_source: Option<u16>,
    pub dependencies: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SourceLanguage {
    Java,
    Kotlin,
    Html,
    Template,
    Javascript,
    Typescript,
    Jsp,
    Go,
    Rust,
}

pub const TEMPLATE_DIR: &str = "src/main/resources/templates";

/// Directories whose sources the Go and Rust toolchains do not build as project code.
fn is_vendored(path: &Path, directories: &[&str]) -> bool {
    path.parent().is_some_and(|parent| {
        parent.components().any(|part| {
            part.as_os_str()
                .to_str()
                .is_some_and(|name| directories.contains(&name))
        })
    })
}

fn is_declaration_file(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| {
            [".d.ts", ".d.mts", ".d.cts"]
                .iter()
                .any(|ext| name.ends_with(ext))
        })
}

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
            // Declaration files describe types only; they contain no code to check.
            "ts" | "mts" | "cts" | "tsx" if !is_declaration_file(path) => Some(Self::Typescript),
            "jsp" | "jspx" | "jspf" | "tag" | "tagx" => Some(Self::Jsp),
            "go" if !is_vendored(path, &["vendor", "testdata"]) => Some(Self::Go),
            "rs" if !is_vendored(path, &["target"]) => Some(Self::Rust),
            _ => None,
        }
    }

    pub fn semgrep_name(self) -> &'static str {
        match self {
            Self::Java => "java",
            Self::Kotlin => "kotlin",
            Self::Html | Self::Template => "html",
            Self::Javascript => "javascript",
            Self::Typescript => "typescript",
            Self::Jsp => "regex",
            Self::Go => "go",
            Self::Rust => "rust",
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
    /// `go vet`, run through the `go` command.
    GoVet,
    /// `cargo clippy`.
    Clippy,
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
            Self::GoVet => "go",
            Self::Clippy => "cargo",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Warning,
    Error,
}

/// How reliably a rule's findings are real problems.
///
/// `High` rules match structure that is wrong by policy, so a finding needs
/// fixing. `Medium` rules infer intent from code shape, so a finding needs a
/// judgment: fix it, or suppress it with a reason when the code is correct.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Medium,
    #[default]
    High,
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
    pub confidence: Confidence,
    pub message: String,
    pub fix: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edit: Option<SourceEdit>,
    pub fingerprint: Option<String>,
    #[serde(skip)]
    pub legacy_fingerprints: Vec<String>,
    /// Set when the diagnostic stands for several findings of one cause. Its fingerprint
    /// names the group, so a baseline or `ignore` entry matches by rule and fingerprint
    /// alone, whichever file the group is reported in.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub group: Option<FindingGroup>,
}

/// How many findings a grouped diagnostic stands for, with a few of them.
#[derive(Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct FindingGroup {
    pub occurrences: usize,
    pub examples: Vec<String>,
}

/// The most examples a grouped diagnostic lists, in its message and in `examples`.
pub const GROUP_EXAMPLES: usize = 3;

#[derive(Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct SourceEdit {
    pub start_byte: usize,
    pub end_byte: usize,
    pub replacement: String,
    pub source_sha256: String,
}

pub const PMD_JAVA_DEFAULT: u16 = 21;
pub const PMD_JAVA_VERSIONS: std::ops::RangeInclusive<u16> = 3..=27;

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
    Incomplete,
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
    pub analysis_notes: BTreeSet<String>,
    pub analysis_warnings: Vec<Failure>,
    pub pmd_coverage: PmdCoverage,
    pub scanned_files: BTreeSet<PathBuf>,
    pub requested_files: BTreeSet<PathBuf>,
    pub incomplete_files: BTreeSet<PathBuf>,
    pub scope: Scope,
    pub project_scope: Option<Scope>,
    pub organization_scope: Option<Scope>,
    pub no_changes: bool,
    pub fallback_reason: Option<String>,
    pub suppressed_count: usize,
    pub skipped_test_files: BTreeSet<PathBuf>,
    pub organization_policy_enforced: bool,
}

impl Outcome {
    pub fn merge(&mut self, other: Self) {
        self.diagnostics.extend(other.diagnostics);
        self.failures.extend(other.failures);
        self.analysis_notes.extend(other.analysis_notes);
        self.analysis_warnings.extend(other.analysis_warnings);
        self.pmd_coverage
            .compiled_classpath
            .extend(other.pmd_coverage.compiled_classpath);
        self.pmd_coverage
            .source_only
            .extend(other.pmd_coverage.source_only);
        self.scanned_files.extend(other.scanned_files);
        self.requested_files.extend(other.requested_files);
        self.incomplete_files.extend(other.incomplete_files);
        self.suppressed_count += other.suppressed_count;
        self.skipped_test_files.extend(other.skipped_test_files);
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
        if !self.skipped_test_files.is_empty() {
            self.analysis_notes.insert(format!(
                "{} test source files below src/test are skipped by rules that skip tests, which is the default. Set skip_java_tests: false on a rule to include them.",
                self.skipped_test_files.len()
            ));
        }
        self.diagnostics.sort();
        self.diagnostics.dedup();
        self.failures.sort();
        self.failures.dedup();
        self.analysis_warnings.sort();
        self.analysis_warnings.dedup();
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
            None if !self.analysis_warnings.is_empty() => (Status::Incomplete, 0),
            None if self.no_changes => (Status::NoChanges, 0),
            None => (Status::Passed, 0),
        };
        let mut rule_counts = BTreeMap::<String, RuleCount>::new();
        for diagnostic in &self.diagnostics {
            let count = rule_counts.entry(diagnostic.rule_id.clone()).or_default();
            match diagnostic.severity {
                Severity::Error => count.errors += 1,
                Severity::Warning => count.warnings += 1,
            }
        }
        CheckReport {
            version: 3,
            status,
            exit_code,
            complete: self.failures.is_empty() && self.analysis_warnings.is_empty(),
            scope: self.scope,
            project_scope: self.project_scope,
            organization_scope: self.organization_scope,
            fallback_reason: self.fallback_reason,
            files_requested: self.requested_files.union(&self.scanned_files).count(),
            files_scanned: self
                .scanned_files
                .difference(&self.incomplete_files)
                .count(),
            suppressed_count: self.suppressed_count,
            organization_policy_enforced: self.organization_policy_enforced,
            analysis_notes: self.analysis_notes,
            analysis_warnings: self.analysis_warnings,
            pmd_coverage: self.pmd_coverage,
            rule_counts,
            report_filter: None,
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
    pub project_scope: Option<Scope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub organization_scope: Option<Scope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    pub files_scanned: usize,
    pub files_requested: usize,
    pub suppressed_count: usize,
    pub organization_policy_enforced: bool,
    #[serde(skip_serializing_if = "BTreeSet::is_empty")]
    pub analysis_notes: BTreeSet<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub analysis_warnings: Vec<Failure>,
    #[serde(skip_serializing_if = "PmdCoverage::is_empty")]
    pub pmd_coverage: PmdCoverage,
    pub rule_counts: BTreeMap<String, RuleCount>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_filter: Option<ReportFilter>,
    pub diagnostics: Vec<Diagnostic>,
    pub failures: Vec<Failure>,
}

#[derive(Default, Serialize)]
pub struct PmdCoverage {
    pub compiled_classpath: BTreeSet<String>,
    pub source_only: BTreeSet<String>,
}

impl PmdCoverage {
    fn is_empty(&self) -> bool {
        self.compiled_classpath.is_empty() && self.source_only.is_empty()
    }
}

#[derive(Serialize)]
pub struct ReportFilter {
    pub rules: Vec<String>,
    pub paths: Vec<String>,
    pub omitted: usize,
}

impl CheckReport {
    pub fn filter_diagnostics(&mut self, rules: Vec<String>, paths: Vec<PathBuf>) {
        if rules.is_empty() && paths.is_empty() {
            return;
        }
        let total = self.diagnostics.len();
        self.diagnostics.retain(|finding| {
            (rules.is_empty() || rules.contains(&finding.rule_id))
                && (paths.is_empty()
                    || finding.path.as_deref().is_some_and(|path| {
                        paths.iter().any(|filter| {
                            let relative = filter.strip_prefix(".").unwrap_or(filter);
                            Path::new(path).starts_with(relative)
                        })
                    }))
        });
        self.report_filter = Some(ReportFilter {
            rules,
            paths: paths
                .iter()
                .map(|path| path.to_string_lossy().replace('\\', "/"))
                .collect(),
            omitted: total - self.diagnostics.len(),
        });
    }
}

#[derive(Default, Serialize)]
pub struct RuleCount {
    pub errors: usize,
    pub warnings: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn go_and_rust_sources_exclude_vendored_and_build_output() {
        assert_eq!(
            SourceLanguage::of(Path::new("cmd/main.go")),
            Some(SourceLanguage::Go)
        );
        assert_eq!(
            SourceLanguage::of(Path::new("a_test.go")),
            Some(SourceLanguage::Go)
        );
        assert_eq!(SourceLanguage::of(Path::new("vendor/x/y.go")), None);
        assert_eq!(SourceLanguage::of(Path::new("pkg/testdata/z.go")), None);
        assert_eq!(
            SourceLanguage::of(Path::new("src/lib.rs")),
            Some(SourceLanguage::Rust)
        );
        assert_eq!(
            SourceLanguage::of(Path::new("target/debug/build/out.rs")),
            None
        );
    }

    #[test]
    fn typescript_sources_exclude_declaration_files() {
        for name in ["a.ts", "a.mts", "a.cts", "a.tsx", "a.test.ts"] {
            assert_eq!(
                SourceLanguage::of(Path::new(name)),
                Some(SourceLanguage::Typescript),
                "{name}"
            );
        }
        for name in ["a.d.ts", "a.d.mts", "a.d.cts"] {
            assert_eq!(SourceLanguage::of(Path::new(name)), None, "{name}");
        }
        assert_eq!(
            SourceLanguage::of(Path::new("a.js")),
            Some(SourceLanguage::Javascript)
        );
    }

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
