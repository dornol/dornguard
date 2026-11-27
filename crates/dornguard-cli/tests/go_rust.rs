#![allow(clippy::unwrap_used)]
use std::fs;
use std::path::Path;

mod common;
use common::{check, report};
use serde_json::Value;

fn write(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, contents).unwrap();
}

fn findings(result: &Value) -> Vec<(String, u64, String, String)> {
    let mut findings = result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| {
            (
                finding["path"].as_str().unwrap().to_owned(),
                finding["line"].as_u64().unwrap(),
                finding["rule_id"].as_str().unwrap().to_owned(),
                finding["subject"].as_str().unwrap().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    findings.sort_unstable();
    findings
}

#[test]
#[ignore = "requires Go 1.22 or later on PATH"]
fn go_profile_reports_go_vet_findings_with_their_analyzer() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    write(root, ".dornguard.yml", "version: 1\nprofiles: [go]\n");
    write(root, "go.mod", "module example.com/vetted\n\ngo 1.22\n");
    write(
        root,
        "pkg/greet.go",
        "package pkg\n\nimport \"fmt\"\n\nfunc Greet(name string) {\n\tfmt.Printf(\"%d\\n\", name)\n\tx := 1\n\tx = x\n\t_ = x\n}\n",
    );
    write(
        root,
        "main.go",
        "package main\n\nimport \"example.com/vetted/pkg\"\n\nfunc main() { pkg.Greet(\"a\") }\n",
    );
    // Vendored and testdata sources are not project code.
    write(root, "vendor/example.com/dep/dep.go", "package dep\n");
    write(root, "pkg/testdata/sample.go", "package sample\n");

    let result = report(check(root).output().unwrap(), 1);
    assert_eq!(result["complete"], true, "{result}");
    assert_eq!(result["files_scanned"], 2, "{result}");
    assert_eq!(
        findings(&result),
        [
            ("pkg/greet.go".into(), 6, "go.vet".into(), "printf".into()),
            ("pkg/greet.go".into(), 8, "go.vet".into(), "assign".into()),
        ],
        "{result}"
    );
    let assign = &result["diagnostics"][1];
    assert_eq!(assign["edit"]["replacement"], "", "{result}");
}

#[test]
#[ignore = "requires Go 1.22 or later on PATH"]
fn go_build_errors_are_engine_errors() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    write(root, ".dornguard.yml", "version: 1\nprofiles: [go]\n");
    write(root, "go.mod", "module example.com/broken\n\ngo 1.22\n");
    write(
        root,
        "broken.go",
        "package broken\n\nfunc Broken() int { return \"s\" }\n",
    );

    let result = report(check(root).output().unwrap(), 3);
    assert_eq!(result["complete"], false, "{result}");
    let message = result["failures"][0]["message"].as_str().unwrap();
    assert!(message.contains("broken.go:3"), "{result}");
}

#[test]
#[ignore = "requires cargo with the Clippy component"]
fn rust_profile_reports_clippy_and_compiler_lints() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    write(root, ".dornguard.yml", "version: 1\nprofiles: [rust]\n");
    write(
        root,
        "Cargo.toml",
        "[package]\nname = \"linted\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[workspace]\n",
    );
    write(
        root,
        "src/main.rs",
        "fn main() {\n    let values = vec![1, 2, 3];\n    let unused = 5;\n    if values.len() == 0 {\n        println!(\"empty\");\n    }\n}\n",
    );

    // Clippy and rustc lints are warnings by default.
    let result = report(check(root).output().unwrap(), 0);
    assert_eq!(result["complete"], true, "{result}");
    assert_eq!(
        findings(&result),
        [
            (
                "src/main.rs".into(),
                2,
                "rust.clippy".into(),
                "clippy::useless_vec".into()
            ),
            (
                "src/main.rs".into(),
                3,
                "rust.rustc-lint".into(),
                "unused_variables".into()
            ),
            (
                "src/main.rs".into(),
                4,
                "rust.clippy".into(),
                "clippy::len_zero".into()
            ),
        ],
        "{result}"
    );

    // A lint the project denies fails the check.
    write(
        root,
        "src/main.rs",
        "#![deny(unused_variables)]\nfn main() {\n    let unused = 5;\n}\n",
    );
    let result = report(check(root).output().unwrap(), 1);
    assert_eq!(result["complete"], true, "{result}");
    assert_eq!(result["diagnostics"][0]["severity"], "error", "{result}");
}

#[test]
fn init_selects_go_and_rust_profiles_without_a_jvm_build() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    write(root, "go.mod", "module example.com/m\n");
    write(root, "Cargo.toml", "[package]\nname = \"m\"\n");
    let init = || {
        std::process::Command::new(env!("CARGO_BIN_EXE_dornguard"))
            .current_dir(root)
            .arg("init")
            .output()
            .unwrap()
    };
    assert!(init().status.success());
    assert_eq!(
        fs::read_to_string(root.join(".dornguard.yml")).unwrap(),
        "version: 1\nprofiles: [go, rust]\n"
    );

    fs::remove_file(root.join(".dornguard.yml")).unwrap();
    write(root, "pom.xml", "<project/>\n");
    assert!(init().status.success());
    assert_eq!(
        fs::read_to_string(root.join(".dornguard.yml")).unwrap(),
        "version: 1\nprofiles: [java]\n"
    );
}
