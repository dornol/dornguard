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

#[test]
#[ignore = "requires ESLint 9 or 10 on PATH"]
fn javascript_profile_flags_always_true_comparisons_and_lost_results() {
    let project = project("class Unused {}\n");
    let root = project.path();
    fs::remove_file(root.join("src/Example.java")).unwrap();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [javascript]\n",
    )
    .unwrap();
    fs::write(
        root.join("src/app.mjs"),
        r#"export function close(file) { try { return file.read(); } finally { return null; } }
export const same = (value) => value === value;
export const empty = (list) => list === [];
export const absent = (key, map) => !key in map;
export const items = (order) => [...order?.items];
export const missing = (n) => n === NaN;
export const isText = (v) => typeof v === "strng";
export const link = "javascript:void(0)";

export function cleanup(file) { try { return file.read(); } finally { file.close(); } }
export const isNumber = (n) => Number.isNaN(n);
export const isEmpty = (list) => list.length === 0;
export const notIn = (key, map) => !(key in map);
export const count = (order) => [...(order?.items ?? [])];
export const isString = (v) => typeof v === "string";
export const home = "https://example.com/";
export const wait = new Promise((resolve) => setTimeout(resolve, 10));
"#,
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 0);
    assert_eq!(result["complete"], true, "{result}");
    let mut findings = result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| {
            (
                finding["line"].as_u64().unwrap(),
                finding["rule_id"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    findings.sort_unstable();
    assert_eq!(
        findings,
        [
            (1, "javascript.no-unsafe-finally"),
            (2, "javascript.no-self-compare"),
            (3, "javascript.no-constant-binary-expression"),
            (4, "javascript.no-unsafe-negation"),
            (5, "javascript.no-unsafe-optional-chaining"),
            (6, "javascript.use-isnan"),
            (7, "javascript.valid-typeof"),
            (8, "javascript.no-script-url"),
        ],
        "{result}"
    );
}
