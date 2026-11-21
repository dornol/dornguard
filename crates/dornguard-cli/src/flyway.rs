use std::path::{Path, PathBuf};

use dornguard_config::Rule;
use dornguard_core::{Diagnostic, Outcome, fingerprint};
use miette::{IntoDiagnostic, Result, WrapErr, bail};

use crate::changed;

pub async fn check(root: &Path, target: &Path, base: Option<&str>, rule: &Rule) -> Result<Outcome> {
    let start = match base {
        Some(base) => changed::compare_commits(root, base).await?.0,
        None => changed::resolve_commit(root, "HEAD").await?,
    };
    let output = changed::git(
        root,
        &[
            "diff",
            "--name-status",
            "-z",
            "--no-renames",
            "--no-ext-diff",
            "--no-textconv",
            "--diff-filter=MDT",
            "--relative",
            &start,
            "--",
            ".",
        ],
    )
    .await
    .wrap_err("Could not compare existing Flyway migrations")?;
    let mut fields = std::str::from_utf8(&output)
        .into_diagnostic()
        .wrap_err("Git reported a migration path with a non-UTF-8 name")?
        .split_terminator('\0');
    let mut outcome = Outcome::default();
    while let Some(status) = fields.next() {
        let Some(path) = fields.next() else {
            bail!("Git returned an incomplete migration change");
        };
        let relative = PathBuf::from(path);
        if !root.join(&relative).starts_with(target) || !is_versioned_migration(&relative) {
            continue;
        }
        let action = match status {
            "M" => "modified",
            "D" => "deleted",
            "T" => "replaced",
            _ => {
                bail!("Git returned an unexpected migration change status");
            }
        };
        let patch = changed::git(
            root,
            &[
                "diff",
                "--no-ext-diff",
                "--no-textconv",
                "--binary",
                &start,
                "--",
                path,
            ],
        )
        .await
        .wrap_err("Could not fingerprint the Flyway migration change")?;
        outcome.scanned_files.insert(relative);
        outcome.diagnostics.push(Diagnostic {
            path: Some(path.to_owned()),
            module: None,
            class_name: None,
            line: None,
            column: None,
            end_line: None,
            end_column: None,
            subject: None,
            rule_id: rule.id.clone(),
            engine: rule.engine,
            severity: rule.severity,
            confidence: rule.confidence,
            message: format!("{} The file was {action}.", rule.description),
            fix: rule.fix.clone(),
            fingerprint: Some(fingerprint(&String::from_utf8_lossy(&patch))),
            edit: None,
            legacy_fingerprints: Vec::new(),
        });
    }
    Ok(outcome)
}

fn is_versioned_migration(path: &Path) -> bool {
    let Some((version, description)) = path
        .file_stem()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix('V'))
        .and_then(|name| name.split_once("__"))
    else {
        return false;
    };
    path.extension().is_some_and(|ext| ext == "sql")
        && !description.is_empty()
        && version
            .split(['.', '_'])
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
        && path
            .parent()
            .is_some_and(|parent| parent.ancestors().any(|dir| dir.ends_with("db/migration")))
}
