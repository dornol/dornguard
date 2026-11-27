use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::process::ExitCode;

use clap::Parser;
use dornguard_config::{BUILTIN_VERSION, CONFIG_FILE, DEFAULT_CONFIG};
use dornguard_core::{Confidence, EngineName, FailureKind, Scope};
use dornguard_engine::{install, supported_version};
use miette::{IntoDiagnostic, Result, WrapErr, bail};

pub(crate) use check::*;
pub(crate) use cli::*;
pub(crate) use doctor::*;

mod audit;
mod baseline;
mod build_check;
mod changed;
mod check;
mod cli;
mod csp;
mod doctor;
mod flyway;
mod maven;
mod overlap;
mod suppressions;

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

async fn run_check(args: CheckArgs) -> Result<u8> {
    let CheckArgs {
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
        strict_changed,
        ratchet,
        organization_policy,
    } = args;
    let mut outcome = async {
        let loaded_baseline = match baseline
            .as_deref()
            .map(baseline::Baseline::load)
            .transpose()
        {
            Ok(baseline) => baseline,
            Err(error) => return failure(FailureKind::ConfigurationError, error),
        };
        let mut outcome = Box::pin(check(
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
        ))
        .await;
        if let Some(baseline) = loaded_baseline {
            baseline.apply_project(
                &mut outcome,
                baseline::ProjectOptions {
                    strict_changed,
                    ratchet,
                },
            );
        }
        outcome
    }
    .await;
    if let Some(policy) = &organization_policy {
        let mut required = Box::pin(check(
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
        ))
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

fn explain_rule(registry: &dornguard_config::Registry, id: &str) -> Result<()> {
    let rule = registry.rule(id)?;
    println!("{}  {}\nFix: {}", rule.id, rule.description, rule.fix);
    println!(
        "Confidence: {}",
        match rule.confidence {
            Confidence::High => "high (a finding needs fixing)",
            Confidence::Medium => "medium (confirm the finding, or suppress it with a reason)",
        }
    );
    let options = match rule.id.as_str() {
        "format.indentation" => Some(
            "Off by default; enable with rules.format.indentation.enabled: true. format.indent_style (spaces), format.java_indent (4), format.kotlin_indent (4), format.javascript_indent (2), format.overrides[].indent_style and indent_size",
        ),
        "format.control-spacing" => {
            Some("format.control_spacing (space), format.overrides[].control_spacing")
        }
        "format.line-ending" => Some("format.line_ending (lf), format.overrides[].line_ending"),
        "format.blank-lines" => {
            Some("format.max_blank_lines (1), format.overrides[].max_blank_lines")
        }
        "java.resource-not-closed" => {
            Some("java_target, java_resource_owners, java_managed_resource_factories")
        }
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

    Ok(())
}

async fn create_baseline(
    output: std::path::PathBuf,
    profile: Vec<String>,
    only_profile: Vec<String>,
    resolve_imports: bool,
    force: bool,
) -> Result<u8> {
    let outcome = Box::pin(check(
        Path::new("."),
        ProfileSelection::from_cli(&profile, &only_profile),
        None,
        None,
        None,
        None,
        resolve_imports,
    ))
    .await;
    let report = outcome.finish();
    if !report.complete {
        dornguard_report::write_text(&report, &mut io::stderr().lock()).into_diagnostic()?;
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

async fn install_engines(engine: Option<InstallTarget>) -> Result<u8> {
    let engines: &[EngineName] = match engine {
        Some(InstallTarget::Semgrep) => &[EngineName::Semgrep],
        Some(InstallTarget::Pmd) => &[EngineName::Pmd],
        Some(InstallTarget::Eslint) => &[EngineName::Eslint],
        None => &[EngineName::Semgrep, EngineName::Pmd],
    };
    for &engine in engines {
        match install(engine).await {
            Ok(path) => println!(
                "{} {} installed at {}",
                engine.executable(),
                if engine == EngineName::Eslint {
                    dornguard_engine::ESLINT_INSTALL_VERSION
                } else {
                    supported_version(engine)
                },
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

async fn dispatch(action: Action) -> Result<u8> {
    match action {
        Action::Check(args) => Box::pin(run_check(args)).await,
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
        } => create_baseline(output, profile, only_profile, resolve_imports, force).await,
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
        } => install_engines(engine).await,
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
                RuleAction::Explain { id } => explain_rule(&registry, &id)?,
            }
            Ok(0)
        }
    }
}
