use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufReader, Write};
use std::path::Path;

use dornguard_config::Ignore;
use dornguard_core::{Diagnostic, Failure, FailureKind, Outcome, Scope, valid_fingerprint};
use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};
use serde::{Deserialize, Serialize};

const VERSION: u8 = 2;

pub struct Baseline(BTreeMap<Entry, usize>);

#[derive(Clone, Copy, Default)]
pub struct ProjectOptions {
    pub strict_changed: bool,
    pub ratchet: bool,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u8,
    violations: Vec<Entry>,
}

#[derive(Clone, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    rule_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    module: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    class_name: Option<String>,
    fingerprint: String,
}

impl Entry {
    fn from_diagnostic(diagnostic: &Diagnostic) -> Option<Self> {
        let (module, class_name) = if diagnostic.path.is_some() {
            (None, None)
        } else {
            (
                Some(diagnostic.module.clone()?),
                Some(diagnostic.class_name.clone()?),
            )
        };
        Some(Self {
            rule_id: diagnostic.rule_id.clone(),
            path: diagnostic.path.clone(),
            module,
            class_name,
            fingerprint: diagnostic.fingerprint.clone()?,
        })
    }
}

impl Baseline {
    pub fn load(path: &Path) -> Result<Self> {
        let file = File::open(path)
            .into_diagnostic()
            .wrap_err_with(|| format!("Could not open baseline {}", path.display()))?;
        let document: Document = serde_json::from_reader(BufReader::new(file))
            .into_diagnostic()
            .wrap_err_with(|| format!("Invalid baseline {}", path.display()))?;
        if !matches!(document.version, 1 | VERSION) {
            bail!("Unsupported baseline version {}", document.version);
        }
        let mut entries = BTreeMap::new();
        for entry in document.violations {
            let valid_location = match (&entry.path, &entry.module, &entry.class_name) {
                (Some(path), None, None) => !path.is_empty(),
                (None, Some(module), Some(class_name)) => {
                    document.version == VERSION && !module.is_empty() && !class_name.is_empty()
                }
                _ => false,
            };
            if entry.rule_id.is_empty() || !valid_location || !valid_fingerprint(&entry.fingerprint)
            {
                bail!("Baseline contains an invalid violation identity");
            }
            *entries.entry(entry).or_insert(0) += 1;
        }
        Ok(Self(entries))
    }

    pub fn from_config(entries: &[Ignore]) -> Self {
        let mut counts = BTreeMap::new();
        for (path, rule, fingerprint) in entries.iter().filter_map(Ignore::finding) {
            let identity = Entry {
                rule_id: rule.to_owned(),
                path: Some(path.to_owned()),
                module: None,
                class_name: None,
                fingerprint: fingerprint.to_owned(),
            };
            *counts.entry(identity).or_insert(0) += 1;
        }
        Self(counts)
    }

    /// Apply fingerprinted `ignore` entries and return those that matched no
    /// finding as `(rule, path)` pairs.
    pub fn apply(mut self, outcome: &mut Outcome) -> Vec<(String, String)> {
        self.apply_to(outcome, &BTreeSet::new());
        self.unmatched()
    }

    fn unmatched(&self) -> Vec<(String, String)> {
        self.0
            .iter()
            .filter(|(_, count)| **count > 0)
            .map(|(entry, _)| {
                (
                    entry.rule_id.clone(),
                    entry.path.clone().unwrap_or_default(),
                )
            })
            .collect()
    }

    /// Apply a project baseline selected with `--baseline`.
    ///
    /// With `strict_changed`, findings in files that a `--changed` run
    /// selected are reported even when the baseline records them, so legacy
    /// findings shrink wherever someone edits. With `ratchet`, a complete
    /// full scan fails when the baseline records findings that no longer
    /// occur, which keeps the baseline from hiding future regressions.
    pub fn apply_project(mut self, outcome: &mut Outcome, options: ProjectOptions) {
        let touched = if options.strict_changed && matches!(outcome.scope, Scope::Changed) {
            outcome
                .scanned_files
                .iter()
                .map(|path| path.to_string_lossy().replace('\\', "/"))
                .collect()
        } else {
            BTreeSet::new()
        };
        let total: usize = self.0.values().sum();
        let reported = self.apply_to(outcome, &touched);
        let remaining: usize = self.0.values().sum();
        if reported > 0 {
            outcome.analysis_notes.insert(format!(
                "{reported} baselined findings in changed files are reported because --strict-changed is set. Fix them while you edit these files."
            ));
        }
        let complete_full_scan = matches!(outcome.scope, Scope::Full)
            && outcome.failures.is_empty()
            && outcome.analysis_warnings.is_empty();
        if total > 0 && remaining > 0 && complete_full_scan {
            let message = format!(
                "The baseline records {remaining} findings that no longer occur. Run `dornguard baseline create --force` to shrink it."
            );
            if options.ratchet {
                outcome
                    .failures
                    .push(Failure::new(FailureKind::ConfigurationError, message));
            } else {
                outcome.analysis_notes.insert(message);
            }
        }
    }

