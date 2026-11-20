#![allow(clippy::unwrap_used)]
use serde_json::Value;
use std::fs;
use std::process::Command;

mod common;
use common::{CLEAN, check, project, report};

#[test]
fn invalid_config_and_missing_targets_have_distinct_statuses() {
    let project = project(CLEAN);
    let root = project.path();
    let result = report(
        check(root).args(["--profile", "unknown"]).output().unwrap(),
        2,
    );
    assert_eq!(result["status"], "configuration_error");

    fs::remove_file(root.join("src/Example.java")).unwrap();
    let result = report(check(root).output().unwrap(), 4);
    assert_eq!(result["status"], "preparation_error");
}

#[cfg(unix)]
#[test]
fn missing_engine_is_reported_as_an_incomplete_check() {
    let project = project("class Example {} \n");
    fs::write(
        project.path().join(".dornguard.yml"),
        "version: 1\nprofiles: [java-source, format]\n",
    )
    .unwrap();
    let empty_path = tempfile::tempdir().unwrap();
    let result = report(
        check(project.path())
            .env("PATH", empty_path.path())
            .env("DORNGUARD_ENGINE_DIR", empty_path.path())
            .output()
            .unwrap(),
        3,
    );
    assert_eq!(result["status"], "engine_error");
    assert_eq!(result["complete"], false);
    assert_eq!(result["failures"][0]["engine"], "semgrep");
    assert_eq!(result["failures"][0]["stage"], "engine");
    assert_eq!(result["failures"][0]["file_count"], 1);
    assert!(
        result["failures"][0]["rule_ids"]
            .as_array()
            .unwrap()
            .contains(&Value::from("java.stdout-usage"))
    );
    let text = Command::new(env!("CARGO_BIN_EXE_dornguard"))
        .current_dir(project.path())
        .env("PATH", empty_path.path())
        .env("DORNGUARD_ENGINE_DIR", empty_path.path())
        .arg("check")
        .output()
        .unwrap();
    assert_eq!(text.status.code(), Some(3));
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(text.contains("ENGINE ERROR"));
    assert!(
        text.find("Rules with incomplete results").unwrap()
            < text.find("error[format.trailing-whitespace]").unwrap(),
        "{text}"
    );
}

#[test]
#[ignore = "requires the pinned Semgrep version on PATH"]
fn real_semgrep_passes_flags_violations_and_rejects_analysis_errors() {
    let project = project(CLEAN);
    let root = project.path();
    let result = report(check(root).output().unwrap(), 0);
    assert_eq!(result["status"], "passed");
    assert_eq!(result["files_scanned"], 1);

    let source = root.join("src/Example.java");
    fs::write(
        &source,
        CLEAN.replace(
            "return 42;",
            "System.out.println(42); // nosemgrep\n        return 42;",
        ),
    )
    .unwrap();
    let result = report(check(root).arg("src/Example.java").output().unwrap(), 1);
    assert_eq!(result["complete"], true);
    assert_eq!(result["diagnostics"][0]["path"], "src/Example.java");
    assert_eq!(result["diagnostics"][0]["rule_id"], "java.stdout-usage");
    assert_eq!(result["diagnostics"][0]["severity"], "error");
    assert_eq!(result["diagnostics"][0]["line"], 5);

    fs::write(
        source,
        "public class Example { void run() { System.out.println(1); @@@ } }\n",
    )
    .unwrap();
    fs::write(root.join("src/Additional.java"), "class Additional {}\n").unwrap();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [java]\n",
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 3);
    assert_eq!(result["status"], "engine_error");
    assert_eq!(result["complete"], false);
    assert_eq!(result["files_requested"], 2);
    assert_eq!(result["files_scanned"], 1);
    for (engine, kind) in [("semgrep", "PartialParsing"), ("pmd", "ParseException")] {
        assert!(
            result["failures"]
                .as_array()
                .unwrap()
                .iter()
                .any(|failure| failure["engine"] == engine
                    && failure["path"] == "src/Example.java"
                    && failure["error_type"] == kind),
            "{result}"
        );
    }
    let output = Command::new(env!("CARGO_BIN_EXE_dornguard"))
        .current_dir(root)
        .args(["check", "--format", "sarif"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(3));
    let sarif: Value = serde_json::from_slice(&output.stdout).unwrap();
    let run = &sarif["runs"][0];
    assert_eq!(run["properties"]["filesRequested"], 2);
    assert_eq!(run["properties"]["filesScanned"], 1);
    assert!(
        run["invocations"][0]["toolExecutionNotifications"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |notification| notification["descriptor"]["id"] == "PartialParsing"
                    && notification["locations"][0]["physicalLocation"]["artifactLocation"]["uri"]
                        == "src/Example.java"
            ),
        "{sarif}"
    );
}

