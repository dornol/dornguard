//! `go vet` with its default analyzers, run once per Go module.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use dornguard_config::Rule;
use dornguard_core::{Diagnostic, EngineName, Outcome, fingerprint};
use miette::{Result, miette};
use serde::Deserialize;
use serde_json::Value;
use tokio::time::Instant;

use crate::source::Sources;
use crate::{
    Engine, Request, check_version, command, display_path, engine_failure, fail, nearest_manifest,
    relative_path, run_command,
};

const ENGINE: EngineName = EngineName::GoVet;

pub struct GoVet;

impl Engine for GoVet {
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
    let deadline = Instant::now() + Duration::from_secs(600);
    check_version(ENGINE, request.root, deadline).await?;
    let rule = *request
        .rules
        .first()
        .ok_or_else(|| miette!("go vet runs without a rule"))?;
    let mut modules = BTreeMap::<PathBuf, BTreeSet<&Path>>::new();
    for file in request.files {
        let module = nearest_manifest(request.root, file, "go.mod")
            .ok_or_else(|| miette!("No go.mod found for {}", display_path(file)))?;
        modules.entry(module).or_default().insert(file);
    }
    let mut sources = Sources::default();
    let mut seen = BTreeSet::new();
    for (module, files) in modules {
        let directory = request.root.join(&module);
        let packages = files
            .iter()
            .filter_map(|file| file.parent()?.strip_prefix(&module).ok())
            .map(|package| format!("./{}", display_path(package)))
            .map(|package| {
                if package == "./" {
                    ".".to_owned()
                } else {
                    package
                }
            })
            .collect::<BTreeSet<_>>();
        let mut vet = command(ENGINE, &directory)?;
        vet.args(["vet", "-json"]).args(&packages);
        let output = run_command(ENGINE, &mut vet, deadline).await?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Go 1.24 and later write the JSON report to stdout, earlier releases to stderr.
        let (reports, mut errors) = parse_reports(&stdout);
        let (more, stderr_errors) = parse_reports(&stderr);
        errors.extend(stderr_errors);
        let requested = files.iter().copied().collect::<BTreeSet<_>>();
        for report in reports.into_iter().chain(more) {
            record(
                request.root,
                &directory,
                rule,
                &requested,
                report,
                &mut sources,
                &mut seen,
                outcome,
            );
        }
        if output.status.success() && errors.is_empty() {
            outcome
                .scanned_files
                .extend(files.iter().map(|file| file.to_path_buf()));
        } else {
            let mut failure = engine_failure(
                ENGINE,
                format!(
                    "go vet could not check every package in {} ({})\n{}",
                    if module.as_os_str().is_empty() {
                        ".".to_owned()
                    } else {
                        display_path(&module)
                    },
                    output.status,
                    errors.join("\n").trim()
                ),
            );
            failure.file_count = Some(files.len());
            outcome.failures.push(failure);
        }
    }
    Ok(())
}

/// One analyzer finding from `go vet -json`.
#[derive(Deserialize)]
struct Finding {
    posn: String,
    #[serde(default)]
    end: Option<String>,
    message: String,
    #[serde(default)]
    suggested_fixes: Vec<SuggestedFix>,
}

#[derive(Deserialize)]
struct SuggestedFix {
    message: String,
}

struct Report {
    analyzer: String,
    finding: Finding,
}

/// Split `go vet -json` output into findings and the text of any build errors.
///
/// The output is a sequence of JSON objects, one per package, mapping the
/// package to analyzers and their findings. `# package` headers and compiler
/// errors appear as plain text between them.
fn parse_reports(text: &str) -> (Vec<Report>, Vec<String>) {
    let mut reports = Vec::new();
    let mut errors = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(|c: char| !c.is_whitespace()) {
        rest = &rest[start..];
        if rest.starts_with('{') {
            let mut stream = serde_json::Deserializer::from_str(rest).into_iter::<Value>();
            match stream.next() {
                Some(Ok(value)) => {
                    collect_packages(value, &mut reports, &mut errors);
                    rest = &rest[stream.byte_offset()..];
                    continue;
                }
                _ => {
                    errors.push(rest.trim().to_owned());
                    break;
                }
            }
        }
        let (line, next) = rest.split_once('\n').unwrap_or((rest, ""));
        if !line.starts_with("# ") {
            errors.push(line.trim_end().to_owned());
        }
        rest = next;
    }
    (reports, errors)
}

