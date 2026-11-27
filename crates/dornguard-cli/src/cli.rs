use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "dornguard",
    version,
    about = "Run your project's code quality policy",
    after_help = "\
Coding agent workflow

Run from the project root to use its .dornguard.yml.
For configuration examples, see dornguard init --help. Use dornguard profiles list
to find profiles and dornguard profiles show NAME to see what a profile includes.

  dornguard check --changed --format agent
      Review staged, unstaged and untracked changes. Shows counts and up to 20 findings.
  dornguard check --changed --base origin/main --format agent
      Review a PR, including committed branch changes. Use the PR's actual base ref.
  dornguard check --format json
      Scan the full project and return every finding. Keep the original scope flags when
      switching a previous check to JSON, for example check --changed --format json.
  dornguard check --only-profile format --format agent
      Run just the formatting profile with the project's settings.
  dornguard rules explain RULE_ID
      Look up a finding's rule, suggested fix and available settings.
Use --report-rule RULE_ID or --report-path PATH to focus the report while keeping
the original check scope and exit code.

Read the result before acting on findings. In JSON, check complete, scope and rule_counts.
If complete is false, inspect failures. If a changed check reports full scope, read
fallback_reason. Build-backed Maven checks scan the full reactor.

Exit codes
  0  No blocking violations (warnings may remain), or no changes
  1  Blocking policy violations
  2  Invalid configuration
  3  Incomplete engine analysis
  4  Check preparation failed

Use dornguard check --help for option details and dornguard doctor to inspect engine setup.
The audit and csp commands also accept --format json."
)]
pub(crate) struct Cli {
    #[command(subcommand)]
    pub(crate) command: Action,
}

#[derive(Subcommand)]
pub(crate) enum Action {
    /// Check project sources and build-backed policies
    #[command(after_help = "\
Examples
  dornguard check --changed --format agent
  dornguard check --changed --base origin/main --format json
  dornguard check --only-profile format --format agent
  dornguard check --resolve-types --format agent

Keep the same scope and profile options when switching between agent and JSON output.
With --changed, build-backed Maven checks report full scope.")]
    Check(CheckArgs),
    /// Audit a Maven project's resolved runtime dependencies for known vulnerabilities
    Audit {
        #[arg(default_value = ".")]
        project: PathBuf,
        /// Output format for the dependency audit report
        #[arg(long, value_enum, default_value_t = TextOrJson::Text)]
        format: TextOrJson,
        /// Activate Maven profiles for dependency resolution
        #[arg(long)]
        maven_profile: Vec<String>,
    },
    /// Inspect the enforced CSP and inline scripts of an HTML response
    Csp {
        url: String,
        /// Output format for the CSP report
        #[arg(long, value_enum, default_value_t = TextOrJson::Text)]
        format: TextOrJson,
    },
    /// Create .dornguard.yml and show configuration examples in help
    #[command(
        after_help = r#"Run dornguard init from the project root to create .dornguard.yml with the java profile.
Edit the existing file if the project already has one.

Example .dornguard.yml for Spring Boot with formatting checks

version: 1
profiles: [spring-boot, format]
rules:
  java.stdout-usage: { severity: warning }
format:
  exclude: ["src/main/resources/static/vendor/**"]
  overrides:
    - files: ["src/main/resources/static/legacy/**/*.js"]
      indent_size: 4
ignore:
  - path: docs/scripts/
    rule: java.stdout-usage
    reason: "These scripts write results to standard output."

Choose profiles with dornguard profiles list and inspect them with dornguard profiles show NAME.
Use rules to change severity or disable an active rule with enabled: false.
Use forward slashes in paths relative to the directory containing .dornguard.yml.
Use exclude to skip source analysis. Use ignore to suppress findings in a file, directory
or glob. Use rule: "format.*" to select the formatting rule family. Every ignore entry
requires a reason.
For the thymeleaf profile, set thymeleaf_partial_parsing: warning to tolerate partial
template parsing while reporting incomplete coverage.
For valid code that the parser rejects, list the file in partial_parsing_files with a reason,
or set partial_parsing: warning for every file.

Run dornguard check --format agent to verify how the configuration applies."#
    )]
    Init,
    /// Print the installed CLI version
    Version,
    /// Verify the engines needed by the project's active profiles
    Doctor {
        /// Add a profile to the configured project profiles
        #[arg(long)]
        profile: Vec<String>,
        /// Read a specific .dornguard.yml
        #[arg(long, value_name = "PATH")]
        config: Option<PathBuf>,
    },
    /// Browse supported engines
    Engines {
        #[command(subcommand)]
        command: EngineAction,
    },
    /// Record existing violations
    Baseline {
        #[command(subcommand)]
        command: BaselineAction,
    },
    /// Browse built-in profiles
    Profiles {
        #[command(subcommand)]
        command: ProfileAction,
    },
    /// Browse built-in rules
    Rules {
        #[command(subcommand)]
        command: RuleAction,
    },
}

