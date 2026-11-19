use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::source::Sources;
use dornguard_config::UNNECESSARY_IMPORT_RULE;
use dornguard_core::{Diagnostic, EngineName, Outcome};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use serde::Deserialize;
use tokio::time::Instant;

use crate::{
    Engine, PMD_VERSION, Request, check_version, command, display_path, fail, relative_path,
    run_command,
};

const ENGINE: EngineName = EngineName::Pmd;

#[derive(Default)]
pub struct Pmd<'a> {
    pub classpath: Option<&'a [PathBuf]>,
}

impl Engine for Pmd<'_> {
    async fn check(&self, request: Request<'_>) -> Outcome {
        let mut outcome = Outcome::default();
        if let Err(error) = scan(request, self.classpath, &mut outcome).await {
            fail(&mut outcome, ENGINE, format!("{error:#}"));
        }
        outcome
    }
}

async fn scan(
    request: Request<'_>,
    classpath: Option<&[PathBuf]>,
    outcome: &mut Outcome,
) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(120);
    check_version(ENGINE, request.root, deadline).await?;
    let temp = tempfile::tempdir()
        .into_diagnostic()
        .wrap_err("Could not create PMD configuration directory")?;
    let mut refs = Vec::new();
    let mut rules = BTreeMap::new();
    for (index, rule) in request.rules.iter().enumerate() {
        let path = temp.path().join(format!("{index}.xml"));
        fs::write(&path, request.registry.engine_config(rule)?)
            .into_diagnostic()
            .wrap_err("Could not write PMD ruleset")?;
        refs.push(path.display().to_string());
        rules.insert(rule.implementation()?.rule_id.as_str(), *rule);
    }

    let file_list = temp.path().join("files.txt");
    let mut list = BufWriter::new(File::create(&file_list).into_diagnostic()?);
    for file in request.files {
        let path = request.root.join(file);
        let path = path.to_str().filter(|path| !path.contains(['\r', '\n']));
        let Some(path) = path else {
            bail!("PMD file lists require UTF-8 paths without line breaks");
        };
        writeln!(list, "{path}").into_diagnostic()?;
    }
    list.flush().into_diagnostic()?;

    let mut check = command(ENGINE, request.root)?;
    check
        .args([
            "check",
            "--no-cache",
            "--no-progress",
            "--no-fail-on-violation",
            "--show-suppressed",
            "--format",
            "json",
            "--encoding",
            "UTF-8",
            "--use-version",
            "java-21",
            "--file-list",
        ])
        .arg(file_list)
        .arg("--rulesets")
        .arg(refs.join(","));
    if let Some(classpath) = classpath {
        let path = temp.path().join("classpath.txt");
        let mut list = BufWriter::new(File::create(&path).into_diagnostic()?);
        for entry in classpath {
            let entry = entry
                .to_str()
                .filter(|entry| !entry.contains(['\r', '\n']) && *entry == entry.trim_end())
                .ok_or_else(|| miette::miette!("PMD cannot represent a classpath entry"))?;
            writeln!(list, "{entry}").into_diagnostic()?;
        }
        list.flush().into_diagnostic()?;
        check.arg("--aux-classpath").arg(classpath_argument(&path)?);
    }
    let output = run_command(ENGINE, &mut check, deadline).await?;
    if !output.status.success() {
        fail(
            outcome,
            ENGINE,
            format!("PMD scan failed ({})", output.status),
        );
    }
    let Ok(report): std::result::Result<Scan, _> = serde_json::from_slice(&output.stdout) else {
        bail!("PMD returned missing or invalid JSON");
    };
    if report.format_version != 0 || report.pmd_version != PMD_VERSION {
        bail!("PMD returned an unsupported report version");
    }
    if !report.processing_errors.is_empty() || !report.configuration_errors.is_empty() {
        fail(
            outcome,
            ENGINE,
            format!(
                "PMD reported {} processing and {} configuration errors",
                report.processing_errors.len(),
                report.configuration_errors.len()
            ),
        );
    }

    let expected: BTreeSet<_> = request.files.iter().collect();
    if outcome.failures.is_empty() {
        for file in request.files {
            if request.root.join(file).is_file() {
                outcome.scanned_files.insert(file.clone());
            } else {
                fail(
                    outcome,
                    ENGINE,
                    "A required PMD input disappeared during the check",
                );
            }
        }
    }
    let mut sources = Sources::default();
    for item in report.files.into_iter().chain(report.suppressed_violations) {
        let Some(path) =
            relative_path(request.root, &item.filename).filter(|path| expected.contains(path))
        else {
            fail(
                outcome,
                ENGINE,
                "PMD returned a diagnostic outside the requested files",
            );
            continue;
        };
        for finding in item.violations {
            let Some(rule) = rules.get(finding.rule.as_str()) else {
                fail(outcome, ENGINE, "PMD returned an unregistered rule ID");
                continue;
            };
            if finding.beginline == 0 || finding.begincolumn == 0 {
                fail(
                    outcome,
                    ENGINE,
                    "PMD returned an invalid diagnostic position",
                );
                continue;
            }
            if classpath.is_none()
                && rule.id == UNNECESSARY_IMPORT_RULE
                && finding.is_unused_wildcard_import()
            {
                continue;
            }
            outcome.diagnostics.push(Diagnostic {
                path: Some(display_path(&path)),
                module: None,
                class_name: None,
                line: Some(finding.beginline),
                column: Some(finding.begincolumn),
                rule_id: rule.id.clone(),
                engine: rule.engine,
                severity: rule.severity,
                message: rule.description.clone(),
                fix: rule.fix.clone(),
                fingerprint: sources.lines(
                    request.root,
                    &path,
                    (finding.beginline, finding.begincolumn),
                    (finding.endline, finding.endcolumn),
                ),
            });
        }
    }
    Ok(())
}

fn classpath_argument(path: &Path) -> Result<String> {
    let path = path
        .to_str()
        .ok_or_else(|| miette::miette!("PMD classpath file path must be UTF-8"))?;
    let mut argument = String::from("file:");
    // PMD parses a leading slash after "file:" as a URI. Windows paths
    // start with a drive letter or backslash and are read as native paths.
    if path.starts_with('/') {
        use std::fmt::Write as _;
        for byte in path.bytes() {
            if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
                argument.push(char::from(byte));
            } else {
                write!(argument, "%{byte:02X}").expect("writing to a String cannot fail");
            }
        }
    } else {
        argument.push_str(path);
    }
    Ok(argument)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Scan {
    format_version: u8,
    pmd_version: String,
    files: Vec<FileResults>,
    suppressed_violations: Vec<FileResults>,
    processing_errors: Vec<serde_json::Value>,
    configuration_errors: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct FileResults {
    filename: PathBuf,
    violations: Vec<Finding>,
}

#[derive(Deserialize)]
struct Finding {
    rule: String,
    #[serde(default)]
    description: String,
    beginline: u32,
    begincolumn: u32,
    endline: u32,
    endcolumn: u32,
}

impl Finding {
    fn is_unused_wildcard_import(&self) -> bool {
        // PMD 7.29 reports unused and duplicate imports with distinct messages.
        // Its pretty-printed import names are independent of source layout.
        self.description.ends_with(".*'")
            && (self.description.starts_with("Unused import '")
                || self.description.starts_with("Unused static import '"))
    }
}
