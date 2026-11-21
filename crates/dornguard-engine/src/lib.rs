use std::future::Future;
use std::path::{Component, Path, PathBuf};
use std::process::{Output, Stdio};
use std::time::Duration;

use dornguard_config::{
    ASYNC_SELF_RULE, CACHE_SELF_RULE, FormatConfig, JPA_RESPONSE_RULE, PartialParsingPolicy,
    RETRY_SELF_RULE, Registry, Rule, TRANSACTIONAL_SELF_RULE,
};
use dornguard_core::{EngineName, Failure, FailureKind, FailureStage, Outcome, SourceLanguage};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use tokio::process::Command;
use tokio::time::{Instant, timeout_at};

mod archunit;
mod eslint;
mod format;
mod install;
mod java_names;
mod jspecify;
mod pmd;
mod semgrep;
mod source;
mod spring;
mod thymeleaf;

pub use archunit::{ArchModule, check_architecture, inspect_archunit, maven_model_path};
pub use eslint::Eslint;
pub use install::{inspect_java, install};
pub use pmd::Pmd;
pub use semgrep::Semgrep;

pub const SEMGREP_VERSION: &str = "1.182.0";
pub const PMD_VERSION: &str = "7.29.0";

#[derive(Clone, Copy)]
pub struct Request<'a> {
    pub root: &'a Path,
    pub files: &'a [PathBuf],
    pub rules: &'a [&'a Rule],
    pub registry: &'a Registry,
    pub required: bool,
}

impl Request<'_> {
    fn mark_incomplete(&self, outcome: &mut Outcome) {
        for failure in outcome
            .failures
            .iter_mut()
            .chain(&mut outcome.analysis_warnings)
        {
            failure.file_count.get_or_insert(self.files.len());
            if failure.rule_ids.is_empty() {
                failure.set_rules(self.rules.iter().map(|rule| rule.id.as_str()));
            }
        }
        outcome.record_coverage(self.files);
    }
}

pub trait Engine {
    fn check(&self, request: Request<'_>) -> impl Future<Output = Outcome> + Send;
}

pub async fn check_builtin(
    request: Request<'_>,
    settings: &FormatConfig,
    partial: &PartialParsingPolicy,
) -> Outcome {
    let (format_rules, java_rules): (Vec<_>, Vec<_>) = request
        .rules
        .iter()
        .copied()
        .partition(|rule| rule.is_format_rule());
    let mut outcome = Outcome::default();
    if !java_rules.is_empty() {
        let java_files = request
            .files
            .iter()
            .filter(|path| SourceLanguage::of(path) == Some(SourceLanguage::Java))
            .cloned()
            .collect::<Vec<_>>();
        let (spring_rules, jspecify_rules): (Vec<_>, Vec<_>) =
            java_rules.into_iter().partition(|rule| {
                matches!(
                    rule.id.as_str(),
                    ASYNC_SELF_RULE
                        | CACHE_SELF_RULE
                        | RETRY_SELF_RULE
                        | TRANSACTIONAL_SELF_RULE
                        | JPA_RESPONSE_RULE
                )
            });
        if !jspecify_rules.is_empty() {
            outcome.merge(
                jspecify::JSpecify {
                    partial: partial.clone(),
                }
                .check(Request {
                    files: &java_files,
                    rules: &jspecify_rules,
                    ..request
                })
                .await,
            );
        }
        if !spring_rules.is_empty() {
            outcome.merge(spring::check(
                Request {
                    files: &java_files,
                    rules: &spring_rules,
                    ..request
                },
                request.required,
            ));
        }
    }
    if !format_rules.is_empty() {
        let files = request
            .files
            .iter()
            .filter(|path| !settings.excludes_file(path))
            .cloned()
            .collect::<Vec<_>>();
        let format_request = Request {
            files: &files,
            rules: &format_rules,
            ..request
        };
        let mut result = format::check(format_request, settings);
        format_request.mark_incomplete(&mut result);
        outcome.merge(result);
    }
    outcome
}

pub fn supported_version(engine: EngineName) -> &'static str {
    match engine {
        EngineName::Semgrep => SEMGREP_VERSION,
        EngineName::Pmd => PMD_VERSION,
        EngineName::Eslint => "9.x or 10.x",
        EngineName::Builtin => "bundled",
        EngineName::Archunit => "21",
        EngineName::Gradle => "9.9.1",
    }
}

pub async fn inspect(engine: EngineName, root: &Path) -> Result<()> {
    if engine == EngineName::Builtin {
        return Ok(());
    }
    check_version(engine, root, Instant::now() + Duration::from_secs(30)).await
}

