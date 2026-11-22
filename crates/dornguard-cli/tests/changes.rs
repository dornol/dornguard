#![allow(clippy::unwrap_used)]
use serde_json::Value;
use std::fs;
use std::process::Command;

mod common;
use common::{CLEAN, check, commit_fixture, git, project, report};

#[test]
#[ignore = "requires Git"]
fn jspecify_changed_rechecks_types_when_package_marking_is_deleted() {
    let project = project("package example; class Example {}");
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [jspecify]\n",
    )
    .unwrap();
    let descriptor = root.join("src/package-info.java");
    fs::write(
        &descriptor,
        "@org.jspecify.annotations.NullMarked package example;",
    )
    .unwrap();
    commit_fixture(root);
    let clean = report(check(root).output().unwrap(), 0);
    assert_eq!(clean["files_scanned"], 2);

    fs::remove_file(descriptor).unwrap();
    let result = report(check(root).arg("--changed").output().unwrap(), 1);
    assert_eq!(result["scope"], "full");
    assert_eq!(
        result["fallback_reason"],
        "A Java package or module declaration changed"
    );
    assert_eq!(result["diagnostics"][0]["path"], "src/Example.java");
}

#[test]
#[ignore = "requires the pinned Semgrep version and Git"]
fn changed_uses_staged_unstaged_and_untracked_paths_with_worktree_contents() {
    let project = project(CLEAN);
    let root = project.path();
    commit_fixture(root);
    let clean = report(check(root).arg("--changed").output().unwrap(), 0);
    assert_eq!(clean["status"], "no_changes");
    assert_eq!(clean["scope"], "changed");

    let violation = CLEAN.replace("return 42;", "System.out.println(42); return 42;");
    let old_path = root.join("src/Example.java");
    fs::write(&old_path, &violation).unwrap();
    git(root, &["add", "src/Example.java"]);
    fs::write(old_path, CLEAN).unwrap();
    fs::write(root.join("src/Untracked.java"), &violation).unwrap();

    let changed = report(check(root).arg("--changed").output().unwrap(), 1);
    assert_eq!(changed["files_scanned"], 2);
    assert_eq!(changed["diagnostics"].as_array().unwrap().len(), 1);
    assert_eq!(changed["diagnostics"][0]["path"], "src/Untracked.java");

    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [java-source]\n# edited\n",
    )
    .unwrap();
    let fallback = report(check(root).arg("--changed").output().unwrap(), 1);
    assert_eq!(fallback["scope"], "full");
    assert!(
        fallback["fallback_reason"]
            .as_str()
            .unwrap()
            .contains(".dornguard.yml")
    );
}

