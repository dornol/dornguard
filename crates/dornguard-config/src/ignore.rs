use std::path::Path;

use serde::Deserialize;

use crate::{PathGlob, project_relative};
use dornguard_core::valid_fingerprint;

#[derive(Deserialize)]
#[serde(try_from = "IgnoreEntry")]
pub struct Ignore {
    pub(crate) rule: Option<String>,
    selector: IgnoreSelector,
}

pub(crate) enum IgnoreSelector {
    Finding { path: String, fingerprint: String },
    Path(PathGlob),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct IgnoreEntry {
    path: String,
    rule: Option<String>,
    fingerprint: Option<String>,
    reason: String,
}

impl TryFrom<IgnoreEntry> for Ignore {
    type Error = String;

    fn try_from(entry: IgnoreEntry) -> std::result::Result<Self, Self::Error> {
        let IgnoreEntry {
            path,
            rule,
            fingerprint,
            reason,
        } = entry;
        if reason.trim().is_empty() {
            return Err(format!("Ignore entry for {path} requires a reason"));
        }
        let selector = if let Some(fingerprint) = fingerprint {
            if !project_relative(&path) {
                return Err(format!(
                    "Ignore path must be a project-relative file path with forward slashes: {path}"
                ));
            }
            if !valid_fingerprint(&fingerprint) {
                return Err(format!(
                    "Ignore fingerprint for {path} must be a lowercase SHA-256 digest"
                ));
            }
            if rule.as_deref().is_none_or(|rule| rule.contains('*')) {
                return Err(format!("Ignore fingerprint for {path} requires a rule ID"));
            }
            IgnoreSelector::Finding { path, fingerprint }
        } else {
            let pattern = if path.ends_with('/') {
                format!("{path}**")
            } else {
                path
            };
            IgnoreSelector::Path(PathGlob::new(&pattern)?)
        };
        Ok(Self { rule, selector })
    }
}

pub(crate) fn ignore_rule_matches(pattern: &str, rule: &str) -> bool {
    if let Some(prefix) = pattern
        .strip_suffix(".*")
        .filter(|prefix| !prefix.is_empty())
    {
        rule.strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with('.'))
    } else {
        pattern == rule
    }
}

impl Ignore {
    pub fn matches_path(&self, path: &Path, rule: &str) -> bool {
        matches!(&self.selector, IgnoreSelector::Path(pattern) if pattern.0.is_match(path))
            && self
                .rule
                .as_deref()
                .is_none_or(|selected| ignore_rule_matches(selected, rule))
    }

    pub fn finding(&self) -> Option<(&str, &str, &str)> {
        let IgnoreSelector::Finding { path, fingerprint } = &self.selector else {
            return None;
        };
        Some((path, self.rule.as_deref()?, fingerprint))
    }
}
