use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Duration;

use crate::source::Sources;
use dornguard_config::Rule;
use dornguard_core::{Diagnostic, EngineName, Outcome};
use miette::{IntoDiagnostic, Result, WrapErr};
use serde::Deserialize;
use tokio::time::Instant;

use crate::{
    Engine, Request, check_version, command, display_path, fail, relative_path, run_command,
};

pub struct Semgrep {
    timeout: Duration,
}

impl Default for Semgrep {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(120),
        }
    }
}

impl Semgrep {
    async fn run(&self, request: Request<'_>, outcome: &mut Outcome) -> Result<()> {
        let configs = tempfile::tempdir()
            .into_diagnostic()
            .wrap_err("Could not create Semgrep configuration directory")?;
        let mut rules = BTreeMap::new();
        for (index, rule) in request.rules.iter().enumerate() {
            rules.insert(rule.implementation()?.rule_id.as_str(), *rule);
            let config = request.registry.engine_config(rule)?;
            std::fs::write(configs.path().join(format!("{index}.yml")), config)
                .into_diagnostic()
                .wrap_err("Could not write Semgrep configuration")?;
        }

        let deadline = Instant::now() + self.timeout;
        check_version(EngineName::Semgrep, request.root, deadline).await?;

        for batch in request.files.chunk_by(budgeted_batch()) {
            let mut scan = command(EngineName::Semgrep, request.root)?;
            scan.args([
                "scan",
                "--json",
                "--metrics=off",
                "--no-rewrite-rule-ids",
                "--disable-version-check",
                "--no-git-ignore",
                "--disable-nosem",
                "--oss-only",
                "--no-error",
                "--strict",
                "--quiet",
                "--config",
            ])
            .arg(configs.path())
            .arg("--")
            .args(batch);
            let output = run_command(EngineName::Semgrep, &mut scan, deadline).await?;
            normalize(&output, request.root, batch, &rules, outcome);
        }
        Ok(())
    }
}

impl Engine for Semgrep {
    async fn check(&self, request: Request<'_>) -> Outcome {
        let mut outcome = Outcome::default();
        if let Err(error) = self.run(request, &mut outcome).await {
            fail(&mut outcome, EngineName::Semgrep, format!("{error:#}"));
        }
        outcome
    }
}

// Keep batches below Windows' command-line limit, including quoting and flags.
pub(super) fn budgeted_batch() -> impl FnMut(&PathBuf, &PathBuf) -> bool {
    fn cost(path: &OsStr) -> usize {
        path.len() * 2 + 4
    }
    let mut bytes = 0;
    move |previous, next| {
        if bytes == 0 {
            bytes = cost(previous.as_os_str());
        }
        let next_bytes = cost(next.as_os_str());
        if bytes + next_bytes > 16_000 {
            bytes = 0;
            return false;
        }
        bytes += next_bytes;
        true
    }
}

#[derive(Deserialize)]
struct Scan {
    paths: ScanPaths,
    results: Vec<Finding>,
    errors: Vec<serde_json::Value>,
    skipped_rules: Vec<serde_json::Value>,
}
#[derive(Deserialize)]
struct ScanPaths {
    scanned: Vec<PathBuf>,
}
#[derive(Deserialize)]
struct Finding {
    check_id: String,
    path: PathBuf,
    start: Location,
    end: Location,
}
#[derive(Deserialize)]
struct Location {
    line: u32,
    col: u32,
    offset: usize,
}

fn normalize(
    output: &Output,
    root: &Path,
    batch: &[PathBuf],
    rules: &BTreeMap<&str, &Rule>,
    outcome: &mut Outcome,
) {
    if !output.status.success() {
        fail(
            outcome,
            EngineName::Semgrep,
            format!("Semgrep scan failed ({})", output.status),
        );
    }
    let Ok(scan): std::result::Result<Scan, _> = serde_json::from_slice(&output.stdout) else {
        fail(
            outcome,
            EngineName::Semgrep,
            "Semgrep returned missing or invalid JSON",
        );
        return;
    };
    if !scan.errors.is_empty() {
        fail(
            outcome,
            EngineName::Semgrep,
            format!("Semgrep reported {} analysis errors", scan.errors.len()),
        );
    }
    if !scan.skipped_rules.is_empty() {
        fail(
            outcome,
            EngineName::Semgrep,
            format!(
                "Semgrep skipped {} required rules",
                scan.skipped_rules.len()
            ),
        );
    }

    let expected: BTreeSet<&Path> = batch.iter().map(PathBuf::as_path).collect();
    let scanned: BTreeSet<PathBuf> = scan
        .paths
        .scanned
        .iter()
        .filter_map(|path| relative_path(root, path))
        .filter(|path| expected.contains(path.as_path()))
        .collect();
    if scanned.len() != expected.len() {
        let missing = expected
            .iter()
            .find(|path| !scanned.contains(**path))
            .map_or_else(String::new, |path| path.display().to_string());
        fail(
            outcome,
            EngineName::Semgrep,
            format!(
                "Semgrep skipped {} required files (first: {missing})",
                expected.len() - scanned.len()
            ),
        );
    }

    outcome.scanned_files.extend(scanned);

    let mut sources = Sources::default();
    for finding in scan.results {
        let Some(rule) = rules.get(finding.check_id.as_str()) else {
            fail(
                outcome,
                EngineName::Semgrep,
                "Semgrep returned an unregistered rule ID",
            );
            continue;
        };
        let Some(path) =
            relative_path(root, &finding.path).filter(|path| expected.contains(path.as_path()))
        else {
            fail(
                outcome,
                EngineName::Semgrep,
                "Semgrep returned a diagnostic outside the requested files",
            );
            continue;
        };
        if finding.start.line == 0 || finding.start.col == 0 {
            fail(
                outcome,
                EngineName::Semgrep,
                "Semgrep returned an invalid diagnostic position",
            );
            continue;
        }
        if rule.excludes_path(&path) {
            continue;
        }
        outcome.diagnostics.push(Diagnostic {
            path: Some(display_path(&path)),
            module: None,
            class_name: None,
            line: Some(finding.start.line),
            column: Some(finding.start.col),
            rule_id: rule.id.clone(),
            engine: rule.engine,
            severity: rule.severity,
            message: rule.description.clone(),
            fix: rule.fix.clone(),
            fingerprint: sources.offsets(root, &path, finding.start.offset, finding.end.offset),
        });
    }
}
