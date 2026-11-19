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
        bail!("Maven build failed ({})\n{}", output.status, log(&output));
    }
    Ok(output)
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
