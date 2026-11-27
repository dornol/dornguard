use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use dornguard_core::{Confidence, EngineName, Severity, SourceLanguage};
use globset::{GlobBuilder, GlobMatcher};
use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};
use serde::Deserialize;

mod format;
mod ignore;
mod ruleset;
pub use format::{
    ControlSpacing, FileFormat, FormatConfig, IndentStyle, LineEnding, PartialParsing,
};
pub use ignore::Ignore;
use ignore::ignore_rule_matches;
use ruleset::{PolicySource, Ruleset, Source};

pub const CONFIG_FILE: &str = ".dornguard.yml";
pub const DEFAULT_CONFIG: &str = "version: 1\nprofiles: [java]\n";
pub const BUILTIN_VERSION: &str = "1.6.0";
pub const FLYWAY_RULE: &str = "flyway.changed-versioned-migration";
pub const JPA_RESPONSE_RULE: &str = "spring.jpa-entity-response";
pub const TRANSACTIONAL_SELF_RULE: &str = "spring.transactional-self-invocation";
pub const ASYNC_SELF_RULE: &str = "spring.async-self-invocation";
pub const CACHE_SELF_RULE: &str = "spring.cache-self-invocation";
pub const RETRY_SELF_RULE: &str = "spring.retry-self-invocation";
pub const IGNORED_RETURN_VALUE_RULE: &str = "java.ignored-return-value";
pub const UNNECESSARY_IMPORT_RULE: &str = "java.unnecessary-import";
pub const RESOURCE_RULE: &str = "java.resource-not-closed";
pub const PRESERVE_STACK_TRACE_RULE: &str = "java.preserve-stack-trace";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    version: u32,
    #[serde(default)]
    profiles: Vec<String>,
    #[serde(default)]
    ruleset: Ruleset,
    #[serde(default)]
    pub modules: BTreeMap<String, ModuleConfig>,
    #[serde(default)]
    pub module_dependencies: ModuleDependencies,
    #[serde(default)]
    pub ignore: Vec<Ignore>,
    #[serde(default)]
    pub format: FormatConfig,
    #[serde(default)]
    exclude: Vec<PathGlob>,
    pub java_target: Option<u16>,
    pub java_source: Option<u16>,
    #[serde(default)]
    pub allow_logged_exceptions: bool,
    #[serde(default)]
    pub java_resource_owners: Vec<String>,
    #[serde(default)]
    pub java_managed_resource_factories: Vec<String>,
    #[serde(default)]
    pub thymeleaf_sanitizers: Vec<String>,
    #[serde(default)]
    pub thymeleaf_partial_parsing: PartialParsing,
    /// Tolerate partial parsing of any source file when set to `warning`.
    #[serde(default)]
    pub partial_parsing: PartialParsing,
    /// Files whose partial parsing is a known limitation of the parser.
    #[serde(default)]
    partial_parsing_files: Vec<PartialParsingFile>,
    #[serde(default)]
    rules: BTreeMap<String, RuleOverride>,
}
pub(crate) struct PathGlob(pub(crate) GlobMatcher);

impl PathGlob {
    fn new(pattern: &str) -> std::result::Result<Self, String> {
        if !project_relative(pattern) {
            return Err("Expected a project-relative glob with forward slashes".into());
        }
        GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
            .map(|glob| Self(glob.compile_matcher()))
            .map_err(|error| error.to_string())
    }
}

impl<'de> Deserialize<'de> for PathGlob {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let pattern = String::deserialize(deserializer)?;
        Self::new(&pattern).map_err(serde::de::Error::custom)
    }
}

/// A file allowed to be analyzed only partly, with the reason it is acceptable.
#[derive(Deserialize)]
#[serde(try_from = "PartialParsingEntry")]
pub struct PartialParsingFile(PathGlob);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PartialParsingEntry {
    path: PathGlob,
    reason: String,
}

impl TryFrom<PartialParsingEntry> for PartialParsingFile {
    type Error = String;

    fn try_from(entry: PartialParsingEntry) -> std::result::Result<Self, Self::Error> {
        if entry.reason.trim().is_empty() {
            return Err(format!(
                "partial_parsing_files entry for {} requires a reason",
                entry.path.0.glob()
            ));
        }
        Ok(Self(entry.path))
    }
}

/// Which files may be reported as incompletely analyzed without failing the check.
#[derive(Clone, Default)]
pub struct PartialParsingPolicy {
    all: bool,
    files: Vec<globset::GlobMatcher>,
}

impl PartialParsingPolicy {
    pub fn tolerates(&self, path: &Path) -> bool {
        self.all || self.files.iter().any(|file| file.is_match(path))
    }
}

impl ProjectConfig {
    pub fn partial_parsing_policy(&self) -> PartialParsingPolicy {
        PartialParsingPolicy {
            all: self.partial_parsing == PartialParsing::Warning,
            files: self
                .partial_parsing_files
                .iter()
                .map(|file| file.0.0.clone())
                .collect(),
        }
    }
}

pub(crate) fn project_relative(path: &str) -> bool {
    !path.contains(['\\', ':']) && path.split('/').all(|part| !matches!(part, "" | "." | ".."))
}

fn identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
}

fn qualified_identifier(name: &str) -> bool {
    name.split('.').all(identifier)
}

fn resource_signature(signature: &str) -> Option<(&str, &str)> {
    let (member, parameters) = signature.strip_suffix(')')?.split_once('(')?;
    let (owner, method) = member.rsplit_once('#')?;
    (qualified_identifier(owner) && owner.contains('.') && identifier(method))
        .then_some((method, parameters))
}

impl ProjectConfig {
    pub fn excludes_source(&self, file: &Path) -> bool {
        self.exclude.iter().any(|glob| glob.0.is_match(file))
    }

