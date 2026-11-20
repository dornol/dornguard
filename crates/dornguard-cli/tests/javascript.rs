#![allow(clippy::unwrap_used)]
use std::fs;

mod common;
use common::{check, project, report};

#[test]
#[ignore = "requires ESLint 9 or 10 on PATH"]
fn javascript_profile_flags_string_evaluation_and_async_promise_executors() {
    let project = project("class Unused {}\n");
    let root = project.path();
    fs::remove_file(root.join("src/Example.java")).unwrap();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [javascript]\n",
    )
    .unwrap();
    fs::write(
        root.join("src/app.js"),
        r#"eval("1 + 1");
setTimeout("refresh()", 10);
const make = new Function("return 1");
new Promise(async (resolve) => resolve(1));

setTimeout(() => refresh(), 10);
const add = (a, b) => a + b;
new Promise((resolve) => resolve(1));
"#,
    )
    .unwrap();
    // The JavaScript rules are warnings, so they do not fail the check.
    let result = report(check(root).output().unwrap(), 0);
    assert_eq!(result["complete"], true, "{result}");
    assert_eq!(result["files_scanned"], 1, "{result}");
    let mut findings = result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| {
            (
                finding["line"].as_u64().unwrap(),
                finding["rule_id"].as_str().unwrap(),
                finding["engine"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    findings.sort_unstable();
    assert_eq!(
        findings,
        [
            (1, "javascript.no-eval", "eslint"),
            (2, "javascript.no-implied-eval", "eslint"),
            (3, "javascript.no-new-func", "eslint"),
            (4, "javascript.no-async-promise-executor", "eslint"),
        ],
        "{result}"
    );
}
