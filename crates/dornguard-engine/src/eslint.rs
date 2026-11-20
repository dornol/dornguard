use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;

use dornguard_core::{Diagnostic, EngineName, Outcome};
use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};
use serde::Deserialize;
use serde_json::json;
use tokio::time::Instant;

use crate::semgrep::budgeted_batch;
use crate::source::Sources;
use crate::{
    Engine, Request, check_version, command, display_path, engine_failure, fail, relative_path,
    run_command,
};

const ENGINE: EngineName = EngineName::Eslint;

pub struct Eslint;

impl Engine for Eslint {
    async fn check(&self, request: Request<'_>) -> Outcome {
        let mut outcome = Outcome::default();
        if let Err(error) = scan(request, &mut outcome).await {
            fail(&mut outcome, ENGINE, format!("{error:#}"));
        }
        request.mark_incomplete(&mut outcome);
        outcome
    }
}

async fn scan(request: Request<'_>, outcome: &mut Outcome) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(120);
    check_version(ENGINE, request.root, deadline).await?;
    let temp = tempfile::tempdir().into_diagnostic()?;
    let config = temp.path().join("eslint.config.mjs");
    let rules = request
        .rules
        .iter()
        .map(|rule| Ok((rule.implementation()?.rule_id.as_str(), *rule)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    let enabled = rules
        .keys()
        .map(|id| (*id, "error"))
        .collect::<BTreeMap<_, _>>();
    let timer_globals = [
        "setTimeout",
        "setInterval",
        "execScript",
        "window",
        "self",
        "globalThis",
        "global",
    ]
    .into_iter()
    .map(|name| (name, "readonly"))
    .collect::<BTreeMap<_, _>>();
    let contents = json!([{
        "languageOptions": { "globals": timer_globals },
        "rules": enabled,
    }]);
    std::fs::write(&config, format!("export default {contents};\n"))
        .into_diagnostic()
        .wrap_err("Could not write ESLint configuration")?;
    let mut sources = Sources::default();

    for batch in request.files.chunk_by(budgeted_batch()) {
        let mut lint = command(ENGINE, request.root)?;
        lint.args([
            "--no-config-lookup",
            "--no-inline-config",
            "--no-ignore",
            "--format=json",
            "--config",
        ])
        .arg(&config)
        .arg("--")
        .args(
            batch
                .iter()
                .map(|file| std::path::Path::new(".").join(file)),
        );
        let output = run_command(ENGINE, &mut lint, deadline).await?;
        if !matches!(output.status.code(), Some(0 | 1)) {
            bail!(
                "ESLint failed ({})\n{}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let reports: Vec<FileReport> = serde_json::from_slice(&output.stdout)
            .into_diagnostic()
            .wrap_err("ESLint returned missing or invalid JSON")?;
        let expected = batch.iter().collect::<BTreeSet<_>>();
        for report in reports {
            let path = relative_path(request.root, &report.file_path)
                .filter(|path| expected.contains(path))
                .ok_or_else(|| miette!("ESLint returned a file outside the requested sources"))?;
            let mut parsed = true;
            for finding in report.messages {
                if finding.fatal {
                    let mut failure = engine_failure(ENGINE, finding.message);
                    failure.path = Some(display_path(&path));
                    failure.error_type = Some("FatalError".to_owned());
                    failure.file_count = Some(batch.len());
                    outcome.failures.push(failure);
                    parsed = false;
                    continue;
                }
                let rule = finding
                    .rule_id
                    .as_deref()
                    .and_then(|id| rules.get(id))
                    .ok_or_else(|| {
                        miette!(
                            "ESLint returned an unregistered rule or ignored file: {}",
                            display_path(&path)
                        )
                    })?;
                let line = finding.line.unwrap_or(0);
                let column = finding.column.unwrap_or(0);
                if line == 0 || column == 0 {
                    bail!("ESLint returned an invalid diagnostic position");
                }
                let source = sources.get(request.root, &path);
                let start = source.and_then(|source| source.offset((line, column)));
                let end = finding.end_line.zip(finding.end_column).filter(|end| {
                    source
                        .and_then(|source| source.offset(*end))
                        .zip(start)
                        .is_some_and(|(end, start)| start < end)
                });
                outcome.diagnostics.push(Diagnostic {
                    path: Some(display_path(&path)),
                    module: None,
                    class_name: None,
                    line: Some(line),
                    column: start.map(|_| column),
                    end_line: end.map(|(line, _)| line),
                    end_column: end.map(|(_, column)| column),
                    subject: None,
                    rule_id: rule.id.clone(),
                    engine: ENGINE,
                    severity: rule.severity,
                    message: rule.description.clone(),
                    fix: rule.fix.clone(),
                    edit: None,
                    legacy_fingerprints: Vec::new(),
                    fingerprint: finding
                        .end_line
                        .zip(finding.end_column)
                        .and_then(|end| sources.span(request.root, &path, (line, column), end)),
                });
            }
            if parsed {
                outcome.scanned_files.insert(path);
            }
        }
        if batch
            .iter()
            .any(|path| !outcome.scanned_files.contains(path))
            && outcome.failures.is_empty()
        {
            bail!("ESLint skipped requested source files");
        }
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileReport {
    file_path: PathBuf,
    messages: Vec<Finding>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Finding {
    rule_id: Option<String>,
    #[serde(default)]
    fatal: bool,
    message: String,
    line: Option<u32>,
    column: Option<u32>,
    end_line: Option<u32>,
    end_column: Option<u32>,
}
