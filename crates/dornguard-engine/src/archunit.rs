use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use dornguard_config::Rule;
use dornguard_core::{Diagnostic, EngineName, Module, Outcome, fingerprint};
use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use crate::{display_path, fail, relative_path, run_command};

const ENGINE: EngineName = EngineName::Archunit;
const SCHEMA_VERSION: u32 = 5;

pub struct ArchModule<'a> {
    pub module: &'a Module,
    pub rules: Vec<&'a Rule>,
    pub files: Vec<PathBuf>,
    pub kotlin: bool,
    pub check_class_timestamps: bool,
}

pub async fn check_architecture(root: &Path, modules: &[ArchModule<'_>]) -> Outcome {
    let mut outcome = Outcome::default();
    if !modules.is_empty()
        && let Err(error) = run(root, modules, &mut outcome).await
    {
        fail(&mut outcome, ENGINE, format!("{error:#}"));
    }
    for failure in &mut outcome.failures {
        let affected = modules
            .iter()
            .filter(|candidate| {
                failure.modules.is_empty() || failure.modules.contains(&candidate.module.path)
            })
            .collect::<Vec<_>>();
        failure.modules = affected
            .iter()
            .map(|candidate| candidate.module.path.clone())
            .collect();
        failure.modules.sort();
        failure.set_rules(
            affected
                .iter()
                .flat_map(|candidate| candidate.rules.iter().map(|rule| rule.id.as_str())),
        );
    }
    outcome.record_coverage(modules.iter().flat_map(|candidate| &candidate.files));
    outcome
}

pub async fn inspect_archunit(root: &Path) -> Result<String> {
    let java = crate::inspect_java(root).await?;
    let runner = runner_path()?;
    let mut command = crate::install::java_command(root);
    command.arg("-jar").arg(&runner).arg("--version");
    let output = run_command(
        ENGINE,
        &mut command,
        Instant::now() + Duration::from_secs(15),
    )
    .await?;
    let protocol = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() || protocol.trim().parse::<u32>().ok() != Some(SCHEMA_VERSION) {
        bail!(
            "ArchUnit runner {} did not report protocol {SCHEMA_VERSION}; install the matching runner",
            runner.display()
        );
    }
    Ok(format!("protocol {SCHEMA_VERSION}, {java}"))
}

async fn run(root: &Path, modules: &[ArchModule<'_>], outcome: &mut Outcome) -> Result<()> {
    let temp = tempfile::tempdir().into_diagnostic()?;
    let request_path = temp.path().join("request.json");
    let result_path = temp.path().join("result.json");
    let mut request = Request {
        schema_version: SCHEMA_VERSION,
        modules: Vec::new(),
    };

    for candidate in modules {
        let module = candidate.module;
        let mut rules = Vec::new();
        for rule in &candidate.rules {
            let options = rule
                .options
                .as_ref()
                .ok_or_else(|| miette!("Missing ArchUnit options for {}", rule.id))?;
            rules.push(RuleInput {
                id: &rule.id,
                origins: &options.from,
                forbidden: &options.forbidden,
                cycles: options.cycles,
            });
        }
        request.modules.push(ModuleInput {
            path: &module.path,
            source_files: candidate.files.iter().map(|file| root.join(file)).collect(),
            kotlin: candidate.kotlin,
            check_class_timestamps: candidate.check_class_timestamps,
            class_dirs: &module.class_dirs,
            classpath: &module.classpath,
            rules,
        });
    }

    fs::write(
        &request_path,
        serde_json::to_vec(&request).into_diagnostic()?,
    )
    .into_diagnostic()?;
    crate::inspect_java(root).await?;
    let runner = runner_path()?;
    let mut command = crate::install::java_command(root);
    command
        .arg("-jar")
        .arg(runner)
        .arg("--config")
        .arg(request_path)
        .arg("--output")
        .arg(&result_path);
    let output = run_command(
        ENGINE,
        &mut command,
        Instant::now() + Duration::from_secs(120),
    )
    .await?;
    if !output.status.success() {
        bail!("ArchUnit runner failed ({})", output.status);
    }
    let contents = fs::read(result_path)
        .into_diagnostic()
        .wrap_err("ArchUnit did not return a result")?;
    let result: Report = serde_json::from_slice(&contents)
        .into_diagnostic()
        .wrap_err("Invalid ArchUnit result")?;
    if result.schema_version != SCHEMA_VERSION {
        bail!(
            "Unsupported ArchUnit result schema {}",
            result.schema_version
        );
    }
    let mut failed = BTreeSet::new();
    for error in result.failures {
        fail(
            outcome,
            ENGINE,
            format!(
                "ArchUnit could not analyze {} ({})",
                error.module, error.reason
            ),
        );
        if let Some(failure) = outcome.failures.last_mut() {
            failure.path = error
                .path
                .and_then(|path| relative_path(root, &path))
                .as_deref()
                .map(display_path);
            failure.modules.push(error.module.clone());
        }
        failed.insert(error.module);
    }
    let analyzed = result
        .analyzed
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    for candidate in modules {
        if failed.contains(&candidate.module.path) {
            continue;
        }
        if !analyzed.contains(candidate.module.path.as_str()) {
            fail(
                outcome,
                ENGINE,
                format!("ArchUnit skipped module {}", candidate.module.path),
            );
            if let Some(failure) = outcome.failures.last_mut() {
                failure.modules.push(candidate.module.path.clone());
            }
        } else {
            outcome
                .scanned_files
                .extend(candidate.files.iter().cloned());
        }
    }

    let scopes: BTreeMap<_, _> = modules
        .iter()
        .map(|candidate| (candidate.module.path.as_str(), candidate))
        .collect();
    let mut relationships = BTreeMap::new();
    for finding in result.findings {
        let Some(candidate) = scopes.get(finding.module.as_str()) else {
            fail(
                outcome,
                ENGINE,
                "ArchUnit returned a diagnostic for an unrequested module",
            );
            continue;
        };
        let Some(rule) = candidate
            .rules
            .iter()
            .find(|rule| rule.id == finding.rule_id)
        else {
            fail(
                outcome,
                ENGINE,
                "ArchUnit returned a diagnostic for an unrequested architecture rule",
            );
            continue;
        };
        let path = match &finding.path {
            Some(path) => {
                let Some(path) =
                    relative_path(root, path).filter(|path| candidate.files.contains(path))
                else {
                    fail(
                        outcome,
                        ENGINE,
                        "ArchUnit returned a diagnostic outside the requested source files",
                    );
                    continue;
                };
                Some(display_path(&path))
            }
            None => None,
        };
        let line = finding.line.filter(|_| path.is_some());
        let legacy_fingerprint = if path.is_some() {
            fingerprint(&finding.identity)
        } else {
            fingerprint(&format!(
                "{}\0{}\0{}\0{}",
                finding.module, finding.origin, rule.id, finding.identity
            ))
        };
        let key = (
            finding.module.clone(),
            rule.id.clone(),
            finding.origin.clone(),
            finding.target.clone(),
        );
        let relationship_fingerprint =
            fingerprint(&format!("{}\0{}\0{}\0{}", key.0, key.1, key.2, key.3));
        let diagnostic = Diagnostic {
            path,
            module: Some(finding.module.clone()),
            class_name: Some(finding.origin.clone()),
            line,
            column: None,
            end_line: None,
            end_column: None,
            subject: None,
            rule_id: rule.id.clone(),
            engine: ENGINE,
            severity: rule.severity,
            message: format!(
                "{} {} depends on {} in {}.",
                rule.description, finding.origin, finding.target, finding.module
            ),
            fix: rule.fix.clone(),
            edit: None,
            legacy_fingerprints: Vec::new(),
            fingerprint: Some(relationship_fingerprint),
        };
        use std::collections::btree_map::Entry;
        match relationships.entry(key) {
            Entry::Vacant(entry) => {
                let mut diagnostic = diagnostic;
                diagnostic.legacy_fingerprints.push(legacy_fingerprint);
                entry.insert(diagnostic);
            }
            Entry::Occupied(mut entry) => {
                let representative = entry.get_mut();
                if (
                    diagnostic.path.is_none(),
                    diagnostic.line.is_none(),
                    diagnostic.line,
                ) < (
                    representative.path.is_none(),
                    representative.line.is_none(),
                    representative.line,
                ) {
                    let legacy = std::mem::take(&mut representative.legacy_fingerprints);
                    *representative = diagnostic;
                    representative.legacy_fingerprints = legacy;
                }
                representative.legacy_fingerprints.push(legacy_fingerprint);
            }
        }
    }
    outcome.diagnostics.extend(relationships.into_values());
    Ok(())
}

fn runner_path() -> Result<PathBuf> {
    let path = match std::env::var_os("DORNGUARD_ARCHUNIT_RUNNER") {
        Some(path) => PathBuf::from(path),
        None => std::env::current_exe()
            .into_diagnostic()?
            .with_file_name("archunit-runner.jar"),
    };
    let path = dunce::canonicalize(&path).into_diagnostic().wrap_err_with(|| {
        format!(
            "Could not find the ArchUnit runner at {}; set DORNGUARD_ARCHUNIT_RUNNER to its JAR",
            path.display()
        )
    })?;
    if let Some(checksum) = option_env!("DORNGUARD_ARCHUNIT_SHA256") {
        crate::install::verify_checksum(&path, checksum)
            .wrap_err("ArchUnit runner verification failed")?;
    }
    Ok(path)
}

pub fn maven_model_path() -> Result<PathBuf> {
    let path = match std::env::var_os("DORNGUARD_ARCHUNIT_RUNNER") {
        Some(runner) => PathBuf::from(runner).with_file_name("maven-model.jar"),
        None => std::env::current_exe()
            .into_diagnostic()?
            .with_file_name("maven-model.jar"),
    };
    let path = dunce::canonicalize(&path).into_diagnostic().wrap_err_with(|| {
        format!(
            "Could not find the Maven extension at {}; use a matching release bundle or build the runner",
            path.display()
        )
    })?;
    if let Some(checksum) = option_env!("DORNGUARD_MAVEN_MODEL_SHA256") {
        crate::install::verify_checksum(&path, checksum)
            .wrap_err("Maven extension verification failed")?;
    }
    Ok(path)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Request<'a> {
    schema_version: u32,
    modules: Vec<ModuleInput<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModuleInput<'a> {
    path: &'a str,
    source_files: Vec<PathBuf>,
    kotlin: bool,
    check_class_timestamps: bool,
    class_dirs: &'a [PathBuf],
    classpath: &'a [PathBuf],
    rules: Vec<RuleInput<'a>>,
}

#[derive(Serialize)]
struct RuleInput<'a> {
    id: &'a str,
    origins: &'a [String],
    forbidden: &'a [String],
    cycles: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    schema_version: u32,
    analyzed: Vec<String>,
    findings: Vec<Finding>,
    failures: Vec<RunnerFailure>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Finding {
    module: String,
    rule_id: String,
    path: Option<PathBuf>,
    line: Option<u32>,
    origin: String,
    target: String,
    identity: String,
}

#[derive(Deserialize)]
struct RunnerFailure {
    module: String,
    path: Option<PathBuf>,
    reason: String,
}
