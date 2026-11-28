use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::time::Duration;

use dornguard_config::Rule;
use dornguard_core::{Diagnostic, EngineName, Outcome, SourceLanguage};
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
    let (typed, plain): (Vec<&Rule>, Vec<&Rule>) = request
        .rules
        .iter()
        .copied()
        .partition(|rule| rule.uses_type_information());
    let plain = rule_map(&plain)?;
    let typed = rule_map(&typed)?;
    let typescript_files = request
        .files
        .iter()
        .filter(|file| SourceLanguage::of(file) == Some(SourceLanguage::Typescript))
        .cloned()
        .collect::<Vec<_>>();
    let parser = if typescript_files.is_empty() {
        None
    } else {
        Some(
            crate::install::typescript_parser_base(request.root).ok_or_else(|| {
                miette!(
                    "TypeScript sources need @typescript-eslint/parser; run \
                 'dornguard engines install eslint' or add it to the project's devDependencies"
                )
            })?,
        )
    };
    let mut sources = Sources::default();
    if !plain.is_empty() {
        let config = write_config(temp.path(), &plain, parser.as_deref())?;
        lint(
            request,
            request.files,
            &config,
            &plain,
            false,
            deadline,
            &mut sources,
            outcome,
        )
        .await?;
    }
    if !typed.is_empty() && !typescript_files.is_empty() {
        let base = parser
            .as_deref()
            .filter(|base| {
                base.join("node_modules/@typescript-eslint/eslint-plugin/package.json")
                    .is_file()
            })
            .ok_or_else(|| {
                miette!(
                    "Rules that use TypeScript type information need @typescript-eslint/eslint-plugin \
                     next to @typescript-eslint/parser; run 'dornguard engines install eslint' again \
                     or add it to the project's devDependencies"
                )
            })?;
        let (covered, uncovered): (Vec<_>, Vec<_>) = typescript_files
            .into_iter()
            .partition(|file| has_tsconfig(request.root, file));
        for file in &uncovered {
            outcome.scanned_files.insert(file.clone());
        }
        let mut skipped = uncovered.len();
        if !covered.is_empty() {
            let config = write_typed_config(temp.path(), &typed, base, request.root)?;
            skipped += lint(
                request,
                &covered,
                &config,
                &typed,
                true,
                deadline,
                &mut sources,
                outcome,
            )
            .await?;
        }
        if skipped > 0 {
            outcome.analysis_notes.insert(format!(
                "{skipped} TypeScript {} not part of a tsconfig.json project, so rules that use type information ({}) skip {}.",
                if skipped == 1 { "file is" } else { "files are" },
                typed
                    .values()
                    .map(|rule| rule.id.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
                if skipped == 1 { "it" } else { "them" },
            ));
        }
    }
    Ok(())
}

/// Run ESLint over `files` in batches. Returns the number of files the TypeScript project
/// service did not find, which typed runs skip instead of failing.
#[allow(clippy::too_many_arguments)]
async fn lint(
    request: Request<'_>,
    files: &[PathBuf],
    config: &std::path::Path,
    rules: &BTreeMap<String, &Rule>,
    typed: bool,
    deadline: Instant,
    sources: &mut Sources,
    outcome: &mut Outcome,
) -> Result<usize> {
    let mut skipped = 0;
    for batch in files.chunk_by(budgeted_batch()) {
        let mut lint = command(ENGINE, request.root)?;
        lint.args([
            "--no-config-lookup",
            "--no-inline-config",
            "--no-ignore",
            "--format=json",
            "--config",
        ])
        .arg(config)
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
        for report in reports {
            skipped += record_report(request.root, batch, report, rules, typed, sources, outcome)?;
        }
        if batch
            .iter()
            .any(|path| !outcome.scanned_files.contains(path))
            && outcome.failures.is_empty()
        {
            bail!("ESLint skipped requested source files");
        }
    }
    Ok(skipped)
}

fn rule_map<'r>(rules: &[&'r Rule]) -> Result<BTreeMap<String, &'r Rule>> {
    rules
        .iter()
        .map(|rule| Ok((rule.implementation()?.rule_id.clone(), *rule)))
        .collect()
}

/// Whether a tsconfig.json in the file's directory or an ancestor within the project
/// can describe the file.
fn has_tsconfig(root: &std::path::Path, file: &std::path::Path) -> bool {
    file.ancestors()
        .skip(1)
        .any(|dir| root.join(dir).join("tsconfig.json").is_file())
}

