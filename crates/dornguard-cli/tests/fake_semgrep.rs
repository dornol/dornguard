//! Runs `dornguard check` against a scripted stand-in for Semgrep so the
//! engine integration (version gate, JSON normalization, failure reporting)
//! is covered without installing the real engine.
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::{Value, json};
use tempfile::TempDir;

const SEMGREP_VERSION: &str = "1.182.0";
const SOURCE: &str = "class Example {\n  void f() { System.out.println(1); }\n}\n";

// Answers `--version`, and for `scan` reports every requested file as scanned
// plus whatever results or errors the test put in the environment.
const FAKE_SEMGREP: &str = r#"#!/bin/sh
for arg in "$@"; do
  if [ "$arg" = "--version" ]; then
    echo "${FAKE_SEMGREP_VERSION}"
    exit 0
  fi
done
scanned=""
after=0
for arg in "$@"; do
  if [ "$after" = 1 ]; then
    scanned="${scanned}${scanned:+,}\"$arg\""
  elif [ "$arg" = "--" ]; then
    after=1
  fi
done
if [ -n "${FAKE_SEMGREP_RAW:-}" ]; then
  printf '%s' "$FAKE_SEMGREP_RAW"
  exit 0
fi
printf '{"paths":{"scanned":[%s]},"results":%s,"errors":%s,"skipped_rules":[]}' \
  "$scanned" "${FAKE_SEMGREP_RESULTS:-[]}" "${FAKE_SEMGREP_ERRORS:-[]}"
"#;

struct Fixture {
    project: TempDir,
    bin: TempDir,
    engines: TempDir,
}

fn fixture() -> Fixture {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join(".dornguard.yml"),
        "version: 1\nprofiles: [java-source]\n",
    )
    .unwrap();
    fs::create_dir_all(project.path().join("src")).unwrap();
    fs::write(project.path().join("src/Example.java"), SOURCE).unwrap();

    let bin = tempfile::tempdir().unwrap();
    let script = bin.path().join("semgrep");
    fs::write(&script, FAKE_SEMGREP).unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    Fixture {
        project,
        bin,
        engines: tempfile::tempdir().unwrap(),
    }
}

impl Fixture {
    fn check(&self, envs: &[(&str, &str)]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dornguard"));
        command
            .current_dir(self.project.path())
            .args(["check", "--format", "json"])
            // Keep a managed or system Semgrep from shadowing the fake one.
            .env("DORNGUARD_ENGINE_DIR", self.engines.path())
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", display(self.bin.path())),
            )
            .env("FAKE_SEMGREP_VERSION", SEMGREP_VERSION);
        for (key, value) in envs {
            command.env(key, value);
        }
        command.output().unwrap()
    }
}

fn display(path: &Path) -> String {
    path.to_str().unwrap().to_owned()
}

fn report(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "invalid report ({error})\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )
    })
}

fn stdout_finding() -> String {
    let start = SOURCE.find("System").unwrap();
    let end = start + "System.out.println(1)".len();
    json!([{
        "check_id": "java-stdout-usage",
        "path": "src/Example.java",
        "start": { "line": 2, "col": 14, "offset": start },
        "end": { "line": 2, "col": 34, "offset": end },
    }])
    .to_string()
}

fn failure_text(report: &Value) -> String {
    report["failures"].to_string()
}

#[test]
fn clean_scan_passes() {
    let fixture = fixture();
    let output = fixture.check(&[]);
    assert_eq!(output.status.code(), Some(0));
    let result = report(&output);
    assert_eq!(result["status"], "passed");
    assert_eq!(result["files_scanned"], 1);
}

#[test]
fn reported_finding_becomes_a_located_diagnostic() {
    let fixture = fixture();
    let output = fixture.check(&[("FAKE_SEMGREP_RESULTS", &stdout_finding())]);
    assert_eq!(output.status.code(), Some(1));
    let result = report(&output);
    let diagnostic = &result["diagnostics"][0];
    assert_eq!(diagnostic["rule_id"], "java.stdout-usage");
    assert_eq!(diagnostic["engine"], "semgrep");
    assert_eq!(diagnostic["path"], "src/Example.java");
    assert_eq!(diagnostic["line"], 2);
    assert_eq!(diagnostic["column"], 14);
}

#[test]
fn unsupported_engine_version_is_an_engine_failure() {
    let fixture = fixture();
    let output = fixture.check(&[("FAKE_SEMGREP_VERSION", "0.0.1")]);
    assert_ne!(output.status.code(), Some(0));
    assert_ne!(report(&output)["status"], "passed");
    assert!(failure_text(&report(&output)).contains("Unsupported semgrep version"));
}

#[test]
fn invalid_json_is_never_treated_as_a_clean_scan() {
    let fixture = fixture();
    let output = fixture.check(&[("FAKE_SEMGREP_RAW", "not json")]);
    assert_ne!(output.status.code(), Some(0));
    assert!(failure_text(&report(&output)).contains("missing or invalid JSON"));
}

#[test]
fn unregistered_rule_id_is_rejected() {
    let fixture = fixture();
    let results = stdout_finding().replace("java-stdout-usage", "not-a-dornguard-rule");
    let output = fixture.check(&[("FAKE_SEMGREP_RESULTS", &results)]);
    assert_ne!(output.status.code(), Some(0));
    assert!(failure_text(&report(&output)).contains("unregistered rule ID"));
}

#[test]
fn files_the_engine_did_not_scan_fail_the_run() {
    let fixture = fixture();
    let raw = r#"{"paths":{"scanned":[]},"results":[],"errors":[],"skipped_rules":[]}"#;
    let output = fixture.check(&[("FAKE_SEMGREP_RAW", raw)]);
    assert_ne!(output.status.code(), Some(0));
    assert!(failure_text(&report(&output)).contains("skipped 1 required files"));
}