async fn check_version(engine: EngineName, root: &Path, deadline: Instant) -> Result<()> {
    let mut command = command(engine, root)?;
    command.arg("--version");
    if engine == EngineName::Semgrep {
        command.args(["--disable-version-check", "--metrics=off"]);
    }
    let version = run_command(engine, &mut command, deadline).await?;
    if !version.status.success() {
        bail!(
            "{} version check failed ({})",
            engine.executable(),
            version.status
        );
    }
    let text = String::from_utf8_lossy(&version.stdout);
    let detected = match engine {
        EngineName::Semgrep => Some(text.trim()),
        EngineName::Eslint => text.trim().strip_prefix('v'),
        EngineName::Pmd => text.lines().find_map(|line| {
            line.strip_prefix("PMD ")
                .and_then(|rest| rest.split_whitespace().next())
        }),
        EngineName::Archunit | EngineName::Builtin | EngineName::Gradle => None,
    };
    let Some(detected) = detected.filter(|value| !value.is_empty()) else {
        bail!("{} did not report a version", engine.executable());
    };
    let supported = if engine == EngineName::Eslint {
        detected
            .split_once('.')
            .is_some_and(|(major, _)| matches!(major, "9" | "10"))
    } else {
        detected == supported_version(engine)
    };
    if !supported {
        bail!(
            "Unsupported {} version; install {}",
            engine.executable(),
            supported_version(engine)
        );
    }
    Ok(())
}

fn command(engine: EngineName, root: &Path) -> Result<Command> {
    let mut command = if engine == EngineName::Eslint {
        let executable = if cfg!(windows) {
            "eslint.cmd"
        } else {
            "eslint"
        };
        let local = root.join("node_modules").join(".bin").join(executable);
        if local.is_file() {
            Command::new(local)
        } else {
            Command::new(executable)
        }
    } else {
        install::engine_command(engine)?
    };
    command.current_dir(root).kill_on_drop(true);
    if engine == EngineName::Eslint {
        command.env_remove("NODE_OPTIONS");
    }
    let prefix = format!("{}_", engine.executable().to_ascii_uppercase());
    for (key, _) in std::env::vars_os() {
        if key
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with(&prefix)
        {
            command.env_remove(key);
        }
    }
    Ok(command)
}

async fn run_command(
    engine: EngineName,
    command: &mut Command,
    deadline: Instant,
) -> Result<Output> {
    if Instant::now() >= deadline {
        bail!("{} timed out", engine.executable());
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    command.process_group(0);
    let child = command.spawn().into_diagnostic().wrap_err_with(|| {
        format!(
            "Could not execute {}; install version {} and make it available on PATH",
            engine.executable(),
            supported_version(engine)
        )
    })?;
    #[cfg(unix)]
    let group = child
        .id()
        .and_then(|id| rustix::process::Pid::from_raw(id as i32));
    let output = match timeout_at(deadline, child.wait_with_output()).await {
        Ok(result) => result
            .into_diagnostic()
            .wrap_err("Could not collect engine output")?,
        Err(_) => {
            #[cfg(unix)]
            if let Some(group) = group {
                let _ = rustix::process::kill_process_group(group, rustix::process::Signal::KILL);
            }
            bail!("{} timed out", engine.executable());
        }
    };
    tracing::debug!(status = %output.status, "Engine process exited");
    Ok(output)
}

fn relative_path(root: &Path, path: &Path) -> Option<PathBuf> {
    let relative = if path.is_absolute() {
        path.strip_prefix(root).ok()?
    } else {
        path
    };
    let mut normalized = PathBuf::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!normalized.as_os_str().is_empty()).then_some(normalized)
}

fn display_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn diagnostic_message(description: &str, subject: Option<&str>) -> String {
    subject.map_or_else(
        || description.to_owned(),
        |subject| format!("{} (`{subject}`).", description.trim_end_matches('.')),
    )
}

fn fail(outcome: &mut Outcome, engine: EngineName, message: impl Into<String>) {
    outcome.failures.push(engine_failure(engine, message));
}

fn engine_failure(engine: EngineName, message: impl Into<String>) -> Failure {
    let mut failure = Failure::new(FailureKind::EngineError, message);
    failure.engine = Some(engine);
    failure.stage = Some(FailureStage::Engine);
    failure
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn times_out_when_the_engine_hangs() {
        let mut command = Command::new("/bin/sleep");
        command.kill_on_drop(true).arg("10");
        let error = run_command(
            EngineName::Semgrep,
            &mut command,
            Instant::now() + Duration::from_millis(100),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("timed out"));
    }
}