#[derive(Args)]
pub(crate) struct CheckArgs {
    #[arg(default_value = ".")]
    pub(crate) target: PathBuf,
    /// Add a profile to the project and module profiles
    #[arg(long, conflicts_with = "only_profile")]
    pub(crate) profile: Vec<String>,
    /// Use these profiles in place of the configured project and module profiles
    #[arg(long, value_name = "PROFILE")]
    pub(crate) only_profile: Vec<String>,
    /// Report format. Use agent for a compact review or json for every finding and failure
    #[arg(long, value_enum, default_value_t = Format::Text)]
    pub(crate) format: Format,
    /// Show findings for this rule without changing the check or exit status. Repeat to include more rules
    #[arg(long, value_name = "RULE_ID")]
    pub(crate) report_rule: Vec<String>,
    /// Show findings under this project-relative file or directory. Repeat to include more paths
    #[arg(long, value_name = "PATH", value_parser = report_path)]
    pub(crate) report_path: Vec<PathBuf>,
    /// Use a specific configuration file
    #[arg(long, value_name = "PATH")]
    pub(crate) config: Option<PathBuf>,
    /// Check one Gradle module or Maven reactor module
    #[arg(long, value_name = "MODULE")]
    pub(crate) module: Option<String>,
    /// Compile main Java sources for PMD type resolution, including wildcard imports
    #[arg(long, visible_alias = "resolve-types")]
    pub(crate) resolve_imports: bool,
    /// Check staged, unstaged and untracked changes
    #[arg(long, conflicts_with = "module")]
    pub(crate) changed: bool,
    /// Compare committed changes from the merge base of HEAD and REF
    #[arg(long, value_name = "REF", requires = "changed")]
    pub(crate) base: Option<String>,
    /// Suppress existing violations recorded in a baseline
    #[arg(long, value_name = "PATH")]
    pub(crate) baseline: Option<PathBuf>,
    /// Report baselined findings in files selected by --changed
    #[arg(long, requires = "baseline", requires = "changed")]
    pub(crate) strict_changed: bool,
    /// Fail a complete full scan when the baseline records findings that no longer occur
    #[arg(long, requires = "baseline")]
    pub(crate) ratchet: bool,
    /// Enforce a policy supplied outside the project worktree
    #[arg(long, value_name = "PATH")]
    pub(crate) organization_policy: Option<PathBuf>,
}

#[derive(Subcommand)]
pub(crate) enum ProfileAction {
    /// List the available profiles and their descriptions
    List,
    /// Show a profile's parent profiles and directly enabled rules
    Show { name: String },
}

#[derive(Subcommand)]
pub(crate) enum RuleAction {
    List,
    Explain { id: String },
}

#[derive(Subcommand)]
pub(crate) enum EngineAction {
    List,
    /// Install verified versions of Semgrep and PMD into DornGuard's data directory
    Install {
        engine: Option<InstallTarget>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum InstallTarget {
    Semgrep,
    Pmd,
    Eslint,
}

#[derive(Subcommand)]
pub(crate) enum BaselineAction {
    Create {
        #[arg(long, default_value = ".dornguard-baseline.json")]
        output: PathBuf,
        /// Add a profile to the project and module profiles
        #[arg(long, conflicts_with = "only_profile")]
        profile: Vec<String>,
        /// Use these profiles in place of the configured project and module profiles
        #[arg(long, value_name = "PROFILE")]
        only_profile: Vec<String>,
        /// Record violations using the compiled main Java classpath
        #[arg(long, visible_alias = "resolve-types")]
        resolve_imports: bool,
        /// Replace an existing baseline
        #[arg(long)]
        force: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum Format {
    /// Human-readable report
    Text,
    /// Complete machine-readable report
    Json,
    /// Compact text summary with up to 20 diagnostics
    Agent,
    /// SARIF 2.1.0 report for code scanning tools
    Sarif,
}

#[derive(Clone, Copy, ValueEnum)]
pub(crate) enum TextOrJson {
    /// Human-readable report
    Text,
    /// Machine-readable JSON report
    Json,
}

pub(crate) fn report_path(value: &str) -> std::result::Result<PathBuf, &'static str> {
    let path = PathBuf::from(value);
    if path.as_os_str().is_empty()
        || path.components().any(|part| {
            !matches!(
                part,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
    {
        return Err("report paths must be inside the project");
    }
    Ok(path)
}
