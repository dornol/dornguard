use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand, ValueEnum};
use dornguard_config::{BUILTIN_VERSION, CONFIG_FILE, DEFAULT_CONFIG, Registry};
use dornguard_core::{EngineName, Failure, FailureKind, Outcome, Scope, SourceLanguage};
use dornguard_engine::{Engine, Pmd, Request, Semgrep, inspect, install, supported_version};
use ignore::WalkBuilder;
use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};

mod baseline;
mod build_check;
mod changed;
mod maven;

#[derive(Parser)]
#[command(
    name = "dornguard",
    version,
    about = "Run your project's code quality policy"
)]
struct Cli {
    #[command(subcommand)]
    command: Action,
}

#[derive(Subcommand)]
enum Action {
    /// Check a project directory or JVM source file
    Check {
        #[arg(default_value = ".")]
        target: PathBuf,
        #[arg(long)]
        profile: Vec<String>,
        #[arg(long, value_enum, default_value_t = Format::Text)]
        format: Format,
        #[arg(long, value_name = "PATH")]
        config: Option<PathBuf>,
        #[arg(long, value_name = "MODULE")]
        module: Option<String>,
        /// Limit the check to changes in the index and working tree
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
    /// Create a default configuration in the current directory
    Init,
    /// Print the installed CLI version
    Version,
    /// Verify that supported engine versions are available
    Doctor,
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
    List,
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
        #[arg(long)]
        profile: Vec<String>,
        /// Replace an existing baseline
        #[arg(long)]
        force: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Format {
    Text,
    Json,
    Agent,
    Sarif,
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
            format,
            config,
            module,
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
                    &profile,
                    config.as_deref(),
                    module.as_deref(),
                    changed.then_some(changed::Request {
                        base: base.as_deref(),
                        baseline: baseline.as_deref(),
                    }),
                    None,
                )
                .await;
                if let Some(baseline) = loaded_baseline {
                    baseline.apply(&mut outcome);
                }
                outcome
            }
            .await;
            if let Some(policy) = &organization_policy {
                let mut required =
                    check(&target, &[], config.as_deref(), None, None, Some(policy)).await;
                outcome.organization_policy_enforced = required.failures.is_empty();
                for diagnostic in &mut required.diagnostics {
                    diagnostic.severity = dornguard_core::Severity::Error;
                }
                outcome.merge(required);
            }
            let report = outcome.finish();
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
                    force,
                },
        } => {
            let outcome = check(Path::new("."), &profile, None, None, None, None).await;
            let report = outcome.finish();
            if !report.complete {
                dornguard_report::write_text(&report, &mut io::stderr().lock())
                    .into_diagnostic()?;
                return Ok(report.exit_code);
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
        Action::Doctor => {
            let root = std::env::current_dir().into_diagnostic()?;
            let mut exit_code = 0;
            for engine in [EngineName::Semgrep, EngineName::Pmd] {
                match inspect(engine, &root).await {
                    Ok(()) => println!("{} {} OK", engine.executable(), supported_version(engine)),
                    Err(error) => {
                        eprintln!("{}: {error:#}", engine.executable());
                        exit_code = 3;
                    }
                }
            }
            Ok(exit_code)
        }
        Action::Engines {
            command: EngineAction::List,
        } => {
            for engine in [EngineName::Semgrep, EngineName::Pmd] {
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
                }
            }
            Ok(0)
        }
    }
}