#[cfg(unix)]
#[test]
fn incorrect_or_missing_engine_versions_are_reported() {
    use std::os::unix::fs::PermissionsExt;

    let project = project(CLEAN);
    let binaries = tempfile::tempdir().unwrap();
    let semgrep = binaries.path().join("semgrep");
    for (command, message) in [
        ("printf '0.0.0\\n'", "Unsupported semgrep version"),
        ("exit 0", "semgrep did not report a version"),
    ] {
        fs::write(&semgrep, format!("#!/bin/sh\n{command}\n")).unwrap();
        fs::set_permissions(&semgrep, fs::Permissions::from_mode(0o755)).unwrap();
        let result = report(
            check(project.path())
                .env("PATH", binaries.path())
                .env("DORNGUARD_ENGINE_DIR", binaries.path())
                .output()
                .unwrap(),
            3,
        );
        assert!(
            result["failures"][0]["message"]
                .as_str()
                .unwrap()
                .contains(message)
        );
    }
}

#[test]
#[ignore = "requires Java 21 and the bundled JVM artifacts"]
fn doctor_validates_the_archunit_runner_for_active_profiles() {
    let project = project(CLEAN);
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [layered]\n",
    )
    .unwrap();
    let doctor = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dornguard"));
        command.current_dir(root).arg("doctor");
        command
    };
    let valid = doctor().output().unwrap();
    assert!(
        valid.status.success(),
        "{}",
        String::from_utf8_lossy(&valid.stderr)
    );
    assert!(String::from_utf8_lossy(&valid.stdout).contains("ArchUnit runner OK (protocol"));
    let wrong = doctor()
        .env("DORNGUARD_ARCHUNIT_RUNNER", root.join("missing-runner.jar"))
        .output()
        .unwrap();
    assert_eq!(wrong.status.code(), Some(3));
    assert!(String::from_utf8_lossy(&wrong.stderr).contains("missing-runner.jar"));
    let invalid_jar = root.join("invalid-runner.jar");
    fs::write(&invalid_jar, "invalid JVM archive").unwrap();
    let wrong = doctor()
        .env("DORNGUARD_ARCHUNIT_RUNNER", invalid_jar)
        .output()
        .unwrap();
    assert_eq!(wrong.status.code(), Some(3));
}

#[test]
fn agent_preview_spreads_findings_across_rules_and_filters_only_the_report() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [format]\n",
    )
    .unwrap();
    fs::write(root.join("a.js"), "if(true) {}\n".repeat(25)).unwrap();
    fs::write(root.join("z.js"), "run();").unwrap();

    let agent = Command::new(env!("CARGO_BIN_EXE_dornguard"))
        .current_dir(root)
        .args(["check", "--format", "agent"])
        .output()
        .unwrap();
    assert_eq!(agent.status.code(), Some(1));
    let text = String::from_utf8(agent.stdout).unwrap();
    assert!(
        text.contains("z.js:1:7: error[format.final-newline]"),
        "{text}"
    );
    assert_eq!(text.matches("error[format.control-spacing]").count(), 19);

    let focused = report(
        check(root)
            .args([
                "--report-rule",
                "format.final-newline",
                "--report-path",
                "./z.js",
            ])
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(focused["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(focused["diagnostics"][0]["path"], "z.js");
    assert_eq!(
        focused["rule_counts"]["format.control-spacing"]["errors"],
        25
    );
    assert_eq!(focused["report_filter"]["omitted"], 25);
    let empty = report(
        check(root)
            .args(["--report-path", "elsewhere"])
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(empty["diagnostics"].as_array().unwrap().len(), 0);
    assert_eq!(empty["report_filter"]["omitted"], 26);
}
