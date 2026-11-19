use std::future::Future;
use std::path::{Component, Path, PathBuf};
use std::process::{Output, Stdio};
use std::time::Duration;

use dornguard_config::{Registry, Rule};
use dornguard_core::{EngineName, Failure, FailureKind, Outcome};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use tokio::process::Command;
use tokio::time::{Instant, timeout_at};

mod archunit;
mod install;
mod pmd;
mod semgrep;
mod source;

pub use archunit::{ArchModule, check_architecture, maven_model_path};
pub use install::install;
pub use pmd::Pmd;
pub use semgrep::Semgrep;

pub const SEMGREP_VERSION: &str = "1.182.0";
pub const PMD_VERSION: &str = "7.29.0";

pub struct Request<'a> {
    pub root: &'a Path,
    pub files: &'a [PathBuf],
    pub rules: &'a [&'a Rule],
    pub registry: &'a Registry,
}

pub trait Engine {
    fn check(&self, request: Request<'_>) -> impl Future<Output = Outcome> + Send;
}

pub fn supported_version(engine: EngineName) -> &'static str {
    match engine {
        EngineName::Semgrep => SEMGREP_VERSION,
        EngineName::Pmd => PMD_VERSION,
        EngineName::Archunit => "21",
        EngineName::Gradle => "9.9.1",
    }
}

pub async fn inspect(engine: EngineName, root: &Path) -> Result<()> {
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
        EngineName::Pmd => text.lines().find_map(|line| {
            line.strip_prefix("PMD ")
                .and_then(|rest| rest.split_whitespace().next())
        }),
        EngineName::Archunit | EngineName::Gradle => None,
    };
    let Some(detected) = detected.filter(|value| !value.is_empty()) else {
        bail!("{} did not report a version", engine.executable());
    };
    if detected != supported_version(engine) {
        bail!(
            "Unsupported {} version; install {}",
            engine.executable(),
            supported_version(engine)
        );
    }
    Ok(())
}

fn command(engine: EngineName, root: &Path) -> Result<Command> {
    let mut command = install::engine_command(engine)?;
    command.current_dir(root).kill_on_drop(true);
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

fn fail(outcome: &mut Outcome, engine: EngineName, message: impl Into<String>) {
    outcome.failures.push(Failure {
        kind: FailureKind::EngineError,
        engine: Some(engine),
        message: message.into(),
    });
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
