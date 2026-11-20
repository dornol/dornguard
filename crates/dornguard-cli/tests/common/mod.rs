#![allow(dead_code)]

use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::Mutex;
use tempfile::TempDir;

pub const CLEAN: &str =
    include_str!("../../../../examples/java-single/src/main/java/example/Example.java");

// These fixtures use the same Maven local repository. Concurrent downloads can
// collide on Windows when Maven replaces repository update markers.
pub static MAVEN_TEST_LOCK: Mutex<()> = Mutex::new(());

pub fn project(source: &str) -> TempDir {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join(".dornguard.yml"),
        include_str!("../../../../examples/java-single/.dornguard.yml"),
    )
    .unwrap();
    fs::create_dir_all(project.path().join("src")).unwrap();
    fs::write(project.path().join("src/Example.java"), source).unwrap();
    project
}

pub fn check(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dornguard"));
    command
        .current_dir(root)
        .args(["check", "--format", "json"]);
    command
}

pub fn report(output: Output, exit_code: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(exit_code),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("stdout should contain exactly one JSON report")
}

pub fn copy_gradle_fixture(source: &Path, destination: &Path) {
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if name == ".gradle" || name == ".kotlin" || name == "build" {
            continue;
        }
        let target = destination.join(name);
        if entry.file_type().unwrap().is_dir() {
            fs::create_dir_all(&target).unwrap();
            copy_gradle_fixture(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

pub fn commit_fixture(root: &Path) {
    git(root, &["init", "--quiet"]);
    git(root, &["add", "--", "."]);
    git(root, &["commit", "--quiet", "-m", "fixture"]);
}

pub fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args([
            "-c",
            "user.name=DornGuard Test",
            "-c",
            "user.email=dornguard@example.invalid",
            "-c",
            "commit.gpgSign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

pub fn write_custom_ruleset(root: &Path) {
    fs::create_dir_all(root).unwrap();
    fs::write(
        root.join("ruleset.yml"),
        "version: 1\nprofiles: [java.yml]\nrules: [stderr.yml]\n",
    )
    .unwrap();
    fs::write(root.join("java.yml"), "name: custom-java\ndescription: Custom Java checks\nextends: [java-source]\nrules: [custom.stderr]\n").unwrap();
    fs::write(root.join("stderr.yml"), "id: custom.stderr\nengine: semgrep\nseverity: error\ndescription: Avoid writing to stderr\nfix: Use the project logger\nimplementation: {config: stderr.semgrep.yml, rule_id: custom-stderr}\n").unwrap();
    fs::write(root.join("stderr.semgrep.yml"), "rules:\n  - id: custom-stderr\n    languages: [java]\n    message: Use the project logger\n    severity: ERROR\n    pattern: System.err.println(...)\n").unwrap();
}

pub fn unnecessary_import_lines(report: &Value, path: &str) -> Vec<u64> {
    report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| {
            finding["rule_id"] == "java.unnecessary-import" && finding["path"] == path
        })
        .map(|finding| finding["line"].as_u64().unwrap())
        .collect()
}