    /// Returns the number of baselined findings kept because their file was
    /// touched.
    fn apply_to(&mut self, outcome: &mut Outcome, touched: &BTreeSet<String>) -> usize {
        if self.0.is_empty() {
            return 0;
        }
        outcome.diagnostics.sort();
        outcome.diagnostics.dedup();
        let mut reported = 0;
        // Match current identities first. A compatibility alias must not
        // spend a recorded occurrence belonging to another exact match.
        for legacy in [false, true] {
            outcome.diagnostics.retain(|diagnostic| {
                if diagnostic.location_independent {
                    // A group's reported file can change, so its identity is the rule and fingerprint.
                    if legacy {
                        return true;
                    }
                    if diagnostic
                        .path
                        .as_ref()
                        .is_some_and(|path| touched.contains(path))
                    {
                        reported += usize::from(self.recorded_group(diagnostic));
                        return true;
                    }
                    let matched = self.consume_group(diagnostic);
                    outcome.suppressed_count += usize::from(matched);
                    return !matched;
                }
                let Some(mut entry) = Entry::from_diagnostic(diagnostic) else {
                    return true;
                };
                if diagnostic
                    .path
                    .as_ref()
                    .is_some_and(|path| touched.contains(path))
                {
                    if legacy {
                        return true;
                    }
                    reported += usize::from(self.recorded(&entry, diagnostic));
                    return true;
                }
                let matched = if legacy {
                    diagnostic.legacy_fingerprints.iter().any(|hash| {
                        entry.fingerprint.clone_from(hash);
                        self.consume(&entry)
                    })
                } else {
                    self.consume(&entry)
                };
                outcome.suppressed_count += usize::from(matched);
                !matched
            });
        }
        reported
    }

    /// Recorded entries for a group: any fingerprint it has had, for its rule, in any file.
    fn group_entries(&self, diagnostic: &Diagnostic) -> Vec<Entry> {
        let current = diagnostic.fingerprint.as_deref();
        self.0
            .iter()
            .filter(|(entry, count)| {
                **count > 0
                    && entry.rule_id == diagnostic.rule_id
                    && (current == Some(entry.fingerprint.as_str())
                        || diagnostic.legacy_fingerprints.contains(&entry.fingerprint))
            })
            .map(|(entry, _)| entry.clone())
            .collect()
    }

    fn recorded_group(&self, diagnostic: &Diagnostic) -> bool {
        !self.group_entries(diagnostic).is_empty()
    }

    /// Spend one occurrence of every entry the group replaces, so a baseline written
    /// before diagnostics were grouped does not report the rest as gone.
    fn consume_group(&mut self, diagnostic: &Diagnostic) -> bool {
        let entries = self.group_entries(diagnostic);
        for entry in &entries {
            self.consume(entry);
        }
        !entries.is_empty()
    }

    fn recorded(&self, entry: &Entry, diagnostic: &Diagnostic) -> bool {
        let has = |entry: &Entry| self.0.get(entry).is_some_and(|count| *count > 0);
        has(entry)
            || diagnostic.legacy_fingerprints.iter().any(|hash| {
                let mut alias = entry.clone();
                alias.fingerprint.clone_from(hash);
                has(&alias)
            })
    }

    fn consume(&mut self, entry: &Entry) -> bool {
        let Some(count) = self.0.get_mut(entry).filter(|count| **count > 0) else {
            return false;
        };
        *count -= 1;
        true
    }