fn collect_packages(value: Value, reports: &mut Vec<Report>, errors: &mut Vec<String>) {
    let Value::Object(packages) = value else {
        errors.push("go vet returned an unexpected report".to_owned());
        return;
    };
    for (package, analyzers) in packages {
        let Value::Object(analyzers) = analyzers else {
            continue;
        };
        for (analyzer, findings) in analyzers {
            match findings {
                Value::Array(findings) => {
                    for finding in findings {
                        match serde_json::from_value::<Finding>(finding) {
                            Ok(finding) => reports.push(Report {
                                analyzer: analyzer.clone(),
                                finding,
                            }),
                            Err(error) => errors.push(format!(
                                "go vet returned an invalid {analyzer} finding: {error}"
                            )),
                        }
                    }
                }
                // {"error": "..."} when an analyzer could not run on the package.
                Value::Object(error) => errors.push(format!(
                    "{package}: {analyzer}: {}",
                    error
                        .get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("analysis failed")
                )),
                _ => {}
            }
        }
    }
}

/// `file:line:column`, where the file itself may contain colons.
fn position(posn: &str) -> Option<(&str, u32, u32)> {
    let mut parts = posn.rsplitn(3, ':');
    let column = parts.next()?.parse().ok()?;
    let line = parts.next()?.parse().ok()?;
    Some((parts.next()?, line, column))
}

#[allow(clippy::too_many_arguments)]
fn record(
    root: &Path,
    directory: &Path,
    rule: &Rule,
    requested: &BTreeSet<&Path>,
    report: Report,
    sources: &mut Sources,
    seen: &mut BTreeSet<(PathBuf, u32, u32, String, String)>,
    outcome: &mut Outcome,
) {
    let Report { analyzer, finding } = report;
    let Some((file, line, column)) = position(&finding.posn) else {
        return;
    };
    let absolute = directory.join(file);
    let Some(path) = relative_path(root, &absolute)
        .or_else(|| relative_path(root, &dunce::canonicalize(&absolute).ok()?))
        .filter(|path| requested.contains(path.as_path()))
    else {
        return;
    };
    // A package and its test variant report the same finding.
    if !seen.insert((
        path.clone(),
        line,
        column,
        analyzer.clone(),
        finding.message.clone(),
    )) {
        return;
    }
    let source = sources.get(root, &path);
    let start = source.and_then(|source| source.byte_column_offset((line, column)));
    let end = finding
        .end
        .as_deref()
        .and_then(position)
        .and_then(|(_, line, column)| source?.byte_column_offset((line, column)))
        .filter(|end| start.is_some_and(|start| start < *end));
    let start_position = start.and_then(|start| source?.position(start));
    let end_position = end.and_then(|end| source?.position(end));
    let fix = finding.suggested_fixes.first();
    let identity = start.and_then(|start| source?.range_fingerprint(start, end.unwrap_or(start)));
    outcome.diagnostics.push(Diagnostic {
        path: Some(display_path(&path)),
        module: None,
        class_name: None,
        line: Some(start_position.map_or(line, |(line, _)| line)),
        column: start_position.map(|(_, column)| column),
        end_line: end_position.map(|(line, _)| line),
        end_column: end_position.map(|(_, column)| column),
        subject: Some(analyzer.clone()),
        rule_id: rule.id.clone(),
        engine: ENGINE,
        severity: rule.severity,
        confidence: rule.confidence,
        message: format!("{} ({analyzer})", finding.message),
        fix: fix.map_or_else(|| rule.fix.clone(), |fix| format!("{}.", fix.message)),
        edit: None,
        legacy_fingerprints: Vec::new(),
        group: None,
        fingerprint: identity.map(|identity| fingerprint(&format!("{analyzer}\n{identity}"))),
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_are_read_between_package_headers_and_build_errors() {
        let text = "# example.com/m/bad\nbad/b.go:3:28: cannot use \"s\"\n{\n\t\"example.com/m/pkg\": {\n\t\t\"printf\": [\n\t\t\t{\"posn\": \"/m/pkg/a.go:6:14\", \"end\": \"/m/pkg/a.go:6:16\", \"message\": \"wrong type\"}\n\t\t]\n\t}\n}\n{}\n";
        let (reports, errors) = parse_reports(text);
        assert_eq!(errors, ["bad/b.go:3:28: cannot use \"s\""]);
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].analyzer, "printf");
        assert_eq!(reports[0].finding.message, "wrong type");
    }

    #[test]
    fn analyzer_errors_are_build_errors() {
        let (reports, errors) =
            parse_reports(r#"{"example.com/m": {"printf": {"error": "broken"}}}"#);
        assert!(reports.is_empty());
        assert_eq!(errors, ["example.com/m: printf: broken"]);
    }

    #[test]
    fn positions_keep_colons_in_file_names() {
        assert_eq!(
            position(r"C:\src\m\a.go:6:14"),
            Some((r"C:\src\m\a.go", 6, 14))
        );
        assert_eq!(position("a.go"), None);
    }
}