/// Convert one ESLint file report into diagnostics, failures and coverage.
fn record_report(
    root: &std::path::Path,
    batch: &[PathBuf],
    report: FileReport,
    rules: &BTreeMap<String, &Rule>,
    typed: bool,
    sources: &mut Sources,
    outcome: &mut Outcome,
) -> Result<usize> {
    let expected = batch.iter().collect::<BTreeSet<_>>();
    let path = relative_path(root, &report.file_path)
        .filter(|path| expected.contains(path))
        .ok_or_else(|| miette!("ESLint returned a file outside the requested sources"))?;
    let mut parsed = true;
    // typescript-eslint 8 reports a file outside every tsconfig.json project as
    // "<path> was not found by the project service".
    if typed
        && report.messages.iter().any(|finding| {
            finding.fatal
                && finding
                    .message
                    .contains("was not found by the project service")
        })
    {
        outcome.scanned_files.insert(path);
        return Ok(1);
    }
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
        let source = sources.get(root, &path);
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
            confidence: rule.confidence,
            message: rule.description.clone(),
            fix: rule.fix.clone(),
            edit: None,
            legacy_fingerprints: Vec::new(),
            group: None,
            fingerprint: finding
                .end_line
                .zip(finding.end_column)
                .and_then(|end| sources.span(root, &path, (line, column), end)),
        });
    }
    if parsed {
        outcome.scanned_files.insert(path);
    }
    Ok(0)
}

/// TypeScript sources, which ESLint does not lint unless a config names them.
const TYPESCRIPT_FILES: [&str; 4] = ["**/*.ts", "**/*.mts", "**/*.cts", "**/*.tsx"];

/// Write the ESLint flat config. With `parser`, the config also lints TypeScript
/// with `@typescript-eslint/parser` resolved from that directory. The parser runs
/// without type information, so only syntax-based rules apply.
fn write_config(
    dir: &std::path::Path,
    rules: &BTreeMap<String, &Rule>,
    parser: Option<&std::path::Path>,
) -> Result<PathBuf> {
    let config = dir.join("eslint.config.mjs");
    let enabled = rules
        .keys()
        .map(|id| (id.as_str(), "error"))
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
    let source = match parser {
        None => format!("export default {contents};\n"),
        Some(base) => {
            let anchor = serde_json::to_string(&base.join("package.json").to_string_lossy())
                .into_diagnostic()?;
            let files = json!(TYPESCRIPT_FILES);
            format!(
                "import {{ createRequire }} from \"node:module\";\n\
                 const parser = createRequire({anchor})(\"@typescript-eslint/parser\");\n\
                 export default [\n\
                 ...{contents},\n\
                 {{ files: {files}, languageOptions: {{ parser }} }},\n\
                 {{ files: [\"**/*.tsx\"], languageOptions: {{ parserOptions: {{ ecmaFeatures: {{ jsx: true }} }} }} }},\n\
                 ];\n"
            )
        }
    };
    std::fs::write(&config, source)
        .into_diagnostic()
        .wrap_err("Could not write ESLint configuration")?;
    Ok(config)
}

/// Write the ESLint flat config for typescript-eslint rules that read type information.
/// The project service finds each file's tsconfig.json below `root`; the parser and the
/// plugin are resolved from `base`.
fn write_typed_config(
    dir: &std::path::Path,
    rules: &BTreeMap<String, &Rule>,
    base: &std::path::Path,
    root: &std::path::Path,
) -> Result<PathBuf> {
    let config = dir.join("eslint.typed.config.mjs");
    let enabled = rules
        .keys()
        .map(|id| (id.as_str(), "error"))
        .collect::<BTreeMap<_, _>>();
    let anchor =
        serde_json::to_string(&base.join("package.json").to_string_lossy()).into_diagnostic()?;
    let root = serde_json::to_string(&root.to_string_lossy()).into_diagnostic()?;
    let files = json!(TYPESCRIPT_FILES);
    let rules = json!(enabled);
    let source = format!(
        "import {{ createRequire }} from \"node:module\";\n\
         const require = createRequire({anchor});\n\
         const parser = require(\"@typescript-eslint/parser\");\n\
         const plugin = require(\"@typescript-eslint/eslint-plugin\");\n\
         export default [\n\
         {{ files: {files}, languageOptions: {{ parser, parserOptions: {{ projectService: true, tsconfigRootDir: {root} }} }}, plugins: {{ \"@typescript-eslint\": plugin }}, rules: {rules} }},\n\
         {{ files: [\"**/*.tsx\"], languageOptions: {{ parserOptions: {{ ecmaFeatures: {{ jsx: true }} }} }} }},\n\
         ];\n"
    );
    std::fs::write(&config, source)
        .into_diagnostic()
        .wrap_err("Could not write ESLint configuration")?;
    Ok(config)
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn typescript_config_is_added_only_with_a_parser() {
        let dir = tempfile::tempdir().unwrap();
        let rules = BTreeMap::new();
        let plain =
            std::fs::read_to_string(write_config(dir.path(), &rules, None).unwrap()).unwrap();
        assert!(plain.starts_with("export default [{"), "{plain}");
        assert!(!plain.contains("typescript-eslint"), "{plain}");

        let base = dir.path().join("project \"quoted\"");
        let typed = std::fs::read_to_string(write_config(dir.path(), &rules, Some(&base)).unwrap())
            .unwrap();
        let anchor = serde_json::to_string(&base.join("package.json").to_string_lossy()).unwrap();
        assert!(
            typed.contains(&format!(
                "createRequire({anchor})(\"@typescript-eslint/parser\")"
            )),
            "{typed}"
        );
        assert!(
            typed.contains(r#"files: ["**/*.ts","**/*.mts","**/*.cts","**/*.tsx"]"#),
            "{typed}"
        );
        assert!(typed.contains("jsx: true"), "{typed}");
    }
}