    pub fn use_only_cli_profiles(&mut self) {
        self.profiles.clear();
        self.module_dependencies.forbid.clear();
        for module in self.modules.values_mut() {
            module.profiles.clear();
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleConfig {
    #[serde(default)]
    pub profiles: Vec<String>,
    #[serde(default)]
    rules: BTreeMap<String, RuleOverride>,
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModuleDependencies {
    #[serde(default)]
    pub forbid: Vec<Dependency>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Dependency {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub extends: Vec<String>,
    #[serde(default)]
    pub rules: Vec<String>,
    #[serde(default)]
    pub rule_defaults: BTreeMap<String, RuleOverride>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RuleOverride {
    enabled: Option<bool>,
    /// Set to `false` to check test sources, which rules skip by default.
    skip_java_tests: Option<bool>,
    severity: Option<Severity>,
    options: Option<ArchitectureOptions>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    pub id: String,
    pub engine: EngineName,
    pub severity: Severity,
    #[serde(default)]
    pub confidence: Confidence,
    pub description: String,
    pub fix: String,
    implementation: Option<Implementation>,
    pub options: Option<ArchitectureOptions>,
    #[serde(default)]
    all_html: bool,
    #[serde(default)]
    jsp_only: bool,
    #[serde(default = "default_skip_tests")]
    skip_java_tests: bool,
    #[serde(skip)]
    semgrep_languages: Vec<String>,
}

fn default_skip_tests() -> bool {
    true
}

/// A source file below a `src/test` directory.
pub fn is_test_source(path: &Path) -> bool {
    let names = path
        .parent()
        .into_iter()
        .flat_map(Path::components)
        .collect::<Vec<_>>();
    names
        .windows(2)
        .any(|pair| pair[0].as_os_str() == "src" && pair[1].as_os_str() == "test")
}

impl Rule {
    pub fn applies_to(&self, language: SourceLanguage) -> bool {
        if self.jsp_only != (language == SourceLanguage::Jsp) {
            return false;
        }
        if language == SourceLanguage::Html && !self.all_html {
            return false;
        }
        match self.engine {
            EngineName::Eslint => language == SourceLanguage::Javascript,
            EngineName::Builtin if self.is_format_rule() => matches!(
                language,
                SourceLanguage::Java | SourceLanguage::Kotlin | SourceLanguage::Javascript
            ),
            EngineName::Builtin if self.id == FLYWAY_RULE => false,
            EngineName::Pmd | EngineName::Builtin => language == SourceLanguage::Java,
            EngineName::Semgrep => self.semgrep_languages.iter().any(|name| {
                name == language.semgrep_name()
                    || (matches!(language, SourceLanguage::Java | SourceLanguage::Kotlin)
                        && matches!(name.as_str(), "generic" | "regex"))
            }),
            _ => false,
        }
    }

    pub fn uses_pmd_classpath(&self) -> bool {
        matches!(
            self.id.as_str(),
            UNNECESSARY_IMPORT_RULE
                | RESOURCE_RULE
                | PRESERVE_STACK_TRACE_RULE
                | IGNORED_RETURN_VALUE_RULE
        )
    }

    pub fn is_format_rule(&self) -> bool {
        self.engine == EngineName::Builtin && self.id.starts_with("format.")
    }

    pub fn enables_kotlin(&self) -> bool {
        self.semgrep_languages.iter().any(|name| name == "kotlin")
    }

    pub fn skips_tests(&self) -> bool {
        self.skip_java_tests
    }

    pub fn excludes_path(&self, path: &Path) -> bool {
        self.skip_java_tests && is_test_source(path)
    }

    pub fn applies_to_java_tests(&self) -> bool {
        !self.skip_java_tests && self.applies_to(SourceLanguage::Java)
    }

    pub fn implementation(&self) -> Result<&Implementation> {
        self.implementation
            .as_ref()
            .ok_or_else(|| miette!("Rule {} is missing its engine implementation", self.id))
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ArchitectureOptions {
    #[serde(default)]
    pub from: Vec<String>,
    #[serde(default)]
    pub forbidden: Vec<String>,
    /// Report dependencies that form package cycles instead of forbidden packages.
    #[serde(default)]
    pub cycles: bool,
}

impl ArchitectureOptions {
    /// An architecture rule names either forbidden packages or package cycles.
    pub fn is_valid(&self) -> bool {
        self.forbidden.is_empty() == self.cycles
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Implementation {
    #[serde(default)]
    pub config: String,
    pub rule_id: String,
}

const BUILTIN_PROFILES: &[&str] = &[
    include_str!("../assets/profiles/java-source.yml"),
    include_str!("../assets/profiles/java.yml"),
    include_str!("../assets/profiles/java-review.yml"),
    include_str!("../assets/profiles/format.yml"),
    include_str!("../assets/profiles/jspecify.yml"),
    include_str!("../assets/profiles/kotlin.yml"),
    include_str!("../assets/profiles/security.yml"),
    include_str!("../assets/profiles/spring-boot.yml"),
    include_str!("../assets/profiles/spring-rollback.yml"),
    include_str!("../assets/profiles/thymeleaf.yml"),
    include_str!("../assets/profiles/thymeleaf-output-review.yml"),
    include_str!("../assets/profiles/hexagonal.yml"),
    include_str!("../assets/profiles/layered.yml"),
    include_str!("../assets/profiles/package-cycles.yml"),
    include_str!("../assets/profiles/java-library.yml"),
    include_str!("../assets/profiles/rest-api.yml"),
    include_str!("../assets/profiles/flyway.yml"),
    include_str!("../assets/profiles/jpa-web.yml"),
    include_str!("../assets/profiles/scheduler.yml"),
    include_str!("../assets/profiles/html.yml"),
    include_str!("../assets/profiles/javascript.yml"),
    include_str!("../assets/profiles/jsp.yml"),
];

const BUILTIN_RULES: &[&str] = &[
    include_str!("../assets/rules/format.indentation.yml"),
    include_str!("../assets/rules/format.line-ending.yml"),
    include_str!("../assets/rules/format.final-newline.yml"),
    include_str!("../assets/rules/format.trailing-whitespace.yml"),
    include_str!("../assets/rules/format.blank-lines.yml"),
    include_str!("../assets/rules/format.control-spacing.yml"),
    include_str!("../assets/rules/java.stdout-usage.yml"),
    include_str!("../assets/rules/flyway.changed-versioned-migration.yml"),
    include_str!("../assets/rules/spring.jpa-entity-response.yml"),
    include_str!("../assets/rules/java.empty-catch.yml"),
    include_str!("../assets/rules/java.ignored-return-value.yml"),
    include_str!("../assets/rules/java.assert-throws-nested-call.yml"),
    include_str!("../assets/rules/java.jspecify-null-marked.yml"),
    include_str!("../assets/rules/kotlin.stdout-usage.yml"),
    include_str!("../assets/rules/security.cors-wildcard.yml"),
    include_str!("../assets/rules/spring.field-injection.yml"),
    include_str!("../assets/rules/thymeleaf.restricted-bean-access.yml"),
    include_str!("../assets/rules/architecture.domain-independence.yml"),
    include_str!("../assets/rules/architecture.application-independence.yml"),
    include_str!("../assets/rules/architecture.adapter-isolation.yml"),
    include_str!("../assets/rules/architecture.framework-independence.yml"),
    include_str!("../assets/rules/architecture.controller-repository.yml"),
    include_str!("../assets/rules/architecture.repository-independence.yml"),
    include_str!("../assets/rules/architecture.package-cycle.yml"),
    include_str!("../assets/rules/security.sql-injection.yml"),
    include_str!("../assets/rules/security.weak-crypto.yml"),
    include_str!("../assets/rules/security.disabled-tls-verification.yml"),
    include_str!("../assets/rules/security.csrf-disabled.yml"),
    include_str!("../assets/rules/architecture.module-dependency.yml"),
    include_str!("../assets/rules/architecture.library-framework-independence.yml"),
    include_str!("../assets/rules/java.ignored-interrupt.yml"),
    include_str!("../assets/rules/kotlin.run-blocking-in-suspend.yml"),
    include_str!("../assets/rules/kotlin.global-scope.yml"),
    include_str!("../assets/rules/spring.private-transactional.yml"),
    include_str!("../assets/rules/spring.transactional-self-invocation.yml"),
    include_str!("../assets/rules/java.unnecessary-import.yml"),
    include_str!("../assets/rules/spring.shared-simple-date-format.yml"),
    include_str!("../assets/rules/java.preserve-stack-trace.yml"),
    include_str!("../assets/rules/java.resource-not-closed.yml"),
    include_str!("../assets/rules/kotlin.swallowed-cancellation.yml"),
    include_str!("../assets/rules/spring.singleton-request-state.yml"),
    include_str!("../assets/rules/spring.checked-exception-rollback.yml"),
    include_str!("../assets/rules/java.fully-qualified-type.yml"),
    include_str!("../assets/rules/kotlin.fully-qualified-type.yml"),
    include_str!("../assets/rules/html.inline-script.yml"),
    include_str!("../assets/rules/html.inline-event-handler.yml"),
    include_str!("../assets/rules/javascript.no-eval.yml"),
    include_str!("../assets/rules/javascript.no-implied-eval.yml"),
    include_str!("../assets/rules/javascript.no-new-func.yml"),
    include_str!("../assets/rules/java.bigdecimal-from-floating-point-literal.yml"),
    include_str!("../assets/rules/java.return-from-finally.yml"),
    include_str!("../assets/rules/spring.async-self-invocation.yml"),
    include_str!("../assets/rules/spring.cache-self-invocation.yml"),
    include_str!("../assets/rules/spring.retry-self-invocation.yml"),
    include_str!("../assets/rules/java.print-stack-trace.yml"),
    include_str!("../assets/rules/kotlin.print-stack-trace.yml"),
    include_str!("../assets/rules/kotlin.transactional-suspend.yml"),
    include_str!("../assets/rules/kotlin.sql-injection.yml"),
    include_str!("../assets/rules/kotlin.thread-sleep-in-suspend.yml"),
    include_str!("../assets/rules/thymeleaf.unescaped-output.yml"),
    include_str!("../assets/rules/java.broken-null-check.yml"),
    include_str!("../assets/rules/java.equals-hashcode-contract.yml"),
    include_str!("../assets/rules/javascript.no-async-promise-executor.yml"),
    include_str!("../assets/rules/jsp.scripting-element.yml"),
    include_str!("../assets/rules/jsp.unescaped-output.yml"),
    include_str!("../assets/rules/kotlin.run-catching-cancellation.yml"),
    include_str!("../assets/rules/java.date-pattern-typo.yml"),
    include_str!("../assets/rules/spring.static-member-injection.yml"),
    include_str!("../assets/rules/security.jackson-default-typing.yml"),
    include_str!("../assets/rules/kotlin.date-pattern-typo.yml"),
    include_str!("../assets/rules/java.log-format-arguments.yml"),
    include_str!("../assets/rules/java.string-reference-equality.yml"),
    include_str!("../assets/rules/java.throw-from-finally.yml"),
    include_str!("../assets/rules/java.double-checked-locking.yml"),
    include_str!("../assets/rules/java.lock-without-finally.yml"),
    include_str!("../assets/rules/kotlin.field-injection.yml"),
    include_str!("../assets/rules/kotlin.private-transactional.yml"),
    include_str!("../assets/rules/kotlin.jpa-data-class-entity.yml"),
    include_str!("../assets/rules/javascript.no-unsafe-finally.yml"),
    include_str!("../assets/rules/javascript.no-self-compare.yml"),
    include_str!("../assets/rules/javascript.no-constant-binary-expression.yml"),
    include_str!("../assets/rules/javascript.no-unsafe-negation.yml"),
    include_str!("../assets/rules/javascript.no-unsafe-optional-chaining.yml"),
    include_str!("../assets/rules/javascript.use-isnan.yml"),
    include_str!("../assets/rules/javascript.valid-typeof.yml"),
    include_str!("../assets/rules/javascript.no-script-url.yml"),
];

const BUILTIN_ENGINE_CONFIGS: &[(&str, &str)] = &[
    (
        "rules/semgrep/kotlin/field-injection.yml",
        include_str!("../assets/rules/semgrep/kotlin/field-injection.yml"),
    ),
    (
        "rules/semgrep/kotlin/private-transactional.yml",
        include_str!("../assets/rules/semgrep/kotlin/private-transactional.yml"),
    ),
    (
        "rules/semgrep/kotlin/jpa-data-class-entity.yml",
        include_str!("../assets/rules/semgrep/kotlin/jpa-data-class-entity.yml"),
    ),
    (
        "rules/pmd/java/log-format-arguments.xml",
        include_str!("../assets/rules/pmd/java/log-format-arguments.xml"),
    ),
    (
        "rules/pmd/java/string-reference-equality.xml",
        include_str!("../assets/rules/pmd/java/string-reference-equality.xml"),
    ),
    (
        "rules/pmd/java/throw-from-finally.xml",
        include_str!("../assets/rules/pmd/java/throw-from-finally.xml"),
    ),
    (
        "rules/pmd/java/double-checked-locking.xml",
        include_str!("../assets/rules/pmd/java/double-checked-locking.xml"),
    ),
    (
        "rules/semgrep/java/lock-without-finally.yml",
        include_str!("../assets/rules/semgrep/java/lock-without-finally.yml"),
    ),
    (
        "rules/semgrep/kotlin/date-pattern-typo.yml",
        include_str!("../assets/rules/semgrep/kotlin/date-pattern-typo.yml"),
    ),
    (
        "rules/semgrep/java/date-pattern-typo.yml",
        include_str!("../assets/rules/semgrep/java/date-pattern-typo.yml"),
    ),
    (
        "rules/semgrep/spring/static-member-injection.yml",
        include_str!("../assets/rules/semgrep/spring/static-member-injection.yml"),
    ),
    (
        "rules/semgrep/security/jackson-default-typing.yml",
        include_str!("../assets/rules/semgrep/security/jackson-default-typing.yml"),
    ),
    (
        "rules/pmd/java/ignored-return-value.xml",
        include_str!("../assets/rules/pmd/java/ignored-return-value.xml"),
    ),
    (
        "rules/semgrep/java/assert-throws-nested-call.yml",
        include_str!("../assets/rules/semgrep/java/assert-throws-nested-call.yml"),
    ),
    (
        "rules/pmd/java/broken-null-check.xml",
        include_str!("../assets/rules/pmd/java/broken-null-check.xml"),
    ),
    (
        "rules/pmd/java/equals-hashcode-contract.xml",
        include_str!("../assets/rules/pmd/java/equals-hashcode-contract.xml"),
    ),
    (
        "rules/semgrep/jsp/scripting-element.yml",
        include_str!("../assets/rules/semgrep/jsp/scripting-element.yml"),
    ),
    (
        "rules/semgrep/jsp/unescaped-output.yml",
        include_str!("../assets/rules/semgrep/jsp/unescaped-output.yml"),
    ),
    (
        "rules/semgrep/kotlin/run-catching-cancellation.yml",
        include_str!("../assets/rules/semgrep/kotlin/run-catching-cancellation.yml"),
    ),
    (
        "rules/pmd/java/bigdecimal-from-floating-point-literal.xml",
        include_str!("../assets/rules/pmd/java/bigdecimal-from-floating-point-literal.xml"),
    ),
    (
        "rules/pmd/java/return-from-finally.xml",
        include_str!("../assets/rules/pmd/java/return-from-finally.xml"),
    ),
    (
        "rules/semgrep/kotlin/thread-sleep-in-suspend.yml",
        include_str!("../assets/rules/semgrep/kotlin/thread-sleep-in-suspend.yml"),
    ),
    (
        "rules/semgrep/thymeleaf/unescaped-output.yml",
        include_str!("../assets/rules/semgrep/thymeleaf/unescaped-output.yml"),
    ),
    (
        "rules/semgrep/java/stdout-usage.yml",
        include_str!("../assets/rules/semgrep/java/stdout-usage.yml"),
    ),
    (
        "rules/semgrep/java/print-stack-trace.yml",
        include_str!("../assets/rules/semgrep/java/print-stack-trace.yml"),
    ),
    (
        "rules/semgrep/kotlin/print-stack-trace.yml",
        include_str!("../assets/rules/semgrep/kotlin/print-stack-trace.yml"),
    ),
    (
        "rules/semgrep/kotlin/sql-injection.yml",
        include_str!("../assets/rules/semgrep/kotlin/sql-injection.yml"),
    ),
    (
        "rules/semgrep/kotlin/transactional-suspend.yml",
        include_str!("../assets/rules/semgrep/kotlin/transactional-suspend.yml"),
    ),
    (
        "rules/pmd/java/empty-catch.xml",
        include_str!("../assets/rules/pmd/java/empty-catch.xml"),
    ),
    (
        "rules/semgrep/kotlin/stdout-usage.yml",
        include_str!("../assets/rules/semgrep/kotlin/stdout-usage.yml"),
    ),
    (
        "rules/semgrep/security/cors-wildcard.yml",
        include_str!("../assets/rules/semgrep/security/cors-wildcard.yml"),
    ),
    (
        "rules/semgrep/security/sql-injection.yml",
        include_str!("../assets/rules/semgrep/security/sql-injection.yml"),
    ),
    (
        "rules/semgrep/security/weak-crypto.yml",
        include_str!("../assets/rules/semgrep/security/weak-crypto.yml"),
    ),
    (
        "rules/semgrep/security/disabled-tls-verification.yml",
        include_str!("../assets/rules/semgrep/security/disabled-tls-verification.yml"),
    ),
    (
        "rules/semgrep/security/csrf-disabled.yml",
        include_str!("../assets/rules/semgrep/security/csrf-disabled.yml"),
    ),
    (
        "rules/semgrep/thymeleaf/restricted-bean-access.yml",
        include_str!("../assets/rules/semgrep/thymeleaf/restricted-bean-access.yml"),
    ),
    (
        "rules/semgrep/spring/field-injection.yml",
        include_str!("../assets/rules/semgrep/spring/field-injection.yml"),
    ),
    (
        "rules/semgrep/java/ignored-interrupt.yml",
        include_str!("../assets/rules/semgrep/java/ignored-interrupt.yml"),
    ),
    (
        "rules/semgrep/kotlin/run-blocking-in-suspend.yml",
        include_str!("../assets/rules/semgrep/kotlin/run-blocking-in-suspend.yml"),
    ),
    (
        "rules/semgrep/kotlin/global-scope.yml",
        include_str!("../assets/rules/semgrep/kotlin/global-scope.yml"),
    ),
    (
        "rules/semgrep/spring/private-transactional.yml",
        include_str!("../assets/rules/semgrep/spring/private-transactional.yml"),
    ),
    (
        "rules/pmd/java/unnecessary-import.xml",
        include_str!("../assets/rules/pmd/java/unnecessary-import.xml"),
    ),
    (
        "rules/semgrep/spring/shared-simple-date-format.yml",
        include_str!("../assets/rules/semgrep/spring/shared-simple-date-format.yml"),
    ),
    (
        "rules/pmd/java/preserve-stack-trace.xml",
        include_str!("../assets/rules/pmd/java/preserve-stack-trace.xml"),
    ),
    (
        "rules/pmd/java/resource-not-closed.xml",
        include_str!("../assets/rules/pmd/java/resource-not-closed.xml"),
    ),
    (
        "rules/semgrep/kotlin/swallowed-cancellation.yml",
        include_str!("../assets/rules/semgrep/kotlin/swallowed-cancellation.yml"),
    ),
    (
        "rules/semgrep/spring/singleton-request-state.yml",
        include_str!("../assets/rules/semgrep/spring/singleton-request-state.yml"),
    ),
    (
        "rules/semgrep/spring/checked-exception-rollback.yml",
        include_str!("../assets/rules/semgrep/spring/checked-exception-rollback.yml"),
    ),
    (
        "rules/semgrep/java/fully-qualified-type.yml",
        include_str!("../assets/rules/semgrep/java/fully-qualified-type.yml"),
    ),
    (
        "rules/semgrep/kotlin/fully-qualified-type.yml",
        include_str!("../assets/rules/semgrep/kotlin/fully-qualified-type.yml"),
    ),
    (
        "rules/semgrep/html/inline-script.yml",
        include_str!("../assets/rules/semgrep/html/inline-script.yml"),
    ),
    (
        "rules/semgrep/html/inline-event-handler.yml",
        include_str!("../assets/rules/semgrep/html/inline-event-handler.yml"),
    ),
];

pub struct Registry {
    profiles: BTreeMap<String, Profile>,
    rules: BTreeMap<String, Rule>,
    engine_configs: BTreeMap<String, String>,
}

impl Registry {
    pub fn builtin() -> Result<Self> {
        let mut registry = Self {
            profiles: BTreeMap::new(),
            rules: BTreeMap::new(),
            engine_configs: BUILTIN_ENGINE_CONFIGS
                .iter()
                .map(|&(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
        };
        for source in BUILTIN_PROFILES {
            let profile: Profile = serde_saphyr::from_str(source)
                .into_diagnostic()
                .wrap_err("Invalid built-in profile")?;
            registry.insert_profile(profile)?;
        }

        for source in BUILTIN_RULES {
            let rule: Rule = serde_saphyr::from_str(source)
                .into_diagnostic()
                .wrap_err("Invalid built-in rule")?;
            registry.insert_rule(rule)?;
        }
        Ok(registry)
    }

    pub fn for_config(path: &Path, config: &ProjectConfig) -> Result<Self> {
        let mut registry = Self::builtin()?;
        if let Some(source) = Source::open(path, &config.ruleset)? {
            registry.extend(source)?;
        }
        for rule in config
            .ignore
            .iter()
            .filter_map(|entry| entry.rule.as_deref())
        {
            if !registry
                .rules
                .keys()
                .any(|id| ignore_rule_matches(rule, id))
            {
                bail!("Invalid ignore rule: Unknown rule or rule family {rule}");
            }
        }
        Ok(registry)
    }

    fn extend(&mut self, source: Source) -> Result<()> {
        let manifest = source.manifest()?;
        for path in manifest.rules {
            let rule: Rule = source.parse(&path)?;
            match rule.engine {
                EngineName::Semgrep => {
                    let implementation = rule.implementation()?;
                    let path = &implementation.config;
                    if self.engine_configs.contains_key(path) {
                        bail!("Duplicate engine configuration path: {path}");
                    }
                    let contents = source.read_file(path)?;
                    self.engine_configs.insert(path.clone(), contents);
                }
                EngineName::Archunit => {}
                EngineName::Pmd | EngineName::Eslint | EngineName::Builtin | EngineName::Gradle => {
                    bail!("Custom rules support Semgrep patterns and ArchUnit package checks");
                }
            }
            self.insert_rule(rule)?;
        }
        for path in manifest.profiles {
            self.insert_profile(source.parse(&path)?)?;
        }
        for profile in self.profiles.values() {
            self.selected_profiles([&profile.name])?;
            for id in &profile.rules {
                self.rule(id)?;
            }
            self.validate_overrides(&profile.rule_defaults)?;
        }
        Ok(())
    }

    fn insert_profile(&mut self, profile: Profile) -> Result<()> {
        let name = profile.name.clone();
        if name.is_empty() || self.profiles.insert(name.clone(), profile).is_some() {
            bail!("Empty or duplicate profile name: {name}");
        }
        Ok(())
    }

    fn insert_rule(&mut self, mut rule: Rule) -> Result<()> {
        let id = rule.id.clone();
        match rule.engine {
            EngineName::Semgrep | EngineName::Pmd | EngineName::Eslint => {
                let implementation = rule.implementation()?;
                if self.rules.values().any(|other| {
                    other.engine == rule.engine
                        && other
                            .implementation
                            .as_ref()
                            .is_some_and(|other| other.rule_id == implementation.rule_id)
                }) {
                    bail!("Duplicate engine rule ID: {}", implementation.rule_id);
                }
                let contents = self.engine_configs.get(&implementation.config);
                if rule.engine != EngineName::Eslint && contents.is_none() {
                    bail!("Rule {id} is missing its engine configuration");
                }
                if let (EngineName::Semgrep, Some(contents)) = (rule.engine, contents) {
                    let path = &implementation.config;
                    let semgrep: SemgrepRules = serde_saphyr::from_str(contents)
                        .into_diagnostic()
                        .wrap_err_with(|| format!("Invalid Semgrep configuration {path}"))?;
                    let [configured] = semgrep.rules.as_slice() else {
                        bail!(
                            "Semgrep configuration {path} must contain exactly the declared engine rule"
                        );
                    };
                    if configured.id != implementation.rule_id || configured.languages.is_empty() {
                        bail!(
                            "Semgrep configuration {path} must declare the engine rule ID and its languages"
                        );
                    }
                    rule.semgrep_languages = configured.languages.clone();
                }
            }
            EngineName::Archunit
                if rule
                    .options
                    .as_ref()
                    .is_none_or(|options| !options.is_valid()) =>
            {
                bail!(
                    "Rule {id} requires architecture options with either forbidden packages or cycles: true"
                );
            }
            EngineName::Archunit | EngineName::Builtin | EngineName::Gradle => {}
        }
        if id.is_empty() || self.rules.insert(id.clone(), rule).is_some() {
            bail!("Empty or duplicate rule ID: {id}");
        }
        Ok(())
    }

    pub fn profiles(&self) -> impl Iterator<Item = &Profile> {
        self.profiles.values()
    }

    pub fn rules(&self) -> impl Iterator<Item = &Rule> {
        self.rules.values()
    }

    pub fn profile(&self, name: &str) -> Result<&Profile> {
        self.profiles
            .get(name)
            .ok_or_else(|| miette!("Unknown profile: {name}"))
    }

    pub fn rule(&self, id: &str) -> Result<&Rule> {
        self.rules
            .get(id)
            .ok_or_else(|| miette!("Unknown rule: {id}"))
    }

    pub fn engine_config(&self, rule: &Rule) -> Result<&str> {
        self.engine_configs
            .get(rule.implementation()?.config.as_str())
            .map(String::as_str)
            .ok_or_else(|| miette!("Missing engine configuration for {}", rule.id))
    }

    pub fn load(path: &Path) -> Result<ProjectConfig> {
        let source = fs::read_to_string(path)
            .into_diagnostic()
            .wrap_err_with(|| format!("Could not read {}", path.display()))?;
        Self::parse(&source)
            .wrap_err_with(|| format!("Invalid configuration in {}", path.display()))
    }

    pub fn parse(source: &str) -> Result<ProjectConfig> {
        let config: ProjectConfig = serde_saphyr::from_str(source).into_diagnostic()?;
        if config.version != 1 {
            bail!(
                "Unsupported configuration version {}; expected 1",
                config.version
            );
        }
        config.ruleset.validate()?;
        if config.java_target == Some(0) {
            bail!("java_target must be a positive Java feature version");
        }
        if let Some(version) = config.java_source
            && !dornguard_core::PMD_JAVA_VERSIONS.contains(&version)
        {
            bail!("java_source must be a Java feature version from 3 to 27");
        }
        for (name, width) in [
            ("java_indent", config.format.java_indent),
            ("kotlin_indent", config.format.kotlin_indent),
            ("javascript_indent", config.format.javascript_indent),
        ] {
            if !(1..=16).contains(&width) {
                bail!("format.{name} must be between 1 and 16");
            }
        }
        for entry in &config.format.overrides {
            if entry.files.is_empty() {
                bail!("format.overrides.files must contain at least one glob");
            }
            if entry
                .indent_size
                .is_some_and(|width| !(1..=16).contains(&width))
            {
                bail!("format.overrides.indent_size must be between 1 and 16");
            }
        }
        for signature in &config.java_resource_owners {
            if !resource_signature(signature).is_some_and(|(_, parameters)| {
                qualified_identifier(parameters.trim_end_matches("[]"))
            }) {
                bail!(
                    "java_resource_owners requires a fully qualified signature with one resource argument, e.g. example.Handle#new(java.sql.Connection)"
                );
            }
        }
        for signature in &config.java_managed_resource_factories {
            if !resource_signature(signature).is_some_and(|(method, parameters)| {
                method != "new"
                    && (parameters.is_empty()
                        || parameters.split(',').all(|parameter| {
                            qualified_identifier(parameter.trim_end_matches("[]"))
                        }))
            }) {
                bail!(
                    "java_managed_resource_factories requires fully qualified method signatures, e.g. example.AppResources#connection()"
                );
            }
        }
        for sanitizer in &config.thymeleaf_sanitizers {
            if !sanitizer
                .strip_prefix('@')
                .and_then(|callee| callee.split_once('.'))
                .is_some_and(|(bean, method)| identifier(bean) && identifier(method))
            {
                bail!(
                    "thymeleaf_sanitizers requires bean method names such as @htmlSanitizer.clean"
                );
            }
        }
        Ok(config)
    }

    pub fn resolve(
        &self,
        config: &ProjectConfig,
        module: Option<&str>,
        extra_profiles: &[String],
    ) -> Result<Vec<Rule>> {
        let module = module.and_then(|module| config.modules.get(module));
        let profiles = self.selected_profiles(
            config
                .profiles
                .iter()
                .chain(module.into_iter().flat_map(|module| &module.profiles))
                .chain(extra_profiles),
        )?;
        let explicit = module
            .into_iter()
            .map(|module| &module.rules)
            .chain([&config.rules])
            .collect::<Vec<_>>();
        for overrides in &explicit {
            self.validate_overrides(overrides)?;
        }
        let ids = profiles
            .iter()
            .flat_map(|profile| &profile.rules)
            .collect::<BTreeSet<_>>();
        let mut active = Vec::new();
        for id in ids {
            let mut rule = self.rule(id)?.clone();
            let explicit = explicit
                .iter()
                .filter_map(|overrides| overrides.get(id))
                .collect::<Vec<_>>();
            let defaults = profiles
                .iter()
                .filter_map(|profile| {
                    profile
                        .rule_defaults
                        .get(id)
                        .map(|value| (profile.name.as_str(), value))
                })
                .collect::<Vec<_>>();
            let enabled = effective(id, "enabled", &explicit, &defaults, |item| {
                item.enabled.as_ref()
            })?
            .unwrap_or(&true);
            let skip_tests = effective(id, "skip_java_tests", &explicit, &defaults, |item| {
                item.skip_java_tests.as_ref()
            })?;
            let severity = effective(id, "severity", &explicit, &defaults, |item| {
                item.severity.as_ref()
            })?;
            let options = effective(id, "options", &explicit, &defaults, |item| {
                item.options.as_ref()
            })?;
            if *enabled {
                if let Some(skip_tests) = skip_tests {
                    rule.skip_java_tests = *skip_tests;
                }
                if let Some(severity) = severity {
                    rule.severity = *severity;
                }
                if let Some(options) = options {
                    rule.options = Some(options.clone());
                }
                active.push(rule);
            }
        }
        Ok(active)
    }

    pub fn kotlin_profile_enabled(
        &self,
        config: &ProjectConfig,
        module: Option<&str>,
        extra_profiles: &[String],
    ) -> Result<bool> {
        let module = module.and_then(|name| config.modules.get(name));
        let profiles = config
            .profiles
            .iter()
            .chain(module.into_iter().flat_map(|module| &module.profiles))
            .chain(extra_profiles);
        Ok(self
            .selected_profiles(profiles)?
            .iter()
            .any(|profile| profile.name == "kotlin"))
    }

    fn validate_overrides(&self, overrides: &BTreeMap<String, RuleOverride>) -> Result<()> {
        for (id, value) in overrides {
            let rule = self.rule(id)?;
            if let Some(options) = &value.options
                && (rule.engine != EngineName::Archunit || !options.is_valid())
            {
                bail!("Rule {id} does not accept these options");
            }
            if rule.engine == EngineName::Gradle {
                bail!("Configure Gradle dependency rules through module_dependencies");
            }
        }
        Ok(())
    }

    fn selected_profiles<'a>(
        &self,
        profiles: impl IntoIterator<Item = &'a String>,
    ) -> Result<Vec<&Profile>> {
        let mut done = BTreeSet::new();
        let mut stack = Vec::new();
        let mut selected = Vec::new();
        for name in profiles {
            self.visit(name, &mut stack, &mut done, &mut selected)?;
        }
        Ok(selected)
    }

    fn visit<'a>(
        &'a self,
        name: &str,
        stack: &mut Vec<&'a str>,
        done: &mut BTreeSet<&'a str>,
        selected: &mut Vec<&'a Profile>,
    ) -> Result<()> {
        if done.contains(name) {
            return Ok(());
        }
        let profile = self.profile(name)?;
        if stack.contains(&profile.name.as_str()) {
            stack.push(&profile.name);
            bail!("Profile inheritance cycle: {}", stack.join(" -> "));
        }
        stack.push(&profile.name);
        for parent in &profile.extends {
            self.visit(parent, stack, done, selected)?;
        }
        selected.push(profile);
        stack.pop();
        done.insert(&profile.name);
        Ok(())
    }
}

fn effective<'a, T: PartialEq + 'a>(
    id: &str,
    option: &str,
    explicit: &[&'a RuleOverride],
    defaults: &[(&str, &'a RuleOverride)],
    field: impl Fn(&'a RuleOverride) -> Option<&'a T>,
) -> Result<Option<&'a T>> {
    if let Some(value) = explicit.iter().find_map(|value| field(value)) {
        return Ok(Some(value));
    }
    let mut values = defaults
        .iter()
        .filter_map(|(name, value)| field(value).map(|value| (*name, value)));
    let first = values.next();
    if let Some((name, value)) = first
        && let Some((other, _)) = values.find(|(_, other)| other != &value)
    {
        bail!("Rule {id} has conflicting {option} defaults in profiles {name} and {other}");
    }
    Ok(first.map(|(_, value)| value))
}

#[derive(Deserialize)]
struct SemgrepRules {
    rules: Vec<SemgrepRuleId>,
}

#[derive(Deserialize)]
struct SemgrepRuleId {
    id: String,
    languages: Vec<String>,
}

impl ProjectConfig {
    pub fn validate_trusted_ruleset(&self, policy: &Path, worktree: &Path) -> Result<()> {
        self.ruleset.validate_trust(policy, worktree)
    }

    pub fn local_ruleset_path(&self) -> Option<&Path> {
        if self.ruleset.source == PolicySource::Local {
            self.ruleset.path.as_deref()
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_indentation_is_off_by_default_and_tests_are_skipped() {
        let registry = Registry::builtin().unwrap();
        let config = Registry::parse("version: 1\n").unwrap();
        let rules = registry
            .resolve(&config, None, &["format".to_owned()])
            .unwrap();
        assert!(rules.iter().all(|rule| rule.id != "format.indentation"));
        assert!(rules.iter().all(|rule| rule.skips_tests()), "{rules:?}");

        let config = Registry::parse(
            "version: 1\nrules:\n  format.indentation: {enabled: true}\n  format.final-newline: {skip_java_tests: false}\n",
        )
        .unwrap();
        let rules = registry
            .resolve(&config, None, &["format".to_owned()])
            .unwrap();
        let skips = |id: &str| {
            rules
                .iter()
                .find(|rule| rule.id == id)
                .unwrap()
                .skips_tests()
        };
        assert!(skips("format.indentation"));
        assert!(!skips("format.final-newline"));
    }

    #[test]
    fn partial_parsing_files_tolerate_only_listed_paths() {
        let config = Registry::parse(
            "version: 1\npartial_parsing_files:\n  - path: src/Odd.java\n    reason: Parser rejects valid syntax\n",
        )
        .unwrap();
        let policy = config.partial_parsing_policy();
        assert!(policy.tolerates(Path::new("src/Odd.java")));
        assert!(!policy.tolerates(Path::new("src/Other.java")));
    }

    #[test]
    fn partial_parsing_warning_tolerates_every_file() {
        let config = Registry::parse("version: 1\npartial_parsing: warning\n").unwrap();
        assert!(
            config
                .partial_parsing_policy()
                .tolerates(Path::new("src/Any.java"))
        );
        let strict = Registry::parse("version: 1\n").unwrap();
        assert!(
            !strict
                .partial_parsing_policy()
                .tolerates(Path::new("src/Any.java"))
        );
    }

    #[test]
    fn partial_parsing_files_require_a_reason() {
        let error = Registry::parse(
            "version: 1\npartial_parsing_files:\n  - path: src/Odd.java\n    reason: ' '\n",
        )
        .err()
        .expect("blank reason must be rejected");
        assert!(
            format!("{error:?}").contains("requires a reason"),
            "{error:?}"
        );
    }

    /// Rules whose findings need a judgment. Adding to this list is a decision:
    /// a rule belongs here when it infers intent from code shape.
    const MEDIUM_CONFIDENCE: [&str; 24] = [
        "java.assert-throws-nested-call",
        "java.broken-null-check",
        "java.fully-qualified-type",
        "java.ignored-interrupt",
        "java.ignored-return-value",
        "java.lock-without-finally",
        "java.preserve-stack-trace",
        "java.resource-not-closed",
        "jsp.unescaped-output",
        "kotlin.fully-qualified-type",
        "kotlin.run-catching-cancellation",
        "kotlin.sql-injection",
        "kotlin.swallowed-cancellation",
        "security.cors-wildcard",
        "security.csrf-disabled",
        "security.disabled-tls-verification",
        "security.jackson-default-typing",
        "security.sql-injection",
        "security.weak-crypto",
        "spring.checked-exception-rollback",
        "spring.jpa-entity-response",
        "spring.shared-simple-date-format",
        "spring.singleton-request-state",
        "thymeleaf.unescaped-output",
    ];

    #[test]
    fn builtin_rules_declare_medium_confidence_only_for_the_reviewed_set() {
        let registry = Registry::builtin().unwrap();
        let medium = registry
            .rules()
            .filter(|rule| rule.confidence == Confidence::Medium)
            .map(|rule| rule.id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(medium, MEDIUM_CONFIDENCE);
    }

    #[test]
    fn rules_default_to_high_confidence() {
        let rule: Rule = serde_saphyr::from_str(
            "id: custom.rule\nengine: semgrep\nseverity: error\ndescription: d\nfix: f\nimplementation:\n  config: c.yml\n  rule_id: r\n",
        )
        .unwrap();
        assert_eq!(rule.confidence, Confidence::High);
    }

    #[test]
    fn profile_inheritance_deduplicates_rules_and_reports_cycles() {
        let mut registry = Registry::builtin().unwrap();
        let java_config: ProjectConfig =
            serde_saphyr::from_str("version: 1\nprofiles: [java]\n").unwrap();
        let java_rules = registry.resolve(&java_config, None, &[]).unwrap();
        registry.profiles.insert(
            "extra".into(),
            Profile {
                name: "extra".into(),
                description: String::new(),
                extends: vec!["java".into()],
                rules: vec!["java.stdout-usage".into()],
                rule_defaults: BTreeMap::new(),
            },
        );
        let selected = ["extra".into(), "java".into()];
        let config: ProjectConfig =
            serde_saphyr::from_str("version: 1\nprofiles: [extra, java]\n").unwrap();
        let resolved = registry.resolve(&config, None, &[]).unwrap();
        let ids = |rules: &[Rule]| rules.iter().map(|rule| rule.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&resolved), ids(&java_rules));
        registry
            .profiles
            .get_mut("java")
            .unwrap()
            .extends
            .push("extra".into());
        assert!(
            registry
                .selected_profiles(&selected)
                .unwrap_err()
                .to_string()
                .contains("extra -> java -> extra")
        );
    }

    #[test]
    fn explicit_project_and_module_fields_resolve_profile_conflicts() {
        let mut registry = Registry::builtin().unwrap();
        for (name, severity) in [("first", "warning"), ("second", "error")] {
            let profile = serde_saphyr::from_str(&format!(
                "name: {name}\ndescription: test\nrules: [java.stdout-usage]\nrule_defaults:\n  java.stdout-usage: {{severity: {severity}}}\n"
            )).unwrap();
            registry.insert_profile(profile).unwrap();
        }
        let conflicting =
            serde_saphyr::from_str("version: 1\nprofiles: [first, second]\n").unwrap();
        let error = registry.resolve(&conflicting, None, &[]).unwrap_err();
        assert!(error.to_string().contains("profiles first and second"));

        let project = serde_saphyr::from_str(
            r#"
version: 1
profiles: [first, second]
rules:
  java.stdout-usage: { severity: error }
modules:
  ":api":
    rules:
      java.stdout-usage: { severity: warning }
  ":admin":
    rules:
      java.stdout-usage: { enabled: false }
"#,
        )
        .unwrap();
        assert_eq!(
            registry.resolve(&project, None, &[]).unwrap()[0].severity,
            Severity::Error
        );
        assert_eq!(
            registry.resolve(&project, Some(":api"), &[]).unwrap()[0].severity,
            Severity::Warning
        );
        assert!(
            registry
                .resolve(&project, Some(":admin"), &[])
                .unwrap()
                .is_empty()
        );
    }
}
