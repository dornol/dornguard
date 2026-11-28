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

#[test]
#[ignore = "requires ESLint 9 or 10 and @typescript-eslint/parser"]
fn javascript_profile_checks_typescript_sources_but_not_declaration_files() {
    let project = project("class Unused {}\n");
    let root = project.path();
    fs::remove_file(root.join("src/Example.java")).unwrap();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [javascript]\n",
    )
    .unwrap();
    fs::write(
        root.join("src/order.ts"),
        r#"interface Order { items?: string[]; total: number }
enum Status { Open, Closed }
export function isMissing(order: Order): boolean { return order.total === NaN; }
export const run = <T,>(code: T): unknown => eval(String(code));
export const status: Status = Status.Open;
export const count = (order?: Order): number => order?.items?.length ?? 0;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/view.tsx"),
        r#"type Props = { label: string };
export const View = ({ label }: Props) => <a href="javascript:void(0)">{label}</a>;
"#,
    )
    .unwrap();
    fs::write(
        root.join("src/types.d.ts"),
        "declare const broken: number;\nexport declare function check(n: number): n is never;\n",
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 0);
    assert_eq!(result["complete"], true, "{result}");
    assert_eq!(result["files_scanned"], 2, "{result}");
    let mut findings = result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| {
            (
                finding["path"].as_str().unwrap(),
                finding["line"].as_u64().unwrap(),
                finding["rule_id"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    findings.sort_unstable();
    assert_eq!(
        findings,
        [
            ("src/order.ts", 3, "javascript.use-isnan"),
            ("src/order.ts", 4, "javascript.no-eval"),
            ("src/view.tsx", 2, "javascript.no-script-url"),
        ],
        "{result}"
    );
}

#[test]
#[ignore = "requires ESLint 9 or 10, @typescript-eslint/parser and @typescript-eslint/eslint-plugin"]
fn typescript_review_uses_type_information_for_files_in_a_tsconfig_project() {
    let project = project("class Unused {}\n");
    let root = project.path();
    fs::remove_file(root.join("src/Example.java")).unwrap();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [typescript-review]\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("web/src")).unwrap();
    fs::write(
        root.join("web/tsconfig.json"),
        r#"{ "compilerOptions": { "strict": true, "target": "ES2022", "module": "ESNext", "moduleResolution": "Bundler", "noEmit": true }, "include": ["src"] }"#,
    )
    .unwrap();
    fs::write(
        root.join("web/src/app.ts"),
        r#"async function save(id: number): Promise<void> {
  if (id < 0) throw new Error("bad id");
}

export async function run(): Promise<void> {
  save(1);
  await save(2);
  void save(3);
  save(4).catch(() => undefined);
  [save(5), save(6)];
  Promise.resolve(7).then(() => undefined, () => undefined);
}
"#,
    )
    .unwrap();
    // Outside the tsconfig.json include list, and with no tsconfig.json at all.
    fs::write(
        root.join("web/legacy.ts"),
        "async function save(): Promise<void> {}\nsave();\n",
    )
    .unwrap();
    fs::write(
        root.join("src/tool.ts"),
        "async function work(): Promise<void> {}\nwork();\n",
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 0);
    assert_eq!(result["complete"], true, "{result}");
    let findings = result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| {
            (
                finding["path"].as_str().unwrap(),
                finding["line"].as_u64().unwrap(),
                finding["rule_id"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        findings,
        [
            ("web/src/app.ts", 6, "typescript.no-floating-promises"),
            ("web/src/app.ts", 10, "typescript.no-floating-promises"),
        ],
        "{result}"
    );
    assert_eq!(
        result["analysis_notes"][0],
        "2 TypeScript files are not part of a tsconfig.json project, so rules that use type information (typescript.no-floating-promises) skip them.",
        "{result}"
    );
}

/// The first executable `name` on PATH, with symlinks resolved.
#[cfg(unix)]
fn on_path(name: &str) -> Option<std::path::PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(name))
        .find(|path| path.is_file())
        .and_then(|path| fs::canonicalize(path).ok())
}

#[test]
#[ignore = "requires Node.js and ESLint 9 or 10 on PATH or managed"]
#[cfg(unix)]
fn typescript_without_a_parser_is_an_engine_error() {
    let managed = std::path::Path::new(&std::env::var_os("HOME").unwrap())
        .join(".local/share/dornguard/engines/eslint-10.14.0/node_modules/.bin/eslint");
    let eslint = on_path("eslint")
        .or_else(|| fs::canonicalize(managed).ok())
        .expect("ESLint on PATH or managed");
    let node = on_path("node").expect("Node.js on PATH");
    let project = project("class Unused {}\n");
    let root = project.path();
    fs::remove_file(root.join("src/Example.java")).unwrap();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [javascript]\n",
    )
    .unwrap();
    // The project provides ESLint but not the parser, PATH has only Node.js, and
    // there is no managed copy.
    fs::create_dir_all(root.join("node_modules/.bin")).unwrap();
    std::os::unix::fs::symlink(eslint, root.join("node_modules/.bin/eslint")).unwrap();
    fs::write(root.join("src/app.ts"), "export const n: number = 1;\n").unwrap();
    let engines = tempfile::tempdir().unwrap();
    let result = report(
        check(root)
            .env("DORNGUARD_ENGINE_DIR", engines.path())
            .env("PATH", node.parent().unwrap())
            .output()
            .unwrap(),
        3,
    );
    assert_eq!(result["complete"], false, "{result}");
    let message = result["failures"][0]["message"].as_str().unwrap();
    assert!(message.contains("@typescript-eslint/parser"), "{result}");
}
