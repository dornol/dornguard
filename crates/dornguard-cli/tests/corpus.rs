//! Regression corpus for rule false positives and false negatives.
//!
//! Each directory below `tests/corpus` is a small project with its own
//! `.dornguard.yml`. A line that must produce a finding ends with
//! `// expect: RULE_ID[, RULE_ID]` (`<!-- expect: RULE_ID -->` in markup).
//! Every other line must produce none. When a real project reports a false
//! positive, add the smallest reproduction as a clean line in the matching case
//! (or a new case), together with the line that must stay reported.
#![allow(clippy::unwrap_used)]
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

mod common;
use common::check;

type Finding = (String, u64, String);

fn copy(from: &Path, to: &Path) {
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            fs::create_dir_all(&target).unwrap();
            copy(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn expectations(root: &Path, dir: &Path, found: &mut BTreeSet<Finding>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            expectations(root, &path, found);
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let relative = path
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        for (index, line) in text.lines().enumerate() {
            let Some((_, rules)) = line.split_once("expect:") else {
                continue;
            };
            let rules = rules.trim_end().trim_end_matches("-->");
            for rule in rules.split(',') {
                found.insert((relative.clone(), index as u64 + 1, rule.trim().to_owned()));
            }
        }
    }
}

#[test]
#[ignore = "requires the pinned Semgrep engine"]
fn corpus_cases_report_exactly_the_expected_findings() {
    let corpus = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus");
    let mut cases = fs::read_dir(&corpus)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();
    cases.sort();
    assert!(!cases.is_empty());

    let mut problems = Vec::new();
    for case in cases {
        let name = case.file_name().unwrap().to_string_lossy().into_owned();
        let project = tempfile::tempdir().unwrap();
        copy(&case, project.path());
        let output = check(project.path()).output().unwrap();
        let result: serde_json::Value =
            serde_json::from_slice(&output.stdout).unwrap_or_else(|_| {
                panic!(
                    "{name}: no JSON report\n{}",
                    String::from_utf8_lossy(&output.stderr)
                )
            });
        if result["complete"] != true {
            problems.push(format!("{name}: analysis was incomplete: {result}"));
            continue;
        }
        let actual = result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|finding| {
                (
                    finding["path"].as_str().unwrap().to_owned(),
                    finding["line"].as_u64().unwrap(),
                    finding["rule_id"].as_str().unwrap().to_owned(),
                )
            })
            .collect::<BTreeSet<Finding>>();
        let mut expected = BTreeSet::new();
        expectations(&case, &case, &mut expected);
        for (path, line, rule) in expected.difference(&actual) {
            problems.push(format!("{name}: missing {rule} at {path}:{line}"));
        }
        for (path, line, rule) in actual.difference(&expected) {
            problems.push(format!("{name}: unexpected {rule} at {path}:{line}"));
        }
    }
    assert!(problems.is_empty(), "\n{}", problems.join("\n"));
}
