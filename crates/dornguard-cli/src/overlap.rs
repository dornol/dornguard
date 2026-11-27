//! Combine diagnostics from different rules that describe the same cause.

use std::collections::BTreeSet;

use dornguard_core::{Diagnostic, Outcome};

const UNNECESSARY_IMPORT: &str = "java.unnecessary-import";
const FULLY_QUALIFIED_TYPE: &str = "java.fully-qualified-type";
const WILDCARD_IMPORT: &str = "java.wildcard-import";

/// A type that is imported but written with its full name is one problem: the fix is to
/// use the simple name, which also makes the import necessary. Removing the import, as
/// `java.unnecessary-import` alone suggests, is the wrong advice for these files.
pub fn merge(outcome: &mut Outcome) {
    drop_unused_wildcards(outcome);
    let qualified: BTreeSet<(String, String)> = outcome
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.rule_id == FULLY_QUALIFIED_TYPE)
        .filter_map(key)
        .collect();
    if qualified.is_empty() {
        return;
    }
    let imported: BTreeSet<(String, String)> = outcome
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.rule_id == UNNECESSARY_IMPORT)
        .filter_map(key)
        .filter(|item| qualified.contains(item))
        .collect();
    if imported.is_empty() {
        return;
    }
    outcome.diagnostics.retain(|diagnostic| {
        diagnostic.rule_id != UNNECESSARY_IMPORT
            || !key(diagnostic).is_some_and(|item| imported.contains(&item))
    });
    for diagnostic in &mut outcome.diagnostics {
        if diagnostic.rule_id != FULLY_QUALIFIED_TYPE
            || !key(diagnostic).is_some_and(|item| imported.contains(&item))
        {
            continue;
        }
        let Some(name) = diagnostic
            .subject
            .as_deref()
            .and_then(|subject| subject.rsplit('.').next())
        else {
            continue;
        };
        diagnostic.message = format!(
            "{} It is already imported, so replace it with `{name}`.",
            diagnostic.message
        );
        diagnostic.fix = format!(
            "Replace the fully qualified name with `{name}`. Keep the existing import, which then becomes used."
        );
    }
}

/// An unused wildcard import is one problem: the fix is to remove it, as
/// `java.unnecessary-import` says, not to expand it into single-type imports.
fn drop_unused_wildcards(outcome: &mut Outcome) {
    let unused: BTreeSet<(String, u32)> = outcome
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.rule_id == UNNECESSARY_IMPORT)
        .filter_map(line_key)
        .collect();
    outcome.diagnostics.retain(|diagnostic| {
        diagnostic.rule_id != WILDCARD_IMPORT
            || !line_key(diagnostic).is_some_and(|item| unused.contains(&item))
    });
}

fn line_key(diagnostic: &Diagnostic) -> Option<(String, u32)> {
    Some((diagnostic.path.clone()?, diagnostic.line?))
}

fn key(diagnostic: &Diagnostic) -> Option<(String, String)> {
    Some((diagnostic.path.clone()?, diagnostic.subject.clone()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use dornguard_core::{Confidence, EngineName, Severity};

    fn diagnostic(rule: &str, path: &str, subject: &str) -> Diagnostic {
        Diagnostic {
            path: Some(path.to_owned()),
            module: None,
            class_name: None,
            line: Some(1),
            column: None,
            end_line: None,
            end_column: None,
            subject: Some(subject.to_owned()),
            rule_id: rule.to_owned(),
            engine: EngineName::Builtin,
            severity: Severity::Warning,
            confidence: Confidence::default(),
            message: format!("{rule} (`{subject}`)."),
            fix: "Original.".to_owned(),
            edit: None,
            legacy_fingerprints: Vec::new(),
            group: None,
            fingerprint: None,
        }
    }

    fn rules(outcome: &Outcome) -> Vec<(&str, &str)> {
        outcome
            .diagnostics
            .iter()
            .map(|item| (item.rule_id.as_str(), item.path.as_deref().unwrap()))
            .collect()
    }

    #[test]
    fn an_imported_type_written_in_full_becomes_one_diagnostic() {
        let mut outcome = Outcome {
            diagnostics: vec![
                diagnostic(UNNECESSARY_IMPORT, "A.java", "java.util.List"),
                diagnostic(FULLY_QUALIFIED_TYPE, "A.java", "java.util.List"),
            ],
            ..Outcome::default()
        };
        merge(&mut outcome);
        assert_eq!(rules(&outcome), [(FULLY_QUALIFIED_TYPE, "A.java")]);
        let merged = &outcome.diagnostics[0];
        assert!(
            merged.message.ends_with("replace it with `List`."),
            "{}",
            merged.message
        );
        assert!(merged.fix.contains("Keep the existing import"));
    }

    #[test]
    fn an_unused_wildcard_import_is_reported_only_as_unnecessary() {
        let mut used = diagnostic(WILDCARD_IMPORT, "A.java", "java.util.*");
        used.line = Some(2);
        let mut outcome = Outcome {
            diagnostics: vec![
                diagnostic(UNNECESSARY_IMPORT, "A.java", "java.io.*"),
                diagnostic(WILDCARD_IMPORT, "A.java", "java.io.*"),
                used,
            ],
            ..Outcome::default()
        };
        merge(&mut outcome);
        assert_eq!(
            rules(&outcome),
            [(UNNECESSARY_IMPORT, "A.java"), (WILDCARD_IMPORT, "A.java")]
        );
        assert_eq!(outcome.diagnostics[1].line, Some(2));
    }

    #[test]
    fn unrelated_imports_and_other_files_are_left_alone() {
        let mut outcome = Outcome {
            diagnostics: vec![
                diagnostic(UNNECESSARY_IMPORT, "A.java", "java.util.Set"),
                diagnostic(UNNECESSARY_IMPORT, "B.java", "java.util.List"),
                diagnostic(FULLY_QUALIFIED_TYPE, "A.java", "java.util.List"),
            ],
            ..Outcome::default()
        };
        merge(&mut outcome);
        assert_eq!(outcome.diagnostics.len(), 3);
        assert_eq!(outcome.diagnostics[2].fix, "Original.");
    }

    #[test]
    fn a_suppressed_full_name_does_not_hide_the_unused_import() {
        let mut outcome = Outcome {
            diagnostics: vec![diagnostic(UNNECESSARY_IMPORT, "A.java", "java.util.List")],
            ..Outcome::default()
        };
        merge(&mut outcome);
        assert_eq!(outcome.diagnostics.len(), 1);
    }
}