#[test]
#[ignore = "requires the pinned Semgrep version on PATH"]
fn changed_base_scans_branch_changes_and_the_current_worktree() {
    let project = project(CLEAN);
    let root = project.path();
    let violation = CLEAN.replace("return 42;", "System.out.println(42); return 42;");
    fs::write(root.join("src/BaseOnly.java"), &violation).unwrap();
    fs::write(root.join("src/Staged.java"), CLEAN).unwrap();
    fs::write(root.join("src/Unstaged.java"), CLEAN).unwrap();
    commit_fixture(root);
    git(root, &["checkout", "--quiet", "-b", "pr-base"]);
    fs::write(root.join("src/BaseOnly.java"), CLEAN).unwrap();
    git(
        root,
        &["commit", "--quiet", "-am", "Fix violation on base only"],
    );
    git(root, &["checkout", "--quiet", "-b", "pr-topic", "pr-base^"]);
    fs::write(root.join("src/Topic.java"), &violation).unwrap();
    git(root, &["add", "src/Topic.java"]);
    git(root, &["commit", "--quiet", "-m", "Add a topic change"]);
    fs::write(root.join("src/Staged.java"), &violation).unwrap();
    git(root, &["add", "src/Staged.java"]);
    fs::write(root.join("src/Unstaged.java"), &violation).unwrap();
    fs::write(root.join("src/Untracked.java"), &violation).unwrap();

    let outcome = report(
        check(root)
            .args(["--changed", "--base", "pr-base"])
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(outcome["scope"], "changed");
    assert_eq!(outcome["files_scanned"], 4);
    let paths = outcome["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["path"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        paths,
        [
            "src/Staged.java",
            "src/Topic.java",
            "src/Unstaged.java",
            "src/Untracked.java"
        ]
    );

    let shallow = tempfile::tempdir().unwrap();
    let cloning = Command::new("git")
        .args([
            "-c",
            "protocol.file.allow=always",
            "clone",
            "--quiet",
            "--no-local",
            "--depth=1",
            "--no-single-branch",
            "--branch",
            "pr-topic",
        ])
        .arg(root)
        .arg(shallow.path())
        .output()
        .unwrap();
    assert!(
        cloning.status.success(),
        "{}",
        String::from_utf8_lossy(&cloning.stderr)
    );
    let fallback = report(
        check(shallow.path())
            .args(["--changed", "--base", "origin/pr-base"])
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(fallback["scope"], "full");
    assert!(
        fallback["fallback_reason"]
            .as_str()
            .unwrap()
            .contains("merge base")
    );

    let missing = report(
        check(root)
            .args(["--changed", "--base", "unknown-base"])
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(missing["scope"], "full");
    assert!(
        missing["fallback_reason"]
            .as_str()
            .unwrap()
            .contains("unknown-base")
    );
}

#[test]
#[ignore = "requires the pinned Semgrep version on PATH"]
fn changed_scans_unchanged_sources_when_the_selected_baseline_changes() {
    let violation = CLEAN.replace("return 42;", "System.out.println(42); return 42;");
    let project = project(&violation);
    let root = project.path();
    let baseline = "Custom Baseline.json";
    let create = Command::new(env!("CARGO_BIN_EXE_dornguard"))
        .current_dir(root)
        .args(["baseline", "create", "--output", baseline])
        .output()
        .unwrap();
    assert!(
        create.status.success(),
        "{}",
        String::from_utf8_lossy(&create.stderr)
    );
    commit_fixture(root);

    let check_changed = || {
        check(root)
            .args(["--changed", "--baseline", baseline])
            .output()
            .unwrap()
    };
    let unchanged = report(check_changed(), 0);
    assert_eq!(unchanged["status"], "no_changes", "{unchanged}");
    fs::write(root.join(baseline), r#"{"version":1,"violations":[]}"#).unwrap();

    let result = report(check_changed(), 1);
    assert_eq!(result["scope"], "full");
    assert_eq!(result["diagnostics"][0]["rule_id"], "java.stdout-usage");
    assert!(
        result["fallback_reason"]
            .as_str()
            .unwrap()
            .contains(baseline)
    );
}

#[test]
#[ignore = "requires the pinned Semgrep version on PATH"]
fn baseline_tracks_findings_across_line_moves_without_hiding_new_duplicates() {
    let violation = CLEAN.replace("return 42;", "System.out.println(42);\n        return 42;");
    let project = project(&violation);
    let root = project.path();
    let create = Command::new(env!("CARGO_BIN_EXE_dornguard"))
        .current_dir(root)
        .args(["baseline", "create"])
        .output()
        .unwrap();
    assert!(
        create.status.success(),
        "{}",
        String::from_utf8_lossy(&create.stderr)
    );

    let baseline = root.join(".dornguard-baseline.json");
    let mut legacy: Value = serde_json::from_slice(&fs::read(&baseline).unwrap()).unwrap();
    legacy["version"] = 1.into();
    fs::write(baseline, serde_json::to_vec(&legacy).unwrap()).unwrap();

    let file = root.join("src/Example.java");
    fs::write(&file, format!("\n\n{violation}")).unwrap();
    let result = report(
        check(root)
            .args(["--baseline", ".dornguard-baseline.json"])
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(result["suppressed_count"], 1);

    fs::write(
        &file,
        violation.replace("return 42;", "System.out.println(42);\n        return 42;"),
    )
    .unwrap();
    let result = report(
        check(root)
            .args(["--baseline", ".dornguard-baseline.json"])
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(result["suppressed_count"], 1);
    assert_eq!(result["diagnostics"].as_array().unwrap().len(), 1);

    let sarif = Command::new(env!("CARGO_BIN_EXE_dornguard"))
        .current_dir(root)
        .args(["check", "--format", "sarif"])
        .output()
        .unwrap();
    let sarif = report(sarif, 1);
    assert_eq!(sarif["version"], "2.1.0");
    assert_eq!(
        sarif["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["artifactLocation"]["uri"],
        "src/Example.java"
    );
    assert!(
        sarif["runs"][0]["results"][0]["partialFingerprints"]["dornguardFingerprint/v1"]
            .is_string()
    );

    let agent = Command::new(env!("CARGO_BIN_EXE_dornguard"))
        .current_dir(root)
        .args(["check", "--format", "agent"])
        .output()
        .unwrap();
    assert_eq!(agent.status.code(), Some(1));
    let output = String::from_utf8(agent.stdout).unwrap();
    assert!(output.contains("2 errors, 0 warnings"));
    assert!(output.contains("Fix: Use the project logger."));
}

#[test]
#[ignore = "requires Git and the pinned Semgrep version"]
fn intentional_exceptions_survive_line_moves_and_keep_org_findings() {
    let source = "class Example {
    void run() {
        System.out.println(42);
        System.out.println(42);
        System.out.println(43);
        System.out.println(43);
    }
}
";
    let project = project(source);
    let root = project.path();
    let initial = report(check(root).output().unwrap(), 1);
    let fingerprint = initial["diagnostics"][2]["fingerprint"].as_str().unwrap();
    fs::write(
        root.join(".dornguard.yml"),
        format!(
            "version: 1\nprofiles: [java-source]\nignore:\n  - path: src/Example.java\n    rule: java.stdout-usage\n    fingerprint: '{fingerprint}'\n    reason: Script output\n"
        ),
    )
    .unwrap();
    commit_fixture(root);

    let moved = "
class Example {
    void run() {
        System.out.println(42);
        // dornguard-ignore java.stdout-usage — Script output
        System.out.println(42);
        System.out.println(43);
        System.out.println(43);
    }
}
";
    fs::write(root.join("src/Example.java"), moved).unwrap();
    let local = report(check(root).arg("--changed").output().unwrap(), 1);
    assert_eq!(local["scope"], "changed");
    assert_eq!(local["files_scanned"], 1);
    assert_eq!(local["suppressed_count"], 2);
    let lines = |report: &Value| {
        report["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|finding| finding["line"].as_u64().unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(lines(&local), [4, 8]);

    let authority = tempfile::tempdir().unwrap();
    let policy = authority.path().join("required.yml");
    fs::write(&policy, "version: 1\nprofiles: [java-source]\n").unwrap();
    let required = report(
        check(root)
            .arg("--organization-policy")
            .arg(policy)
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(required["organization_policy_enforced"], true);
    assert_eq!(lines(&required), [4, 6, 7, 8]);
}

#[test]
fn malformed_ignore_entries_are_configuration_errors() {
    let project = project(CLEAN);
    let root = project.path();
    let valid_fingerprint = "f".repeat(64);
    for (path, rule, fingerprint, reason, error) in [
        (
            "../outside.java",
            "java.stdout-usage",
            valid_fingerprint.as_str(),
            "purpose",
            "project-relative",
        ),
        (
            "src/Example.java",
            "java.stdout-usage",
            "a",
            "purpose",
            "SHA-256",
        ),
        (
            "src/Example.java",
            "java.stdout-usage",
            valid_fingerprint.as_str(),
            "  ",
            "requires a reason",
        ),
        (
            "src/Example.java",
            "unknown",
            valid_fingerprint.as_str(),
            "purpose",
            "Unknown rule",
        ),
    ] {
        fs::write(
            root.join(".dornguard.yml"),
            format!(
                "version: 1\nprofiles: [java-source]\nignore:\n  - path: '{path}'\n    rule: {rule}\n    fingerprint: '{fingerprint}'\n    reason: '{reason}'\n"
            ),
        )
        .unwrap();
        let result = report(check(root).output().unwrap(), 2);
        assert!(
            result["failures"][0]["message"]
                .as_str()
                .unwrap()
                .contains(error),
            "{result}"
        );
    }

    fs::write(
        root.join(".dornguard.yml"),
        format!(
            "version: 1\nprofiles: [java-source]\nignore:\n  - path: src/Example.java\n    fingerprint: '{valid_fingerprint}'\n    reason: Require a rule\n"
        ),
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 2);
    assert!(
        result["failures"][0]["message"]
            .as_str()
            .unwrap()
            .contains("requires a rule")
    );
}

#[test]
#[ignore = "requires the pinned Semgrep and PMD engines"]
fn pmd_baseline_uses_exclusive_fingerprint_and_matches_legacy_entries() {
    let source = r#"import java.io.InputStream;
import java.io.IOException;
import java.nio.file.Files;
import java.nio.file.Path;
class Example {
    void read(Path path) throws IOException {
        InputStream stream=Files.newInputStream(path);
        stream.available();
    }
}
"#;
    let project = project(source);
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [java]\nrules:\n  java.resource-not-closed: {severity: error}\n",
    )
    .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_dornguard"))
        .current_dir(root)
        .args(["baseline", "create"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let baseline = root.join(".dornguard-baseline.json");
    let mut existing: Value = serde_json::from_slice(&fs::read(&baseline).unwrap()).unwrap();
    assert_eq!(
        existing["violations"][0]["fingerprint"],
        dornguard_core::fingerprint("stream")
    );
    fs::write(
        root.join("src/Example.java"),
        source.replace("stream=", "stream = "),
    )
    .unwrap();
    let matched = || {
        report(
            check(root)
                .arg("--baseline")
                .arg(&baseline)
                .output()
                .unwrap(),
            0,
        )
    };
    assert_eq!(matched()["suppressed_count"], 1);
    existing["violations"][0]["fingerprint"] = Value::from(dornguard_core::fingerprint("stream="));
    fs::write(&baseline, serde_json::to_vec(&existing).unwrap()).unwrap();
    assert_eq!(matched()["suppressed_count"], 1);
}

#[test]
fn jpa_entity_change_rechecks_unchanged_controllers() {
    let project = project(
        r#"import example.model.Book;
@org.springframework.web.bind.annotation.RestController
class Example {
    @org.springframework.web.bind.annotation.GetMapping
    Book read() { return null; }
}
"#,
    );
    let root = project.path();
    let model = root.join("src/Book.java");
    fs::write(&model, "package example.model; public class Book {}\n").unwrap();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [jpa-web]\n",
    )
    .unwrap();
    commit_fixture(root);
    assert!(
        report(check(root).output().unwrap(), 0)["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    fs::write(
        model,
        "package example.model; @jakarta.persistence.Entity public class Book {}\n",
    )
    .unwrap();
    let changed = report(check(root).arg("--changed").output().unwrap(), 0);
    assert_eq!(changed["scope"], "full");
    assert_eq!(changed["diagnostics"][0]["path"], "src/Example.java");
    assert_eq!(
        changed["diagnostics"][0]["rule_id"],
        "spring.jpa-entity-response"
    );
}

#[test]
fn jpa_profile_keeps_changed_scope_when_no_persistence_type_changes() {
    let project = project("class Example {}\n");
    let root = project.path();
    let helper = root.join("src/Helper.java");
    fs::write(&helper, "class Helper {}\n").unwrap();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [jpa-web]\n",
    )
    .unwrap();
    commit_fixture(root);
    fs::write(&helper, "class Helper { int value; }\n").unwrap();
    let changed = report(check(root).arg("--changed").output().unwrap(), 0);
    assert_eq!(changed["scope"], "changed", "{changed}");

    fs::remove_file(&helper).unwrap();
    let deleted = report(check(root).arg("--changed").output().unwrap(), 0);
    assert_eq!(deleted["scope"], "full", "{deleted}");
}

#[test]
fn flyway_compares_existing_versioned_migrations_against_git() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    let dir = root.join("app/src/main/resources/db/migration");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [flyway]\n",
    )
    .unwrap();
    fs::write(dir.join("V1__initial.sql"), "select 1;\n").unwrap();
    fs::write(dir.join("V2_1__rename.sql"), "select 2;\n").unwrap();
    fs::write(dir.join("R__view.sql"), "select 1;\n").unwrap();
    commit_fixture(root);
    git(root, &["branch", "pr-base"]);
    fs::write(dir.join("V1__initial.sql"), "select 10;\n").unwrap();
    fs::write(dir.join("R__view.sql"), "select 9;\n").unwrap();
    fs::write(dir.join("V3__new.sql"), "select 3;\n").unwrap();
    fs::rename(dir.join("V2_1__rename.sql"), dir.join("V2_1__renamed.sql")).unwrap();
    let local = report(check(root).output().unwrap(), 0);
    let list = local["diagnostics"].as_array().unwrap();
    assert_eq!(list.len(), 2, "{local}");
    assert!(
        list[0]["path"]
            .as_str()
            .unwrap()
            .ends_with("V1__initial.sql")
    );
    assert!(list[1]["message"].as_str().unwrap().contains("deleted"));
    git(root, &["add", "-A"]);
    git(root, &["commit", "-m", "Change migrations"]);
    assert!(
        report(check(root).output().unwrap(), 0)["diagnostics"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let compared = report(
        check(root)
            .args(["--changed", "--base", "pr-base"])
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(compared["scope"], "changed");
    assert_eq!(compared["diagnostics"], local["diagnostics"]);
    let missing_base = report(
        check(root)
            .args(["--changed", "--base", "missing-ref"])
            .output()
            .unwrap(),
        4,
    );
    assert_eq!(
        missing_base["failures"][0]["rule_ids"][0],
        "flyway.changed-versioned-migration"
    );
}

#[test]
fn ignore_paths_suppress_findings_by_file_folder_and_rule() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    for path in [
        "manual.js",
        "vendor/nested/library.js",
        "legacy/example.js",
        "legacy/nested/example.js",
        "legacy-other/example.js",
    ] {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "if(true) {\n   run();\n}\n").unwrap();
    }
    let config = root.join(".dornguard.yml");
    fs::write(
        &config,
        "version: 1\nprofiles: [format]\nrules: {format.indentation: {enabled: true}}\n",
    )
    .unwrap();
    let before = report(check(root).output().unwrap(), 1);
    assert_eq!(before["diagnostics"].as_array().unwrap().len(), 10);

    fs::write(
        &config,
        r#"version: 1
profiles: [format]
rules:
  format.indentation: {enabled: true}
ignore:
  - path: manual.js
    reason: Hand-maintained compatibility script
  - path: vendor/
    reason: Upstream sources
  - path: "legacy/**"
    rule: format.indentation
    reason: Preserve existing indentation
"#,
    )
    .unwrap();
    let local = report(check(root).output().unwrap(), 1);
    assert_eq!(local["files_scanned"], 5);
    assert_eq!(local["suppressed_count"], 6);
    let findings = local["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                item["path"].as_str().unwrap(),
                item["rule_id"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        findings,
        [
            ("legacy-other/example.js", "format.control-spacing"),
            ("legacy-other/example.js", "format.indentation"),
            ("legacy/example.js", "format.control-spacing"),
            ("legacy/nested/example.js", "format.control-spacing"),
        ]
    );

    let authority = tempfile::tempdir().unwrap();
    let policy = authority.path().join("required.yml");
    fs::write(
        &policy,
        "version: 1\nprofiles: [format]\nrules: {format.indentation: {enabled: true}}\n",
    )
    .unwrap();
    let required = report(
        check(root)
            .arg("--organization-policy")
            .arg(policy)
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(required["organization_policy_enforced"], true);
    assert_eq!(required["diagnostics"], before["diagnostics"]);

    fs::write(root.join("vendor/broken.js"), [0xff]).unwrap();
    let incomplete = report(check(root).output().unwrap(), 3);
    assert_eq!(incomplete["complete"], false);
    assert_eq!(incomplete["files_requested"], 6);

    let previous_config = fs::read_to_string(&config).unwrap();
    fs::write(
        &config,
        format!("{previous_config}exclude: ['vendor/**']\n"),
    )
    .unwrap();
    let excluded = report(check(root).output().unwrap(), 1);
    assert_eq!(excluded["complete"], true);
    assert_eq!(excluded["files_requested"], 4);
    assert_eq!(excluded["diagnostics"], local["diagnostics"]);
}

#[test]
fn tracked_changes_are_checked_even_when_their_directory_is_gitignored() {
    let project = project("class Example {}\n");
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [format]\nrules: {format.indentation: {enabled: true}}\n",
    )
    .unwrap();
    commit_fixture(root);
    fs::write(root.join(".gitignore"), "src/\n").unwrap();
    git(root, &["add", ".gitignore"]);
    git(
        root,
        &["commit", "--quiet", "-m", "Ignore untracked sources"],
    );
    fs::write(root.join("src/Example.java"), "class Example {}").unwrap();
    fs::write(root.join("src/Untracked.java"), "class Untracked {}").unwrap();

    for arguments in [&[][..], &["--changed"][..], &["src/Example.java"][..]] {
        let result = report(check(root).args(arguments).output().unwrap(), 1);
        assert_eq!(result["files_requested"], 1, "{result}");
        assert_eq!(
            result["diagnostics"][0]["path"], "src/Example.java",
            "{result}"
        );
        assert_eq!(result["diagnostics"][0]["rule_id"], "format.final-newline");
    }
}

#[test]
#[ignore = "requires the pinned Semgrep version and Git"]
fn strict_changed_reports_baselined_findings_only_in_edited_files() {
    let violation = CLEAN.replace("return 42;", "System.out.println(42); return 42;");
    let project = project(&violation);
    let root = project.path();
    fs::write(
        root.join("src/Other.java"),
        violation.replace("Example", "Other"),
    )
    .unwrap();
    let create = Command::new(env!("CARGO_BIN_EXE_dornguard"))
        .current_dir(root)
        .args(["baseline", "create"])
        .output()
        .unwrap();
    assert!(create.status.success());
    commit_fixture(root);

    fs::write(
        root.join("src/Example.java"),
        format!("{violation}\n// touched\n"),
    )
    .unwrap();
    let args = ["--changed", "--baseline", ".dornguard-baseline.json"];
    let lenient = report(check(root).args(args).output().unwrap(), 0);
    assert_eq!(lenient["suppressed_count"], 1, "{lenient}");

    let strict = report(
        check(root)
            .args(args)
            .arg("--strict-changed")
            .output()
            .unwrap(),
        1,
    );
    let diagnostics = strict["diagnostics"].as_array().unwrap();
    assert_eq!(diagnostics.len(), 1, "{strict}");
    assert_eq!(diagnostics[0]["path"], "src/Example.java");
    assert!(
        strict["analysis_notes"]
            .to_string()
            .contains("1 baselined findings in changed files")
    );
}

#[test]
#[ignore = "requires the pinned Semgrep version on PATH"]
fn ratchet_fails_when_the_baseline_records_findings_that_no_longer_occur() {
    let violation = CLEAN.replace("return 42;", "System.out.println(42); return 42;");
    let project = project(&violation);
    let root = project.path();
    let create = Command::new(env!("CARGO_BIN_EXE_dornguard"))
        .current_dir(root)
        .args(["baseline", "create"])
        .output()
        .unwrap();
    assert!(create.status.success());
    let args = ["--baseline", ".dornguard-baseline.json"];

    let current = report(check(root).args(args).arg("--ratchet").output().unwrap(), 0);
    assert_eq!(current["suppressed_count"], 1, "{current}");

    fs::write(root.join("src/Example.java"), CLEAN).unwrap();
    let note = report(check(root).args(args).output().unwrap(), 0);
    assert!(
        note["analysis_notes"]
            .to_string()
            .contains("records 1 findings that no longer occur"),
        "{note}"
    );
    let failed = report(check(root).args(args).arg("--ratchet").output().unwrap(), 2);
    assert!(
        failed["failures"][0]["message"]
            .as_str()
            .unwrap()
            .contains("baseline create --force")
    );
}

#[test]
#[ignore = "requires the pinned Semgrep version on PATH"]
fn full_scan_notes_suppressions_that_no_longer_match_a_finding() {
    let source = "
class Example {
    void run() {
        // dornguard-ignore java.stdout-usage -- Script output
        System.out.println(42);
        // dornguard-ignore java.stdout-usage -- Obsolete
        int unused = 1;
    }
}
";
    let project = project(source);
    let root = project.path();
    let stale = "e".repeat(64);
    fs::write(
        root.join(".dornguard.yml"),
        format!(
            "version: 1\nprofiles: [java-source]\nignore:\n  - path: src/Gone.java\n    rule: java.stdout-usage\n    reason: Removed file\n  - path: src/Example.java\n    rule: java.stdout-usage\n    fingerprint: '{stale}'\n    reason: Old finding\n  - path: generated/\n    reason: Generated code\n"
        ),
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 0);
    assert_eq!(result["suppressed_count"], 1, "{result}");
    let notes = result["analysis_notes"].to_string();
    assert!(
        notes.contains("3 suppressions matched no finding"),
        "{notes}"
    );
    assert!(
        notes.contains("comment for java.stdout-usage at src/Example.java:6"),
        "{notes}"
    );
    assert!(
        notes.contains("ignore entry for java.stdout-usage in src/Gone.java"),
        "{notes}"
    );
    assert!(
        notes.contains("ignore entry for java.stdout-usage in src/Example.java"),
        "{notes}"
    );
    assert!(!notes.contains("generated"), "{notes}");

    // A partial scan cannot tell whether a suppression is obsolete.
    let selected = report(check(root).arg("src/Example.java").output().unwrap(), 0);
    assert!(
        !selected["analysis_notes"]
            .to_string()
            .contains("matched no finding"),
        "{selected}"
    );
}
