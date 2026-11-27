use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Duration;

use dornguard_core::ProjectModel;
use dornguard_engine::maven_model_path;
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use tokio::process::Command;

pub async fn collect(root: &Path, compile: bool) -> Result<ProjectModel> {
    let extension = maven_model_path()?;
    let mut extension_option = OsString::from("-Dmaven.ext.class.path=");
    extension_option.push(extension);

    let temp = tempfile::tempdir()
        .into_diagnostic()
        .wrap_err("Could not create temporary Maven model directory")?;
    let model_path = temp.path().join("model.json");
    let mut command = command(root);
    command
        .env("DORNGUARD_MAVEN_MODEL", &model_path)
        .arg(extension_option)
        .arg(if compile { "compile" } else { "validate" });
    let output = run(&mut command).await?;
    let data = fs::read(model_path).into_diagnostic().wrap_err_with(|| {
        format!(
            "Maven did not export a complete project model\n{}",
            log(&output)
        )
    })?;
    let model: ProjectModel = serde_json::from_slice(&data)
        .into_diagnostic()
        .wrap_err("Maven returned an invalid project model")?;
    if model.schema_version != 1 || model.modules.is_empty() {
        bail!("Maven returned an unsupported or empty project model");
    }
    let mut modules = BTreeSet::new();
    for module in &model.modules {
        if !modules.insert(&module.path) {
            bail!(
                "Maven reactor module coordinates are ambiguous: {}",
                module.path
            );
        }
    }
    Ok(model)
}

pub fn command(root: &Path) -> Command {
    let wrapper = root.join(if cfg!(windows) { "mvnw.cmd" } else { "mvnw" });
    let maven = if wrapper.is_file() {
        wrapper
    } else {
        PathBuf::from(if cfg!(windows) { "mvn.cmd" } else { "mvn" })
    };
    let mut command = Command::new(maven);
    command.current_dir(root).kill_on_drop(true).args([
        "--batch-mode",
        "--no-transfer-progress",
        "-Dstyle.color=never",
    ]);
    command
}

pub async fn run(command: &mut Command) -> Result<Output> {
    let output = tokio::time::timeout(Duration::from_secs(300), command.output())
        .await
        .into_diagnostic()
        .wrap_err("Maven build timed out")?
        .into_diagnostic()
        .wrap_err("Could not start Maven. Make mvnw executable or install Maven on PATH")?;
    if !output.status.success() {
        let log = log(&output);
        let hint = annotation_processing_hint(build_java_major(), &log)
            .map(|hint| format!("\n{hint}"))
            .unwrap_or_default();
        bail!(
            "Maven build failed ({})\nMaven ran with {}{hint}\n{log}",
            output.status,
            java_home_note()
        );
    }
    Ok(output)
}

/// JDK 23 and later stop running annotation processors that are only found on
/// the classpath, so Lombok-style generated members go missing.
fn annotation_processing_hint(major: Option<u32>, log: &str) -> Option<&'static str> {
    (major? >= 23 && log.contains("cannot find symbol")).then_some(
        "Hint: JDK 23 and later do not run annotation processors (such as Lombok) unless the \
         build enables them, so generated members can be missing. Build with an older JDK, or \
         set <proc>full</proc> or annotationProcessorPaths on maven-compiler-plugin.",
    )
}

/// Major version of the JDK Maven runs with, from `JAVA_HOME/release` or `java -version`.
fn build_java_major() -> Option<u32> {
    let version = std::env::var_os("JAVA_HOME")
        .filter(|java| !java.is_empty())
        .and_then(|java| fs::read_to_string(PathBuf::from(java).join("release")).ok())
        .and_then(|release| {
            release
                .lines()
                .find_map(|line| line.strip_prefix("JAVA_VERSION="))
                .map(|value| value.trim_matches('"').to_owned())
        })
        .or_else(|| {
            let output = std::process::Command::new("java")
                .arg("-version")
                .output()
                .ok()?;
            let text = String::from_utf8_lossy(&output.stderr).into_owned();
            let start = text.find('"')? + 1;
            Some(text[start..].split('"').next()?.to_owned())
        })?;
    version.split(['.', '-', '+']).next()?.parse().ok()
}

fn java_home_note() -> String {
    std::env::var_os("JAVA_HOME")
        .filter(|java| !java.is_empty())
        .map_or_else(
            || {
                "JAVA_HOME unset (java from PATH); the build JDK may differ from the project's"
                    .to_string()
            },
            |java| format!("JAVA_HOME={}", PathBuf::from(java).display()),
        )
}

fn log(output: &Output) -> String {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    format!("{}\n{}", tail(&stdout), tail(&stderr))
}

fn tail(log: &str) -> String {
    let lines = log.lines().rev().take(35).collect::<Vec<_>>();
    lines.into_iter().rev().collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAILURE: &str = "[ERROR] ...: cannot find symbol\n  symbol: method getName()";

    #[test]
    fn hints_on_jdk_23_and_later_when_symbols_are_missing() {
        assert!(annotation_processing_hint(Some(25), FAILURE).is_some());
        assert!(annotation_processing_hint(Some(23), FAILURE).is_some());
    }

    #[test]
    fn no_hint_on_older_jdks_or_unrelated_failures() {
        assert!(annotation_processing_hint(Some(21), FAILURE).is_none());
        assert!(annotation_processing_hint(Some(25), "[ERROR] BUILD FAILURE").is_none());
        assert!(annotation_processing_hint(None, FAILURE).is_none());
    }
}
