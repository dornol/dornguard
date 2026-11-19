use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Write};
use std::path::Path;

use dornguard_config::Ignore;
use dornguard_core::{Diagnostic, Outcome, valid_fingerprint};
use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};
use serde::{Deserialize, Serialize};

const VERSION: u8 = 2;

pub struct Baseline(BTreeMap<Entry, usize>);

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
        for entry in entries {
            let identity = Entry {
                rule_id: entry.rule.clone(),
                path: Some(entry.path.clone()),
                module: None,
                class_name: None,
                fingerprint: entry.fingerprint.clone(),
            };
            *counts.entry(identity).or_insert(0) += 1;
        }
        Self(counts)
    }

    pub fn apply(mut self, outcome: &mut Outcome) {
        if self.0.is_empty() {
            return;
        }
        outcome.diagnostics.sort();
        outcome.diagnostics.dedup();
        outcome.diagnostics.retain(|diagnostic| {
            let Some(entry) = Entry::from_diagnostic(diagnostic) else {
                return true;
            };
            let Some(count) = self.0.get_mut(&entry) else {
                return true;
            };
            if *count == 0 {
                return true;
            }
            *count -= 1;
            outcome.suppressed_count += 1;
            false
        });
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
