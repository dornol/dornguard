use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use dornguard_config::{
    BUILTIN_VERSION, CONFIG_FILE, DEFAULT_CONFIG, FLYWAY_RULE, JPA_RESPONSE_RULE, PartialParsing,
    ProjectConfig, Registry, Rule,
};
use dornguard_core::{EngineName, Failure, FailureKind, Outcome, Scope, SourceLanguage};
use dornguard_engine::{
    Engine, Eslint, Pmd, Request, Semgrep, check_builtin, inspect, inspect_archunit, inspect_java,
    install, maven_model_path, supported_version,
};
use ignore::WalkBuilder;
use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};

mod audit;
mod baseline;
mod build_check;
mod changed;
mod csp;
mod flyway;
mod maven;
mod suppressions;

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
struct Cli {
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Check project sources and build-backed policies
    #[command(after_help = "\
Examples
  dornguard check --changed --format agent
  dornguard check --changed --base origin/main --format json
  dornguard check --only-profile format --format agent
  dornguard check --resolve-types --format agent

Keep the same scope and profile options when switching between agent and JSON output.
With --changed, build-backed Maven checks report full scope.")]
    Check {
        #[arg(default_value = ".")]
        target: PathBuf,
        /// Add a profile to the project and module profiles
        #[arg(long, conflicts_with = "only_profile")]
        profile: Vec<String>,
        /// Use these profiles in place of the configured project and module profiles
        #[arg(long, value_name = "PROFILE")]
        only_profile: Vec<String>,
        /// Report format. Use agent for a compact review or json for every finding and failure
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
        /// Show findings for this rule without changing the check or exit status. Repeat to include more rules
        #[arg(long, value_name = "RULE_ID")]
        report_rule: Vec<String>,
        /// Show findings under this project-relative file or directory. Repeat to include more paths
        #[arg(long, value_name = "PATH", value_parser = report_path)]
        report_path: Vec<PathBuf>,
        /// Use a specific configuration file
        #[arg(long, value_name = "PATH")]
        config: Option<PathBuf>,
        /// Check one Gradle module or Maven reactor module
        #[arg(long, value_name = "MODULE")]
        module: Option<String>,
        /// Compile main Java sources for PMD type resolution, including wildcard imports
        #[arg(long, visible_alias = "resolve-types")]
        resolve_imports: bool,
        /// Check staged, unstaged and untracked changes
        #[arg(long, conflicts_with = "module")]
        changed: bool,
        /// Compare committed changes from the merge base of HEAD and REF
        #[arg(long, value_name = "REF", requires = "changed")]
        base: Option<String>,
        /// Suppress existing violations recorded in a baseline
        #[arg(long, value_name = "PATH")]
        baseline: Option<PathBuf>,
        /// Enforce a policy supplied outside the project worktree
        #[arg(long, value_name = "PATH")]
        organization_policy: Option<PathBuf>,
    },
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

#[derive(Subcommand)]
enum ProfileAction {
    /// List the available profiles and their descriptions
    List,
    /// Show a profile's parent profiles and directly enabled rules
    Show { name: String },
}

#[derive(Subcommand)]
enum RuleAction {
    List,
    Explain { id: String },
}

#[derive(Subcommand)]
enum EngineAction {
    List,
    /// Install verified versions of Semgrep and PMD into DornGuard's data directory
    Install {
        engine: Option<InstallTarget>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum InstallTarget {
    Semgrep,
    Pmd,
}

#[derive(Subcommand)]
enum BaselineAction {
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
enum Format {
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
enum TextOrJson {
    /// Human-readable report
    Text,
    /// Machine-readable JSON report
    Json,
}

fn report_path(value: &str) -> std::result::Result<PathBuf, &'static str> {
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

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let _ = tracing_subscriber::fmt()
        .with_writer(io::stderr)
        .with_max_level(tracing::Level::WARN)
        .try_init();
    let cli = Cli::parse();
    match dispatch(cli.command).await {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("{error:#}");
            ExitCode::from(2)
        }
    }
}

async fn dispatch(action: Action) -> Result<u8> {
    match action {
        Action::Check {
            target,
            profile,
            only_profile,
            format,
            report_rule,
            report_path,
            config,
            module,
            resolve_imports,
            changed,
            base,
            baseline,
            organization_policy,
        } => {
            let mut outcome = async {
                let loaded_baseline = match baseline
                    .as_deref()
                    .map(baseline::Baseline::load)
                    .transpose()
                {
                    Ok(baseline) => baseline,
                    Err(error) => return failure(FailureKind::ConfigurationError, error),
                };
                let mut outcome = check(
                    &target,
                    ProfileSelection::from_cli(&profile, &only_profile),
                    config.as_deref(),
                    module.as_deref(),
                    changed.then_some(changed::Request {
                        base: base.as_deref(),
                        baseline: baseline.as_deref(),
                    }),
                    None,
                    resolve_imports,
                )
                .await;
                if let Some(baseline) = loaded_baseline {
                    baseline.apply(&mut outcome);
                }
                outcome
            }
            .await;
            if let Some(policy) = &organization_policy {
                let mut required = check(
                    &target,
                    ProfileSelection::default(),
                    config.as_deref(),
                    None,
                    changed.then_some(changed::Request {
                        base: base.as_deref(),
                        baseline: None,
                    }),
                    Some(policy),
                    resolve_imports,
                )
                .await;
                outcome.organization_policy_enforced = required.failures.is_empty();
                outcome.project_scope = Some(outcome.scope);
                outcome.organization_scope = Some(required.scope);
                outcome.scope = dornguard_core::Scope::Full;
                outcome.no_changes &= required.no_changes && required.scanned_files.is_empty();
                for diagnostic in &mut required.diagnostics {
                    diagnostic.severity = dornguard_core::Severity::Error;
                }
                outcome.merge(required);
            }
            let mut report = outcome.finish();
            report.filter_diagnostics(report_rule, report_path);
            let mut out = io::stdout().lock();
            match format {
                Format::Text => dornguard_report::write_text(&report, &mut out),
                Format::Json => dornguard_report::write_json(&report, &mut out),
                Format::Agent => dornguard_report::write_agent(&report, &mut out, 20),
                Format::Sarif => dornguard_report::write_sarif(&report, &mut out),
            }
            .into_diagnostic()
            .wrap_err("Could not write the check result")?;
            Ok(report.exit_code)
        }
        Action::Audit {
            project,
            format,
            maven_profile,
        } => audit::scan(&project, &maven_profile, format).await,
        Action::Csp { url, format } => csp::check(&url, format).await,
        Action::Init => {
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(CONFIG_FILE)
                .into_diagnostic()
                .wrap_err_with(|| format!("Could not create {CONFIG_FILE}"))?;
            if let Err(error) = (&file).write_all(DEFAULT_CONFIG.as_bytes()) {
                drop(file);
                let _ = fs::remove_file(CONFIG_FILE);
                return Err(error)
                    .into_diagnostic()
                    .wrap_err("Could not write the default configuration");
            }
            println!("Created {CONFIG_FILE}");
            Ok(0)
        }
        Action::Baseline {
            command:
                BaselineAction::Create {
                    output,
                    profile,
                    only_profile,
                    resolve_imports,
                    force,
                },
        } => {
            let outcome = check(
                Path::new("."),
                ProfileSelection::from_cli(&profile, &only_profile),
                None,
                None,
                None,
                None,
                resolve_imports,
            )
            .await;
            let report = outcome.finish();
            if !report.complete {
                dornguard_report::write_text(&report, &mut io::stderr().lock())
                    .into_diagnostic()?;
                return Ok(if report.exit_code == 0 {
                    3
                } else {
                    report.exit_code
                });
            }
            if !matches!(report.scope, Scope::Full) {
                bail!("Run baseline create at the project root");
            }
            let count = baseline::Baseline::create(&output, &report.diagnostics, force)?;
            println!("Recorded {count} violations in {}", output.display());
            Ok(0)
        }
        Action::Version => {
            println!(
                "dornguard {} (built-in ruleset {BUILTIN_VERSION})",
                env!("CARGO_PKG_VERSION")
            );
            Ok(0)
        }
        Action::Doctor { profile, config } => doctor(&profile, config.as_deref()).await,
        Action::Engines {
            command: EngineAction::List,
        } => {
            for engine in [
                EngineName::Semgrep,
                EngineName::Pmd,
                EngineName::Eslint,
                EngineName::Builtin,
            ] {
                println!("{} {}", engine.executable(), supported_version(engine));
            }
            Ok(0)
        }
        Action::Engines {
            command: EngineAction::Install { engine },
        } => {
            let engines: &[EngineName] = match engine {
                Some(InstallTarget::Semgrep) => &[EngineName::Semgrep],
                Some(InstallTarget::Pmd) => &[EngineName::Pmd],
                None => &[EngineName::Semgrep, EngineName::Pmd],
            };
            for &engine in engines {
                match install(engine).await {
                    Ok(path) => println!(
                        "{} {} installed at {}",
                        engine.executable(),
                        supported_version(engine),
                        path.display()
                    ),
                    Err(error) => {
                        eprintln!("Could not install {}: {error:#}", engine.executable());
                        return Ok(3);
                    }
                }
            }
            Ok(0)
        }
        Action::Profiles { command } => {
            let registry = current_registry()?;
            match command {
                ProfileAction::List => {
                    for profile in registry.profiles() {
                        println!("{}  {}", profile.name, profile.description);
                    }
                }
                ProfileAction::Show { name } => {
                    let profile = registry.profile(&name)?;
                    println!("{}  {}", profile.name, profile.description);
                    for parent in &profile.extends {
                        println!("  extends {parent}");
                    }
                    for rule in &profile.rules {
                        println!("  {rule}");
                    }
                }
            }
            Ok(0)
        }
        Action::Rules { command } => {
            let registry = current_registry()?;
            match command {
                RuleAction::List => {
                    for rule in registry.rules() {
                        println!("{}  {}", rule.id, rule.description);
                    }
                }
                RuleAction::Explain { id } => {
                    let rule = registry.rule(&id)?;
                    println!("{}  {}\nFix: {}", rule.id, rule.description, rule.fix);
                    let options = match rule.id.as_str() {
                        "format.indentation" => Some(
                            "format.indent_style (spaces), format.java_indent (4), format.kotlin_indent (4), format.javascript_indent (2), format.overrides[].indent_style and indent_size",
                        ),
                        "format.control-spacing" => Some(
                            "format.control_spacing (space), format.overrides[].control_spacing",
                        ),
                        "format.line-ending" => {
                            Some("format.line_ending (lf), format.overrides[].line_ending")
                        }
                        "format.blank-lines" => {
                            Some("format.max_blank_lines (1), format.overrides[].max_blank_lines")
                        }
                        "java.resource-not-closed" => Some(
                            "java_target, java_resource_owners, java_managed_resource_factories",
                        ),
                        "java.preserve-stack-trace" => Some("allow_logged_exceptions (false)"),
                        "thymeleaf.unescaped-output" => Some("thymeleaf_sanitizers"),
                        _ if rule.options.is_some() => {
                            Some("rules.<id>.options.from, rules.<id>.options.forbidden")
                        }
                        _ => None,
                    };
                    if let Some(options) = options {
                        println!("Configuration keys: {options}");
                    }
                    if rule.is_format_rule() {
                        println!(
                            "Use format.exclude for formatting-only globs, format.include_minified to check minified JavaScript, or check --only-profile format to run formatting alone."
                        );
                    }
                }
            }
            Ok(0)
        }
    }
}

async fn doctor(profiles: &[String], config: Option<&Path>) -> Result<u8> {
    let cwd = dunce::canonicalize(".").into_diagnostic()?;
    let config = discover_config(&cwd, config)?;
    let root = config.as_deref().and_then(Path::parent).unwrap_or(&cwd);
    let mut engines = BTreeSet::new();
    let mut needs_build_model = false;
    if let Some(path) = &config {
        let config = Registry::load(path)?;
        let registry = Registry::for_config(path, &config)?;
        for module in
            std::iter::once(None).chain(config.modules.keys().map(|name| Some(name.as_str())))
        {
            engines.extend(
                registry
                    .resolve(&config, module, profiles)?
                    .into_iter()
                    .map(|rule| rule.engine),
            );
        }
        needs_build_model = !config.modules.is_empty() || engines.contains(&EngineName::Archunit);
    } else if profiles.is_empty() {
        engines.extend([EngineName::Semgrep, EngineName::Pmd]);
        if std::env::var_os("DORNGUARD_ARCHUNIT_RUNNER").is_some() {
            engines.insert(EngineName::Archunit);
        }
    } else {
        let config = Registry::parse("version: 1\n")?;
        engines.extend(
            Registry::builtin()?
                .resolve(&config, None, profiles)?
                .into_iter()
                .map(|rule| rule.engine),
        );
        needs_build_model = engines.contains(&EngineName::Archunit);
    }

    let mut exit_code = 0;
    for engine in engines.iter().copied().filter(|engine| {
        matches!(
            engine,
            EngineName::Semgrep | EngineName::Pmd | EngineName::Eslint
        )
    }) {
        match inspect(engine, root).await {
            Ok(()) => println!("{} {} OK", engine.executable(), supported_version(engine)),
            Err(error) => {
                eprintln!("{}: {error:#}", engine.executable());
                exit_code = 3;
            }
        }
    }
    if engines.contains(&EngineName::Archunit) {
        match inspect_archunit(root).await {
            Ok(details) => println!("ArchUnit runner OK ({details})"),
            Err(error) => {
                eprintln!("ArchUnit runner: {error:#}");
                exit_code = 3;
            }
        }
    } else if config.is_none() && profiles.is_empty() {
        match inspect_java(root).await {
            Ok(java) => println!("ArchUnit runtime (Java 21+): {java}"),
            Err(error) => eprintln!("Optional ArchUnit runtime: {error:#}"),
        }
    }
    if needs_build_model
        && build_check::BuildSystem::detect(root) == build_check::BuildSystem::Maven
    {
        match maven_model_path() {
            Ok(path) => println!("Maven model extension OK ({})", path.display()),
            Err(error) => {
                eprintln!("{error:#}");
                exit_code = 3;
            }
        }
    }
    if engines.iter().all(|engine| *engine == EngineName::Builtin) && !needs_build_model {
        println!("No external engines required by the active rules.");
    }
    Ok(exit_code)
}

#[derive(Default)]
struct ProfileSelection<'a> {
    names: &'a [String],
    exclusive: bool,
}

impl<'a> ProfileSelection<'a> {
    fn from_cli(additional: &'a [String], only: &'a [String]) -> Self {
        Self {
            names: if only.is_empty() { additional } else { only },
            exclusive: !only.is_empty(),
        }
    }
}

async fn check(
    target: &Path,
    selection: ProfileSelection<'_>,
    config: Option<&Path>,
    module: Option<&str>,
    changed: Option<changed::Request<'_>>,
    organization: Option<&Path>,
    resolve_imports: bool,
) -> Outcome {
    let config_error = |error| failure(FailureKind::ConfigurationError, error);
    let prep_error = |error| failure(FailureKind::PreparationError, error);
    let target = match dunce::canonicalize(target)
        .into_diagnostic()
        .wrap_err_with(|| format!("Could not access {}", target.display()))
    {
        Ok(path) => path,
        Err(error) => return prep_error(error),
    };
    let project = match find_config(&target, config) {
        Ok(path) => path,
        Err(error) => return config_error(error),
    };
    let Some(root) = project.parent() else {
        return config_error(miette!("Configuration must be a file inside a project"));
    };
    let policy = if let Some(path) = organization {
        match trusted_policy(root, path) {
            Ok(path) => path,
            Err(error) => return config_error(error),
        }
    } else {
        project.clone()
    };
    let mut config = match Registry::load(&policy) {
        Ok(config) => config,
        Err(error) => return config_error(error),
    };
    if selection.exclusive {
        config.use_only_cli_profiles();
    }
    let profiles = selection.names;
    let registry = match Registry::for_config(&policy, &config) {
        Ok(registry) => registry,
        Err(error) => return config_error(error),
    };
    let resolved_rules = match registry.resolve(&config, None, profiles) {
        Ok(rules) => rules,
        Err(error) => return config_error(error),
    };
    let rules = resolved_rules.iter().collect::<Vec<_>>();
    let flyway_rule = rules.iter().copied().find(|rule| rule.id == FLYWAY_RULE);
    let mut jpa_enabled = rules.iter().any(|rule| rule.id == JPA_RESPONSE_RULE);
    let mut jspecify_enabled = rules
        .iter()
        .any(|rule| rule.id == "java.jspecify-null-marked");
    for module_name in config.modules.keys() {
        let module_rules = match registry.resolve(&config, Some(module_name), profiles) {
            Ok(rules) => rules,
            Err(error) => return config_error(error),
        };
        if module_rules
            .iter()
            .find(|rule| rule.id == FLYWAY_RULE)
            .map(|rule| rule.severity)
            != flyway_rule.map(|rule| rule.severity)
        {
            return config_error(miette!("The flyway rule is configured at project scope"));
        }
        jpa_enabled |= module_rules.iter().any(|rule| rule.id == JPA_RESPONSE_RULE);
        jspecify_enabled |= module_rules
            .iter()
            .any(|rule| rule.id == "java.jspecify-null-marked");
    }
    let global_kotlin = match registry.kotlin_profile_enabled(&config, None, profiles) {
        Ok(enabled) => enabled || rules.iter().any(|rule| rule.enables_kotlin()),
        Err(error) => return config_error(error),
    };
    let mut fallback_reason = None;
    let changes = if organization.is_none()
        && let Some(request) = changed
    {
        match changed::discover(root, &project, config.local_ruleset_path(), request).await {
            Ok(changes) if jspecify_enabled && changes.java_scope_changed() => {
                fallback_reason = Some("A Java package or module declaration changed".to_owned());
                None
            }
            Ok(changes)
                if jpa_enabled
                    && changes
                        .paths()
                        .any(|path| SourceLanguage::of(path) == Some(SourceLanguage::Java)) =>
            {
                fallback_reason = Some(
                    "A Java change may affect JPA response types in unchanged controllers"
                        .to_owned(),
                );
                None
            }
            Ok(changes) => Some(changes),
            Err(error) => {
                fallback_reason = Some(error.to_string());
                None
            }
        }
    } else {
        None
    };
    let scope = if changes.is_some() {
        Scope::Changed
    } else if organization.is_none() && changed.is_none() && (target != root || module.is_some()) {
        Scope::Selected
    } else {
        Scope::Full
    };
    let target = if changed.is_some() || organization.is_some() {
        root
    } else {
        &target
    };
    let git_outcome = if let Some(rule) = flyway_rule {
        match flyway::check(root, target, changed.and_then(|request| request.base), rule).await {
            Ok(outcome) => outcome,
            Err(error) => {
                let mut outcome = prep_error(error.wrap_err("Flyway Git comparison failed"));
                outcome.failures[0].set_rules([FLYWAY_RULE]);
                outcome
            }
        }
    } else {
        Outcome::default()
    };
    let uses_build = global_kotlin
        || module.is_some()
        || !config.modules.is_empty()
        || !config.module_dependencies.forbid.is_empty()
        || rules.iter().any(|rule| rule.engine == EngineName::Archunit);
    let tracked = if organization.is_some() {
        BTreeSet::new()
    } else {
        match changed::tracked_sources(root).await {
            Ok(tracked) => tracked,
            Err(error) => return prep_error(error),
        }
    };
    let sources = SourceFiles {
        root,
        tracked,
        required: organization.is_some(),
    };
    if uses_build || resolve_imports && rules.iter().any(|rule| rule.uses_pmd_classpath()) {
        let (module_git_outcome, git_outcome) = if module.is_some() {
            (git_outcome, Outcome::default())
        } else {
            (Outcome::default(), git_outcome)
        };
        let build = build_check::BuildSystem::detect(root);
        let maven_full = build == build_check::BuildSystem::Maven && changes.is_some();
        let mut outcome = build_check::check(build_check::Check {
            build,
            root,
            target,
            filter: module,
            registry: &registry,
            config: &config,
            profiles,
            global: &rules,
            global_kotlin,
            resolve_imports,
            sources: &sources,
            changes: if maven_full { None } else { changes.as_ref() },
            required: organization.is_some(),
            module_git_outcome,
        })
        .await;
        if !git_outcome.diagnostics.is_empty() {
            outcome.no_changes = false;
        }
        outcome.merge(git_outcome);
        if outcome.fallback_reason.is_none() {
            outcome.scope = if maven_full { Scope::Full } else { scope };
            outcome.fallback_reason = if maven_full {
                Some("Maven build checks scan the full reactor".to_owned())
            } else {
                fallback_reason
            };
        }
        if organization.is_none() {
            suppressions::apply(root, &config.ignore, &mut outcome);
        }
        return outcome;
    }
    let scoped_prep_error = |error| {
        let mut outcome = prep_error(error);
        outcome.scope = scope;
        outcome.fallback_reason = fallback_reason.clone();
        outcome
    };
    if rules.is_empty() {
        return scoped_prep_error(miette!(
            "No rules apply; select a profile with active rules"
        ));
    }
    let mut files = if rules.iter().all(|rule| rule.id == FLYWAY_RULE) {
        Vec::new()
    } else {
        match sources.collect(target, |language| {
            rules.iter().any(|rule| rule.applies_to(language))
        }) {
            Ok(files) => files,
            Err(error) => {
                let mut outcome = scoped_prep_error(error);
                outcome.merge(git_outcome);
                return outcome;
            }
        }
    };
    if let Some(changes) = &changes {
        files.retain(|file| changes.matches(file));
        if files.is_empty() && flyway_rule.is_none() {
            return Outcome {
                scope,
                no_changes: true,
                ..Outcome::default()
            };
        }
    }
    if files.is_empty() {
        if flyway_rule.is_some() {
            let mut outcome = git_outcome;
            outcome.scope = scope;
            outcome.fallback_reason = fallback_reason;
            outcome.no_changes =
                changes.is_some() && outcome.diagnostics.is_empty() && outcome.failures.is_empty();
            if organization.is_none() {
                suppressions::apply(root, &config.ignore, &mut outcome);
            }
            return outcome;
        }
        return scoped_prep_error(miette!("No source files apply to this check"));
    }
    let previous_count = files.len();
    files.retain(|file| !config.excludes_source(file));
    let mut outcome = run_source_engines(
        root,
        &files,
        &registry,
        &rules,
        &config,
        None,
        organization.is_some(),
    )
    .await;
    outcome.merge(git_outcome);
    outcome.no_changes = previous_count > 0 && files.is_empty() && flyway_rule.is_none();
    outcome.scope = scope;
    outcome.fallback_reason = fallback_reason;
    if organization.is_none() {
        suppressions::apply(root, &config.ignore, &mut outcome);
    }
    outcome
}

async fn run_source_engines(
    root: &Path,
    files: &[PathBuf],
    registry: &Registry,
    rules: &[&dornguard_config::Rule],
    config: &ProjectConfig,
    java_source: Option<u16>,
    required: bool,
) -> Outcome {
    let warn_on_thymeleaf_partial =
        !required && config.thymeleaf_partial_parsing == PartialParsing::Warning;
    let check_group = async |engine, files: Vec<PathBuf>, accepts_rule: fn(&Rule) -> bool| {
        let languages = files
            .iter()
            .filter_map(|path| SourceLanguage::of(path))
            .collect::<std::collections::BTreeSet<_>>();
        let rules = rules
            .iter()
            .copied()
            .filter(|rule| {
                rule.engine == engine
                    && accepts_rule(rule)
                    && languages.iter().any(|&language| rule.applies_to(language))
            })
            .collect::<Vec<_>>();
        if rules.is_empty() {
            return Outcome::default();
        }
        let files = files
            .into_iter()
            .filter(|path| {
                SourceLanguage::of(path)
                    .is_some_and(|language| rules.iter().any(|rule| rule.applies_to(language)))
            })
            .collect::<Vec<_>>();
        let request = Request {
            root,
            files: &files,
            rules: &rules,
            registry,
            required,
        };
        match engine {
            EngineName::Semgrep => {
                Semgrep::with_thymeleaf(&config.thymeleaf_sanitizers, warn_on_thymeleaf_partial)
                    .check(request)
                    .await
            }
            EngineName::Pmd => {
                Pmd {
                    java_target: config.java_target,
                    java_source: config.java_source.or(java_source),
                    allow_logged_exceptions: config.allow_logged_exceptions,
                    resource_owners: &config.java_resource_owners,
                    managed_resource_factories: &config.java_managed_resource_factories,
                    ..Pmd::default()
                }
                .check(request)
                .await
            }
            EngineName::Eslint => Eslint.check(request).await,
            EngineName::Builtin => check_builtin(request, &config.format).await,
            _ => unreachable!("source runner requires a source engine"),
        }
    };
    let semgrep = async {
        // Keep template rules off ordinary HTML and generic JVM rules off web sources.
        let mut outcome = Outcome::default();
        for group in [
            &[SourceLanguage::Java, SourceLanguage::Kotlin][..],
            &[SourceLanguage::Template],
            &[SourceLanguage::Html],
            &[SourceLanguage::Javascript],
            &[SourceLanguage::Jsp],
        ] {
            let files: Vec<_> = files
                .iter()
                .filter(|path| SourceLanguage::of(path).is_some_and(|kind| group.contains(&kind)))
                .cloned()
                .collect();
            if warn_on_thymeleaf_partial && group == [SourceLanguage::Template] {
                outcome.merge(
                    check_group(EngineName::Semgrep, files.clone(), |rule| {
                        rule.id.starts_with("thymeleaf.")
                    })
                    .await,
                );
                outcome.merge(
                    check_group(EngineName::Semgrep, files, |rule| {
                        !rule.id.starts_with("thymeleaf.")
                    })
                    .await,
                );
            } else {
                outcome.merge(check_group(EngineName::Semgrep, files, |_| true).await);
            }
        }
        outcome
    };
    let (mut outcome, pmd, eslint, builtin) = tokio::join!(
        semgrep,
        check_group(EngineName::Pmd, files.to_vec(), |_| true),
        check_group(EngineName::Eslint, files.to_vec(), |_| true),
        check_group(EngineName::Builtin, files.to_vec(), |_| true),
    );
    outcome.merge(pmd);
    outcome.merge(eslint);
    outcome.merge(builtin);
    outcome
}

fn find_config(target: &Path, config: Option<&Path>) -> Result<PathBuf> {
    discover_config(target, config)?
        .ok_or_else(|| miette!("No {CONFIG_FILE} found; run dornguard init at the project root"))
}

fn discover_config(target: &Path, config: Option<&Path>) -> Result<Option<PathBuf>> {
    if let Some(path) = config {
        return dunce::canonicalize(path)
            .map(Some)
            .into_diagnostic()
            .wrap_err_with(|| format!("Could not access configuration {}", path.display()));
    }
    let directory = if target.is_dir() {
        Some(target)
    } else {
        target.parent()
    }
    .ok_or_else(|| miette!("No project directory found"))?;
    for ancestor in directory.ancestors() {
        let path = ancestor.join(CONFIG_FILE);
        match fs::symlink_metadata(&path) {
            Ok(_) => return Ok(Some(path)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).into_diagnostic().wrap_err_with(|| {
                    format!("Could not access configuration {}", path.display())
                });
            }
        }
    }
    Ok(None)
}

fn current_registry() -> Result<Registry> {
    let cwd = dunce::canonicalize(".")
        .into_diagnostic()
        .wrap_err("Could not resolve the current directory")?;
    if let Some(path) = cwd
        .ancestors()
        .map(|ancestor| ancestor.join(CONFIG_FILE))
        .find(|path| path.is_file())
    {
        let config = Registry::load(&path)?;
        Registry::for_config(&path, &config)
    } else {
        Registry::builtin()
    }
}

fn trusted_policy(root: &Path, path: &Path) -> Result<PathBuf> {
    let path = dunce::canonicalize(path)
        .into_diagnostic()
        .wrap_err("Could not access the organization policy")?;
    let mut worktree = root.to_path_buf();
    for directory in root.ancestors() {
        if directory.join(".git").exists() {
            worktree = directory.to_path_buf();
            break;
        }
    }
    if path.starts_with(&worktree) {
        bail!("Organization policy must be outside the project worktree");
    }
    let config = Registry::load(&path)?;
    config.validate_trusted_ruleset(&path, &worktree)?;
    Ok(path)
}

struct SourceFiles<'a> {
    root: &'a Path,
    tracked: BTreeSet<PathBuf>,
    required: bool,
}