    pub fn create(path: &Path, diagnostics: &[Diagnostic], overwrite: bool) -> Result<usize> {
        let mut violations = diagnostics
            .iter()
            .map(|item| {
                Entry::from_diagnostic(item).ok_or_else(|| {
                    miette!(
                        "Could not fingerprint rule {} in {}",
                        item.rule_id,
                        item.path
                            .as_deref()
                            .or(item.module.as_deref())
                            .unwrap_or("unknown location"),
                    )
                })
            })
            .collect::<Result<Vec<_>>>()?;
        violations.sort();
        let count = violations.len();
        let document = Document {
            version: VERSION,
            violations,
        };

        let directory = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let mut temp = tempfile::NamedTempFile::new_in(directory)
            .into_diagnostic()
            .wrap_err("Could not prepare baseline file")?;
        serde_json::to_writer_pretty(&mut temp, &document).into_diagnostic()?;
        writeln!(temp).into_diagnostic()?;
        temp.as_file().sync_all().into_diagnostic()?;
        if overwrite {
            temp.persist(path)
        } else {
            temp.persist_noclobber(path)
        }
        .into_diagnostic()
        .wrap_err_with(|| format!("Could not save baseline {}", path.display()))?;
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dornguard_core::{Confidence, EngineName, Severity};

    fn fp(digit: char) -> String {
        digit.to_string().repeat(64)
    }

    fn diagnostic(
        path: &str,
        fingerprint: &str,
        legacy: &[String],
        independent: bool,
    ) -> Diagnostic {
        Diagnostic {
            path: Some(path.to_owned()),
            module: None,
            class_name: None,
            line: Some(1),
            column: None,
            end_line: None,
            end_column: None,
            subject: None,
            rule_id: "architecture.package-cycle".to_owned(),
            engine: EngineName::Archunit,
            severity: Severity::Error,
            confidence: Confidence::default(),
            message: String::new(),
            fix: String::new(),
            edit: None,
            legacy_fingerprints: legacy.to_vec(),
            location_independent: independent,
            fingerprint: Some(fingerprint.to_owned()),
        }
    }

    fn baseline(entries: &[(&str, String)]) -> Baseline {
        let violations = entries
            .iter()
            .map(|(path, fingerprint)| {
                serde_json::json!({
                    "rule_id": "architecture.package-cycle",
                    "path": path,
                    "fingerprint": fingerprint,
                })
            })
            .collect::<Vec<_>>();
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("baseline.json");
        std::fs::write(
            &file,
            serde_json::json!({ "version": 2, "violations": violations }).to_string(),
        )
        .unwrap();
        Baseline::load(&file).unwrap()
    }

    fn apply(baseline: Baseline, diagnostics: Vec<Diagnostic>) -> (Outcome, Vec<(String, String)>) {
        let mut outcome = Outcome {
            diagnostics,
            ..Outcome::default()
        };
        let unmatched = baseline.apply(&mut outcome);
        (outcome, unmatched)
    }

    #[test]
    fn a_group_stays_baselined_when_its_reported_file_changes() {
        // Recorded while the group was reported in A.java; it is now reported in B.java.
        let (outcome, unmatched) = apply(
            baseline(&[("A.java", fp('a'))]),
            vec![diagnostic("B.java", &fp('a'), &[], true)],
        );
        assert!(outcome.diagnostics.is_empty());
        assert_eq!(outcome.suppressed_count, 1);
        assert!(unmatched.is_empty());
    }

    #[test]
    fn a_group_spends_every_entry_it_replaces() {
        // A baseline from before grouping has one entry per class dependency.
        let legacy = [fp('b'), fp('c'), fp('d')];
        let (outcome, unmatched) = apply(
            baseline(&[
                ("A.java", fp('b')),
                ("B.java", fp('c')),
                ("C.java", fp('d')),
            ]),
            vec![diagnostic("C.java", &fp('a'), &legacy, true)],
        );
        assert!(outcome.diagnostics.is_empty());
        assert!(unmatched.is_empty(), "{unmatched:?}");
    }

    #[test]
    fn a_different_group_is_still_reported() {
        let (outcome, unmatched) = apply(
            baseline(&[("A.java", fp('a'))]),
            vec![diagnostic("A.java", &fp('e'), &[], true)],
        );
        assert_eq!(outcome.diagnostics.len(), 1);
        assert_eq!(unmatched.len(), 1);
    }

    #[test]
    fn an_ordinary_finding_still_needs_the_same_file() {
        let (outcome, _) = apply(
            baseline(&[("A.java", fp('a'))]),
            vec![diagnostic("B.java", &fp('a'), &[], false)],
        );
        assert_eq!(outcome.diagnostics.len(), 1);
    }
}
