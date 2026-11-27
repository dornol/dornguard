use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::time::Duration;

use crate::java_names::JavaNames;
use crate::source::Sources;
use crate::thymeleaf::SanitizedOutput;
use dornguard_config::{PartialParsingPolicy, Rule};
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
    partial_policy: PartialParsingPolicy,
}

impl Default for Semgrep {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(120),
            thymeleaf_sanitizers: Vec::new(),
            warn_on_thymeleaf_partial: false,
            partial_policy: PartialParsingPolicy::default(),
        }
    }
}

impl Semgrep {
    /// Report partial parsing of the files this policy covers as incomplete analysis
    /// instead of a failure.
    #[must_use]
    pub fn with_partial_parsing(mut self, policy: PartialParsingPolicy) -> Self {
        self.partial_policy = policy;
        self
    }

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
        let partial_policy = if request.required {
            PartialParsingPolicy::default()
        } else {
            self.partial_policy.clone()
        };
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
                &Tolerance {
                    thymeleaf: warn_on_thymeleaf_partial,
                    policy: &partial_policy,
                },
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

fn literal_markup_hint(root: &Path, path: &Path, line: u64, column: u64) -> Option<String> {
    let text = std::fs::read_to_string(root.join(path)).ok()?;
    let line_index = usize::try_from(line.checked_sub(1)?).ok()?;
    let column_index = usize::try_from(column.checked_sub(1)?).ok()?;
    let source_line = text.lines().nth(line_index)?;
    // Semgrep reports columns as UTF-8 byte offsets, not characters.
    let rest = source_line.get(column_index..)?;
    match rest.chars().next()? {
        '>' => Some("A literal `>` in text should be written as `&gt;`.".to_owned()),
        '&' => Some("A literal `&` in text should be written as `&amp;`.".to_owned()),
        '<' => duplicate_closing_tag(&text, line_index, rest),
        _ => None,
    }
}

/// Names a closing tag that closes an element already closed earlier.
fn duplicate_closing_tag(text: &str, line_index: usize, rest: &str) -> Option<String> {
    let name = rest.strip_prefix("</")?.split('>').next()?.trim();
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    let closing = format!("</{name}>");
    let before: Vec<&str> = text.lines().take(line_index).collect();
    let earlier = before.iter().any(|line| line.contains(&closing));
    let opens = before
        .iter()
        .map(|line| line.matches(&format!("<{name}")).count())
        .sum::<usize>();
    let closes = before
        .iter()
        .map(|line| line.matches(&closing).count())
        .sum::<usize>();
    (earlier && closes >= opens).then(|| {
        format!(
            "`{closing}` closes an element that is already closed. Remove the extra closing tag."
        )
    })
}

/// Describe a Semgrep analysis error, locating where partial parsing stopped.
fn analysis_error_message(
    root: &Path,
    kind: &str,
    detail: &serde_json::Value,
    path: Option<&PathBuf>,
    template: bool,
) -> String {
    let mut message = if kind == "PartialParsing" {
        "Semgrep parsed only part of the file. Findings for this file are incomplete.".to_owned()
    } else {
        "Semgrep could not complete analysis".to_owned()
    };
    if kind == "PartialParsing" {
        let position = detail
            .as_array()
            .and_then(|parts| parts.get(1)?.as_array()?.first()?.get("start"))
            .and_then(|start| Some((start.get("line")?.as_u64()?, start.get("col")?.as_u64()?)));
        if let Some((line, column)) = position {
            message.push_str(&format!(
                " Parsing stopped at line {line}, column {column}."
            ));
            if let Some(path) = path
                && let Some(hint) = literal_markup_hint(root, path, line, column)
            {
                message.push(' ');
                message.push_str(&hint);
            }
        }
        if template {
            message.push_str(" Set thymeleaf_partial_parsing: warning to report this as incomplete analysis without failing the check.");
        } else {
            message.push_str(" If the file is valid and the parser is wrong, list it in partial_parsing_files with a reason, or set partial_parsing: warning.");
        }
    }
    message
}

/// Which partially parsed files are reported as warnings rather than failures.
struct Tolerance<'a> {
    thymeleaf: bool,
    policy: &'a PartialParsingPolicy,
}

#[derive(Default)]
struct ScanState {
    failed_files: BTreeSet<PathBuf>,
    warning_files: BTreeSet<PathBuf>,
    batch_failed: bool,
}