async fn check(
    target: &Path,
    profiles: &[String],
    config: Option<&Path>,
    module: Option<&str>,
    changed: Option<changed::Request<'_>>,
    organization: Option<&Path>,
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
    let config = match Registry::load(&policy) {
        Ok(config) => config,
        Err(error) => return config_error(error),
    };
    let registry = match Registry::for_config(&policy, &config) {
        Ok(registry) => registry,
        Err(error) => return config_error(error),
    };
    let resolved_rules = match registry.resolve(&config, None, profiles) {
        Ok(rules) => rules,
        Err(error) => return config_error(error),
    };
    let rules = resolved_rules.iter().collect::<Vec<_>>();
    let global_kotlin = match registry.kotlin_profile_enabled(&config, None, profiles) {
        Ok(enabled) => enabled || rules.iter().any(|rule| rule.enables_kotlin()),
        Err(error) => return config_error(error),
    };
    let mut fallback_reason = None;
    let changes = if let Some(request) = changed {
        match changed::discover(root, &project, config.local_ruleset_path(), request).await {
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
    if global_kotlin
        || module.is_some()
        || !config.modules.is_empty()
        || !config.module_dependencies.forbid.is_empty()
        || rules.iter().any(|rule| rule.engine == EngineName::Archunit)
    {
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
            changes: if maven_full { None } else { changes.as_ref() },
            required: organization.is_some(),
        })
        .await;
        if outcome.fallback_reason.is_none() {
            outcome.scope = if maven_full { Scope::Full } else { scope };
            outcome.fallback_reason = if maven_full {
                Some("Maven build checks scan the full reactor".to_owned())
            } else {
                fallback_reason
            };
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
    let mut files = match source_files(root, target, organization.is_some(), |language| {
        language == SourceLanguage::Java && rules.iter().any(|rule| rule.applies_to(language))
    }) {
        Ok(files) => files,
        Err(error) => return scoped_prep_error(error),
    };
    if let Some(changes) = &changes {
        files.retain(|file| changes.matches(file));
        if files.is_empty() {
            return Outcome {
                scope,
                no_changes: true,
                ..Outcome::default()
            };
        }
    }
    if files.is_empty() {
        return scoped_prep_error(miette!("No Java source files apply to this check"));
    }
    let mut outcome = run_source_engines(root, &files, &registry, &rules).await;
    outcome.scope = scope;
    outcome.fallback_reason = fallback_reason;
    outcome
}

async fn run_source_engines(
    root: &Path,
    files: &[PathBuf],
    registry: &Registry,
    rules: &[&dornguard_config::Rule],
) -> Outcome {
    let languages = files
        .iter()
        .filter_map(|path| SourceLanguage::of(path))
        .collect::<std::collections::BTreeSet<_>>();
    let selected = |engine| {
        rules
            .iter()
            .copied()
            .filter(|rule| {
                rule.engine == engine && languages.iter().any(|&language| rule.applies_to(language))
            })
            .collect::<Vec<_>>()
    };
    let (semgrep_rules, pmd_rules) = (selected(EngineName::Semgrep), selected(EngineName::Pmd));
    let files_for = |rules: &[&dornguard_config::Rule]| {
        files
            .iter()
            .filter(|path| {
                SourceLanguage::of(path)
                    .is_some_and(|language| rules.iter().any(|rule| rule.applies_to(language)))
            })
            .cloned()
            .collect::<Vec<_>>()
    };
    let (semgrep_files, pmd_files) = (files_for(&semgrep_rules), files_for(&pmd_rules));
    let semgrep = async {
        if semgrep_rules.is_empty() {
            Outcome::default()
        } else {
            Semgrep::default()
                .check(Request {
                    root,
                    files: &semgrep_files,
                    rules: &semgrep_rules,
                    registry,
                })
                .await
        }
    };
    let pmd = async {
        if pmd_rules.is_empty() {
            Outcome::default()
        } else {
            Pmd.check(Request {
                root,
                files: &pmd_files,
                rules: &pmd_rules,
                registry,
            })
            .await
        }
    };
    let (mut outcome, pmd_outcome) = tokio::join!(semgrep, pmd);
    outcome.merge(pmd_outcome);
    outcome
}

fn find_config(target: &Path, config: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = config {
        return dunce::canonicalize(path)
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
            Ok(_) => return Ok(path),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).into_diagnostic().wrap_err_with(|| {
                    format!("Could not access configuration {}", path.display())
                });
            }
        }
    }
    bail!("No {CONFIG_FILE} found; run dornguard init at the project root");
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

fn source_files(
    root: &Path,
    target: &Path,
    required: bool,
    accepts: impl Fn(SourceLanguage) -> bool,
) -> Result<Vec<PathBuf>> {
    if !target.starts_with(root) {
        bail!("The check target must be inside the configuration directory");
    }
    let mut files = Vec::new();
    let mut walker = WalkBuilder::new(target);
    walker
        .require_git(false)
        .git_global(false)
        .git_exclude(false)
        .ignore(false)
        .hidden(!required)
        .git_ignore(!required)
        .filter_entry(|entry| entry.file_name() != ".git");
    for entry in walker.build() {
        let entry = entry
            .into_diagnostic()
            .wrap_err("Could not enumerate source files")?;
        if let Some(error) = entry.error() {
            bail!("Could not read project ignore rules: {error}");
        }
        let path = entry.path();
        if entry.file_type().is_some_and(|kind| kind.is_file())
            && SourceLanguage::of(path).is_some_and(&accepts)
        {
            let relative = path.strip_prefix(root).into_diagnostic()?;
            if relative.to_str().is_none() {
                bail!(
                    "Source path is not representable as UTF-8: {}",
                    path.display()
                );
            }
            files.push(relative.to_path_buf());
        }
    }
    files.sort();
    Ok(files)
}

fn failure(kind: FailureKind, error: miette::Report) -> Outcome {
    Outcome {
        failures: vec![Failure {
            kind,
            engine: None,
            message: format!("{error:#}"),
        }],
        ..Outcome::default()
    }
}
