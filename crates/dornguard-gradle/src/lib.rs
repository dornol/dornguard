use std::path::{Path, PathBuf};
use std::time::Duration;

use miette::{IntoDiagnostic, Result, WrapErr, bail};
use serde::{Deserialize, Serialize};
use tokio::process::Command;

const MODEL_SCRIPT: &str = include_str!("model.gradle");

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectModel {
    schema_version: u32,
    pub modules: Vec<Module>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Module {
    pub path: String,
    pub name: String,
    pub project_dir: PathBuf,
    pub source_dirs: Vec<PathBuf>,
    pub kotlin_source_dirs: Vec<PathBuf>,
    pub class_dirs: Vec<PathBuf>,
    pub classpath: Vec<PathBuf>,
    pub dependencies: Vec<String>,
}

pub enum CompileModules<'a> {
    Selected(&'a [String]),
    AllJava,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Request<'a> {
    output: &'a Path,
    compile_modules: &'a [String],
    compile_all: bool,
}

pub async fn collect(root: &Path, compile: CompileModules<'_>) -> Result<ProjectModel> {
    #[cfg(windows)]
    let wrapper = root.join("gradlew.bat");
    #[cfg(not(windows))]
    let wrapper = root.join("gradlew");
    if !wrapper.is_file() {
        bail!("Gradle Wrapper is missing at {}", wrapper.display());
    }
    let temp = tempfile::tempdir()
        .into_diagnostic()
        .wrap_err("Could not create temporary Gradle model directory")?;
    let script = temp.path().join("dornguard.gradle");
    std::fs::write(&script, MODEL_SCRIPT).into_diagnostic()?;
    let model_path = temp.path().join("model.json");
    let config_path = temp.path().join("request.json");
    let request = Request {
        output: &model_path,
        compile_modules: match compile {
            CompileModules::Selected(modules) => modules,
            CompileModules::AllJava => &[],
        },
        compile_all: matches!(compile, CompileModules::AllJava),
    };
    std::fs::write(
        &config_path,
        serde_json::to_vec(&request).into_diagnostic()?,
    )
    .into_diagnostic()?;
    let mut property = std::ffi::OsString::from("-PdornguardModelRequest=");
    property.push(config_path);

    let mut command = Command::new(wrapper);
    command
        .current_dir(root)
        .kill_on_drop(true)
        .args([
            "--no-daemon",
            "--no-configuration-cache",
            "--no-configure-on-demand",
            "--console=plain",
            "--quiet",
            "--init-script",
        ])
        .arg(script)
        .arg(property)
        .arg(":dornguardExportProjectModel");
    let output = tokio::time::timeout(Duration::from_secs(300), command.output())
        .await
        .into_diagnostic()
        .wrap_err("Gradle model export timed out")?
        .into_diagnostic()
        .wrap_err("Could not start Gradle Wrapper")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let first_lines = stderr.lines().take(35).collect::<Vec<_>>().join("\n");
        bail!(
            "Gradle model export failed ({})\n{first_lines}",
            output.status
        );
    }

    let data = std::fs::read(model_path)
        .into_diagnostic()
        .wrap_err("Gradle did not produce its project model")?;
    let model: ProjectModel = serde_json::from_slice(&data)
        .into_diagnostic()
        .wrap_err("Gradle returned an invalid project model")?;
    if model.schema_version != 1 {
        bail!(
            "Unsupported Gradle model schema version {}",
            model.schema_version
        );
    }
    Ok(model)
}