fn record_scan_errors(
    errors: Vec<ScanError>,
    root: &Path,
    expected: &BTreeSet<&Path>,
    rules: &BTreeMap<&str, &Rule>,
    tolerance: &Tolerance<'_>,
    state: &mut ScanState,
    outcome: &mut Outcome,
) {
    for error in errors {
        let kind = error
            .kind
            .as_str()
            .or_else(|| error.kind.as_array()?.first()?.as_str())
            .unwrap_or("AnalysisError");
        let path = error
            .path
            .and_then(|path| relative_path(root, &path))
            .filter(|path| expected.contains(path.as_path()));
        let template = !tolerance.thymeleaf
            && path
                .as_ref()
                .is_some_and(|path| SourceLanguage::of(path) == Some(SourceLanguage::Template));
        let message = analysis_error_message(root, kind, &error.kind, path.as_ref(), template);
        let mut failure = engine_failure(EngineName::Semgrep, message);
        failure.error_type = Some(kind.to_owned());
        if let Some(rule) = error.rule_id.as_deref().and_then(|id| rules.get(id)) {
            failure.set_rules([rule.id.as_str()]);
        }
        if let Some(path) = path {
            failure.path = Some(display_path(&path));
            if kind == "PartialParsing"
                && (tolerance.policy.tolerates(&path)
                    || (tolerance.thymeleaf
                        && SourceLanguage::of(&path) == Some(SourceLanguage::Template)))
            {
                state.warning_files.insert(path.clone());
                state.failed_files.insert(path);
                outcome.analysis_warnings.push(failure);
                continue;
            }
            state.failed_files.insert(path);
        } else {
            state.batch_failed = true;
        }
        outcome.failures.push(failure);
    }
}

fn record_findings(
    results: Vec<Finding>,
    root: &Path,
    expected: &BTreeSet<&Path>,
    rules: &BTreeMap<&str, &Rule>,
    sanitized: &mut SanitizedOutput<'_>,
    state: &ScanState,
    outcome: &mut Outcome,
) {
    let mut sources = Sources::default();
    let mut java = JavaNames::default();
    for finding in results {
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
            && !state.batch_failed
            && !state.failed_files.contains(&path)
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
            confidence: rule.confidence,
            fix: rule.fix.clone(),
            edit: None,
            legacy_fingerprints: Vec::new(),
            group: None,
            fingerprint: sources.offsets(root, &path, finding.start.offset, finding.end.offset),
        });
    }
}

fn normalize(
    output: &Output,
    root: &Path,
    batch: &[PathBuf],
    rules: &BTreeMap<&str, &Rule>,
    sanitized: &mut SanitizedOutput<'_>,
    tolerance: &Tolerance<'_>,
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
    let mut state = ScanState {
        batch_failed: !output.status.success() && scan.errors.is_empty(),
        ..ScanState::default()
    };
    if state.batch_failed {
        fail(
            outcome,
            EngineName::Semgrep,
            format!("Semgrep scan failed ({})", output.status),
        );
    }
    record_scan_errors(
        scan.errors,
        root,
        &expected,
        rules,
        tolerance,
        &mut state,
        outcome,
    );
    if !scan.skipped_rules.is_empty() {
        state.batch_failed = true;
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
        .filter(|path| !scanned.contains(**path) && !state.warning_files.contains(**path));
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
    record_findings(
        scan.results,
        root,
        &expected,
        rules,
        sanitized,
        &state,
        outcome,
    );
    if !state.batch_failed && first_finding_failure == outcome.failures.len() {
        outcome
            .scanned_files
            .extend(scanned.difference(&state.failed_files).cloned());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hint_for(source: &str, line: u64, column: u64) -> std::io::Result<Option<String>> {
        let directory = tempfile::tempdir()?;
        std::fs::write(directory.path().join("page.html"), source)?;
        Ok(literal_markup_hint(
            directory.path(),
            Path::new("page.html"),
            line,
            column,
        ))
    }

    #[test]
    fn hint_uses_byte_columns_after_multibyte_text() -> std::io::Result<()> {
        // Semgrep columns are 1-based UTF-8 byte offsets: the `>` is at byte 12.
        let hint = hint_for("<title>가 > 나</title>\n", 1, 12)?;
        assert_eq!(
            hint.as_deref(),
            Some("A literal `>` in text should be written as `&gt;`.")
        );
        Ok(())
    }

    #[test]
    fn hint_for_ampersand_after_korean_text() -> std::io::Result<()> {
        let hint = hint_for("<th>대출일 & 예약일</th>\n", 1, 15)?;
        assert_eq!(
            hint.as_deref(),
            Some("A literal `&` in text should be written as `&amp;`.")
        );
        Ok(())
    }

    #[test]
    fn column_inside_a_character_gives_no_hint() -> std::io::Result<()> {
        assert_eq!(hint_for("<p>가</p>\n", 1, 5)?, None);
        Ok(())
    }
}
