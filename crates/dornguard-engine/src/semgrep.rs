use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Duration;

use crate::java_names::JavaNames;
use crate::source::Sources;
use crate::thymeleaf::SanitizedOutput;
use dornguard_config::Rule;
use dornguard_core::{Diagnostic, EngineName, Outcome, SourceLanguage};
use miette::{IntoDiagnostic, Result, WrapErr};
use serde::Deserialize;
use tokio::time::Instant;

use crate::{
    Engine, Request, check_version, command, display_path, engine_failure, fail, relative_path,
    run_command,
};

pub struct Semgrep {
    timeout: Duration,
    thymeleaf_sanitizers: Vec<String>,
    warn_on_thymeleaf_partial: bool,
}

impl Default for Semgrep {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(120),
            thymeleaf_sanitizers: Vec::new(),
            warn_on_thymeleaf_partial: false,
        }
    }
}

impl Semgrep {
    pub fn with_thymeleaf(sanitizers: &[String], warn_on_thymeleaf_partial: bool) -> Self {
        Self {
            thymeleaf_sanitizers: sanitizers.to_vec(),
            warn_on_thymeleaf_partial,
            ..Self::default()
        }
    }

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

        let mut sanitized = SanitizedOutput::new(&self.thymeleaf_sanitizers);
        let warn_on_thymeleaf_partial = self.warn_on_thymeleaf_partial
            && !request.required
            && request
                .rules
                .iter()
                .all(|rule| rule.id.starts_with("thymeleaf."));
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
            let first_failure = outcome.failures.len();
            let first_warning = outcome.analysis_warnings.len();
            normalize(
                &output,
                request.root,
                batch,
                &rules,
                &mut sanitized,
                warn_on_thymeleaf_partial,
                outcome,
            );
            for failure in outcome.failures[first_failure..]
                .iter_mut()
                .chain(&mut outcome.analysis_warnings[first_warning..])
            {
                failure.file_count = Some(batch.len());
            }
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
        request.mark_incomplete(&mut outcome);
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
    errors: Vec<ScanError>,
    skipped_rules: Vec<serde_json::Value>,
}
#[derive(Deserialize)]
struct ScanError {
    #[serde(default, rename = "type")]
    kind: serde_json::Value,
    path: Option<PathBuf>,
    rule_id: Option<String>,
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
    sanitized: &mut SanitizedOutput<'_>,
    warn_on_thymeleaf_partial: bool,
    outcome: &mut Outcome,
) {
    let Ok(scan): std::result::Result<Scan, _> = serde_json::from_slice(&output.stdout) else {
        fail(
            outcome,
            EngineName::Semgrep,
            format!(
                "Semgrep returned missing or invalid JSON ({})",
                output.status
            ),
        );
        return;
    };
    let expected: BTreeSet<&Path> = batch.iter().map(PathBuf::as_path).collect();
    let mut failed_files = BTreeSet::new();
    let mut warning_files = BTreeSet::new();
    let mut batch_failed = !output.status.success() && scan.errors.is_empty();
    if batch_failed {
        fail(
            outcome,
            EngineName::Semgrep,
            format!("Semgrep scan failed ({})", output.status),
        );
    }
    for error in scan.errors {
        let kind = error
            .kind
            .as_str()
            .or_else(|| error.kind.as_array()?.first()?.as_str())
            .unwrap_or("AnalysisError");
        let path = error
            .path
            .and_then(|path| relative_path(root, &path))
            .filter(|path| expected.contains(path.as_path()));
        let message = if kind == "PartialParsing" {
            "Semgrep parsed only part of the file. Findings for this file are incomplete."
        } else {
            "Semgrep could not complete analysis"
        };
        let mut failure = engine_failure(EngineName::Semgrep, message);
        failure.error_type = Some(kind.to_owned());
        if let Some(rule) = error.rule_id.as_deref().and_then(|id| rules.get(id)) {
            failure.set_rules([rule.id.as_str()]);
        }
        if let Some(path) = path {
            failure.path = Some(display_path(&path));
            if warn_on_thymeleaf_partial
                && kind == "PartialParsing"
                && SourceLanguage::of(&path) == Some(SourceLanguage::Template)
            {
                warning_files.insert(path.clone());
                failed_files.insert(path);
                outcome.analysis_warnings.push(failure);
                continue;
            }
            failed_files.insert(path);
        } else {
            batch_failed = true;
        }
        outcome.failures.push(failure);
    }
    if !scan.skipped_rules.is_empty() {
        batch_failed = true;
        fail(
            outcome,
            EngineName::Semgrep,
            format!(
                "Semgrep skipped {} required rules",
                scan.skipped_rules.len()
            ),
        );
    }

    let scanned: BTreeSet<PathBuf> = scan
        .paths
        .scanned
        .iter()
        .filter_map(|path| relative_path(root, path))
        .filter(|path| expected.contains(path.as_path()))
        .collect();
    let mut missing = expected
        .iter()
        .filter(|path| !scanned.contains(**path) && !warning_files.contains(**path));
    if let Some(first) = missing.next() {
        fail(
            outcome,
            EngineName::Semgrep,
            format!(
                "Semgrep skipped {} required files (first: {})",
                missing.count() + 1,
                first.display()
            ),
        );
    }

    let first_finding_failure = outcome.failures.len();
    let mut sources = Sources::default();
    let mut java = JavaNames::default();
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
        let source = sources.get(root, &path);
        if rule.id == "thymeleaf.unescaped-output"
            && !batch_failed
            && !failed_files.contains(&path)
            && source.is_some_and(|source| {
                sanitized.contains(
                    &path,
                    source.text(),
                    finding.start.offset..finding.end.offset,
                )
            })
        {
            continue;
        }
        if rule.id == "java.fully-qualified-type"
            && source.is_some_and(|source| {
                java.starts_with_value(
                    &path,
                    source.text(),
                    finding.start.offset..finding.end.offset,
                )
            })
        {
            continue;
        }
        let start = source
            .and_then(|source| source.position(finding.start.offset))
            .filter(|(line, _)| *line == finding.start.line);
        let end = source
            .and_then(|source| source.position(finding.end.offset))
            .filter(|end| start.is_some_and(|start| start < *end) && end.0 == finding.end.line);
        let subject = (end.is_some()
            && matches!(
                rule.id.as_str(),
                "java.fully-qualified-type" | "kotlin.fully-qualified-type"
            ))
        .then(|| {
            source?
                .fragment(finding.start.offset, finding.end.offset)
                .map(|name| name.split_whitespace().collect::<String>())
        })
        .flatten();
        outcome.diagnostics.push(Diagnostic {
            path: Some(display_path(&path)),
            module: None,
            class_name: None,
            line: Some(finding.start.line),
            column: start.map(|(_, column)| column),
            end_line: end.map(|(line, _)| line),
            end_column: end.map(|(_, column)| column),
            message: crate::diagnostic_message(&rule.description, subject.as_deref()),
            subject,
            rule_id: rule.id.clone(),
            engine: rule.engine,
            severity: rule.severity,
            fix: rule.fix.clone(),
            edit: None,
            legacy_fingerprints: Vec::new(),
            fingerprint: sources.offsets(root, &path, finding.start.offset, finding.end.offset),
        });
    }
    if !batch_failed && first_finding_failure == outcome.failures.len() {
        outcome
            .scanned_files
            .extend(scanned.difference(&failed_files).cloned());
    }
}
