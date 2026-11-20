#![allow(clippy::unwrap_used)]
use serde_json::Value;
use std::fs;
use std::process::Command;

mod common;
use common::{CLEAN, check, commit_fixture, git, project, report, write_custom_ruleset};

#[test]
#[ignore = "requires the pinned Semgrep version and Git"]
fn local_and_git_rulesets_resolve_custom_profiles_and_reuse_cached_tag_commits() {
    let source = CLEAN.replace("return 42;", "System.err.println(42); return 42;");
    let project = project(&source);
    let root = project.path();
    let policy = root.join(".policy");
    write_custom_ruleset(&policy);
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nruleset: {source: local, path: .policy}\nprofiles: [custom-java]\n",
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 1);
    assert_eq!(result["diagnostics"][0]["rule_id"], "custom.stderr");
    let explained = Command::new(env!("CARGO_BIN_EXE_dornguard"))
        .current_dir(root)
        .args(["rules", "explain", "custom.stderr"])
        .output()
        .unwrap();
    assert!(
        explained.status.success(),
        "{}",
        String::from_utf8_lossy(&explained.stderr)
    );

    let remote = tempfile::tempdir().unwrap();
    write_custom_ruleset(remote.path());
    commit_fixture(remote.path());
    git(remote.path(), &["tag", "v1"]);
    let configuration = format!(
        "version: 1\nruleset:\n  source: git\n  repository: {}\n  ref: v1\nprofiles: [custom-java]\n",
        serde_json::to_string(&remote.path()).unwrap()
    );
    fs::write(root.join(".dornguard.yml"), configuration).unwrap();
    let cache = tempfile::tempdir().unwrap();

    let result = report(
        check(root)
            .env("DORNGUARD_CACHE_DIR", cache.path())
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(result["diagnostics"][0]["rule_id"], "custom.stderr");
    let locked: Value =
        serde_json::from_slice(&fs::read(root.join(".dornguard-ruleset.lock.json")).unwrap())
            .unwrap();
    assert_eq!(locked["commit"].as_str().unwrap().len(), 40);
    drop(remote);

    let cached = report(
        check(root)
            .env("DORNGUARD_CACHE_DIR", cache.path())
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(cached["diagnostics"], result["diagnostics"]);
}

#[test]
#[ignore = "requires the pinned Semgrep version on PATH"]
fn outside_organization_policy_keeps_ignored_and_baselined_rules_blocking() {
    let violation = CLEAN.replace("return 42;", "System.out.println(42); return 42;");
    let project = project(&violation);
    let root = project.path();
    fs::write(root.join("src/Clean.java"), CLEAN).unwrap();
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
    fs::write(root.join(".gitignore"), "src/Example.java\n").unwrap();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [java-source]\nrules:\n  java.stdout-usage: {severity: warning}\n",
    )
    .unwrap();

    let authority = tempfile::tempdir().unwrap();
    let policy = authority.path().join("required.yml");
    fs::write(&policy, "version: 1\nprofiles: [java-source]\n").unwrap();
    let result = report(
        check(root)
            .args([
                "src/Clean.java",
                "--baseline",
                ".dornguard-baseline.json",
                "--organization-policy",
            ])
            .arg(&policy)
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(result["organization_policy_enforced"], true);
    assert_eq!(result["diagnostics"][0]["path"], "src/Example.java");
    assert_eq!(result["diagnostics"][0]["severity"], "error");

    let untrusted = root.join("required.yml");
    fs::copy(&policy, &untrusted).unwrap();
    let result = report(
        check(root)
            .arg("--organization-policy")
            .arg(untrusted)
            .output()
            .unwrap(),
        2,
    );
    assert_eq!(result["organization_policy_enforced"], false);
}

#[test]
fn organization_policy_reports_its_full_scope_alongside_changed_scope() {
    let project = project("class Example {}\n");
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [format]\n",
    )
    .unwrap();
    commit_fixture(root);
    let authority = tempfile::tempdir().unwrap();
    let policy = authority.path().join("required.yml");
    fs::write(&policy, "version: 1\nprofiles: [format]\n").unwrap();
    let run = || {
        check(root)
            .arg("--changed")
            .arg("--organization-policy")
            .arg(&policy)
            .output()
            .unwrap()
    };
    let clean = report(run(), 0);
    assert_eq!(clean["status"], "passed");
    assert_eq!(clean["scope"], "full");
    assert_eq!(clean["project_scope"], "changed");
    assert_eq!(clean["organization_scope"], "full");
    assert_eq!(clean["files_scanned"], 1);

    fs::write(root.join("src/Example.java"), "class Example {}").unwrap();
    git(root, &["add", "src/Example.java"]);
    git(
        root,
        &[
            "commit",
            "--quiet",
            "-m",
            "Add an existing formatting violation",
        ],
    );
    let failed = report(run(), 1);
    assert_eq!(failed["scope"], "full");
    assert_eq!(failed["diagnostics"][0]["path"], "src/Example.java");
}