impl SourceFiles<'_> {
    fn collect(
        &self,
        target: &Path,
        accepts: impl Fn(SourceLanguage) -> bool,
    ) -> Result<Vec<PathBuf>> {
        let root = self.root;
        let required = self.required;
        if !target.starts_with(root) {
            bail!("The check target must be inside the configuration directory");
        }
        let mut files = BTreeSet::new();
        let mut walker = WalkBuilder::new(target);
        let web = [
            SourceLanguage::Html,
            SourceLanguage::Template,
            SourceLanguage::Javascript,
            SourceLanguage::Jsp,
        ]
        .into_iter()
        .any(&accepts);
        walker
            .require_git(false)
            .git_global(false)
            .git_exclude(false)
            .ignore(false)
            .hidden(!required)
            .git_ignore(!required)
            .filter_entry(move |entry| {
                entry.file_name() != ".git" && (!web || entry.file_name() != "node_modules")
            });
        let allowed = |relative: &Path| {
            SourceLanguage::of(relative).is_some_and(&accepts)
                && !relative.components().any(|part| {
                    part.as_os_str() == ".git" || web && part.as_os_str() == "node_modules"
                })
        };
        for entry in walker.build() {
            let entry = entry
                .into_diagnostic()
                .wrap_err("Could not enumerate source files")?;
            if let Some(error) = entry.error() {
                bail!("Could not read project ignore rules: {error}");
            }
            let relative = entry.path().strip_prefix(root).into_diagnostic()?;
            if entry.file_type().is_some_and(|kind| kind.is_file()) && allowed(relative) {
                if relative.to_str().is_none() {
                    bail!(
                        "Source path is not representable as UTF-8: {}",
                        entry.path().display()
                    );
                }
                files.insert(relative.to_path_buf());
            }
        }
        let relative_target = target.strip_prefix(root).into_diagnostic()?;
        for path in self
            .tracked
            .iter()
            .filter(|path| path.starts_with(relative_target) && allowed(path))
        {
            if files.contains(path) {
                continue;
            }
            let absolute = root.join(path);
            match fs::symlink_metadata(&absolute) {
                Ok(metadata) if metadata.is_file() => {
                    // The walker does not follow symlinks, including parents of tracked paths.
                    if dunce::canonicalize(&absolute).into_diagnostic()? == absolute {
                        files.insert(path.clone());
                    }
                }
                Ok(_) => {}
                Err(error)
                    if error.kind() == io::ErrorKind::NotFound
                        || error.kind() == io::ErrorKind::NotADirectory => {}
                Err(error) => {
                    return Err(error).into_diagnostic().wrap_err_with(|| {
                        format!("Could not inspect tracked source {}", path.display())
                    });
                }
            }
        }
        Ok(files.into_iter().collect())
    }
}

fn failure(kind: FailureKind, error: miette::Report) -> Outcome {
    Outcome {
        failures: vec![Failure::new(kind, format!("{error:#}"))],
        ..Outcome::default()
    }
}
