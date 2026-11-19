use std::fs;
use std::path::Path;
use std::process::{Command, Output};

use serde_json::Value;
use tempfile::TempDir;

const CLEAN: &str =
    include_str!("../../../examples/java-single/src/main/java/example/Example.java");

fn project(source: &str) -> TempDir {
    let project = tempfile::tempdir().unwrap();
    fs::write(
        project.path().join(".dornguard.yml"),
        include_str!("../../../examples/java-single/.dornguard.yml"),
    )
    .unwrap();
    fs::create_dir_all(project.path().join("src")).unwrap();
    fs::write(project.path().join("src/Example.java"), source).unwrap();
    project
}

fn check(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_dornguard"));
    command
        .current_dir(root)
        .args(["check", "--format", "json"]);
    command
}

fn report(output: Output, exit_code: i32) -> Value {
    assert_eq!(
        output.status.code(),
        Some(exit_code),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).expect("stdout should contain exactly one JSON report")
}

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
    let project = project(CLEAN);
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
    let result = report(check(root).output().unwrap(), 3);
    assert_eq!(result["status"], "engine_error");
    assert_eq!(result["complete"], false);
}

#[test]
#[ignore = "requires the pinned Semgrep and PMD versions on PATH"]
fn both_engines_apply_spring_and_security_profiles() {
    let project = project(CLEAN);
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [spring-boot, security]\n",
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 0);
    assert_eq!(result["files_scanned"], 1);

    fs::write(
        root.join("src/Example.java"),
        r#"
import org.springframework.beans.factory.annotation.Autowired;

class Example {
    @Autowired
    private Object service;

    void configure(CorsRegistration cors) {
        cors.allowedOrigins("*");
        System.out.println(service);
        try {
            throw new RuntimeException();
        } catch (RuntimeException exception) { } // NOPMD
    }
}
"#,
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 1);
    let diagnostics = result["diagnostics"].as_array().unwrap();
    assert_eq!(
        diagnostics
            .iter()
            .map(|item| item["rule_id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "spring.field-injection",
            "security.cors-wildcard",
            "java.stdout-usage",
            "java.empty-catch"
        ],
    );
    assert_eq!(diagnostics.last().unwrap()["engine"], "pmd");
    assert_eq!(result["files_scanned"], 1);

    fs::write(
        root.join("src/Broken.java"),
        "class Broken { void x() { @@@ } }",
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 3);
    assert_eq!(result["complete"], false);
    assert!(
        result["failures"]
            .as_array()
            .unwrap()
            .iter()
            .any(|failure| failure["engine"] == "pmd")
    );
    assert!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| diagnostic["rule_id"] == "java.stdout-usage")
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
#[ignore = "requires the pinned engines, JDK 21, Gradle and DORNGUARD_ARCHUNIT_RUNNER"]
fn gradle_project_enforces_architecture_and_keeps_module_policies_separate() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/spring-boot-multimodule");
    let project = tempfile::tempdir().unwrap();
    copy_gradle_fixture(&fixture, project.path());
    let root = project.path();

    let initial = report(check(root).output().unwrap(), 0);
    assert_eq!(initial["files_scanned"], 7);

    let domain = root.join("api/src/main/java/example/api/domain/Book.java");
    fs::write(&domain, "package example.api.domain; public record Book(String title) { public Class<?> type() { return example.api.adapter.BookAdapter.class; } }").unwrap();
    let moved = "common/src/main/java/example/common/Moved.java";
    let moved_to = "batch/src/main/java/example/batch/Moved.java";
    fs::write(root.join(moved), "class Moved {}\n").unwrap();
    commit_fixture(root);
    git(root, &["branch", "pr-base"]);
    git(root, &["mv", moved, moved_to]);
    git(
        root,
        &["commit", "--quiet", "-m", "Move source across modules"],
    );
    let changed = report(
        check(root)
            .args(["--changed", "--base", "pr-base"])
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(changed["scope"], "changed");
    assert!(
        changed["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| diagnostic["rule_id"] == "architecture.domain-independence")
    );

    let controller = root.join("admin/src/main/java/example/admin/controller/AdminController.java");
    let source = fs::read_to_string(&controller).unwrap();
    fs::write(
        &controller,
        source.replace(
            "return service.ready();",
            "return new example.admin.repository.AdminRepository() != null && service.ready();",
        ),
    )
    .unwrap();
    let build = root.join("build.gradle.kts");
    let script = fs::read_to_string(&build).unwrap();
    fs::write(build, format!("{script}\nproject(\":api\") {{ dependencies {{ \"implementation\"(project(\":admin\")) }} }}\n")).unwrap();

    let lint = report(check(root).output().unwrap(), 1);
    let diagnostics = lint["diagnostics"].as_array().unwrap();
    for id in [
        "architecture.domain-independence",
        "architecture.controller-repository",
        "architecture.module-dependency",
    ] {
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic["rule_id"] == id),
            "missing {id} in {lint}"
        );
    }

    let admin = report(
        check(root).args(["--module", ":admin"]).output().unwrap(),
        1,
    );
    let diagnostics = admin["diagnostics"].as_array().unwrap();
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic["rule_id"] == "architecture.controller-repository")
    );
    assert!(diagnostics.iter().all(|diagnostic| diagnostic["path"]
        == "admin/src/main/java/example/admin/controller/AdminController.java"));

    fs::write(domain, "package example.api.domain; public record Book( {}").unwrap();
    let build_failure = report(check(root).args(["--module", ":api"]).output().unwrap(), 4);
    assert_eq!(build_failure["complete"], false);
    assert_eq!(build_failure["status"], "preparation_error");
}

fn copy_gradle_fixture(source: &Path, destination: &Path) {
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

fn commit_fixture(root: &Path) {
    git(root, &["init", "--quiet"]);
    git(root, &["add", "--", "."]);
    git(root, &["commit", "--quiet", "-m", "fixture"]);
}

fn git(root: &Path, args: &[&str]) {
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

fn write_custom_ruleset(root: &Path) {
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

fn kotlin_fixture(name: &str) -> TempDir {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../examples")
        .join(name);
    let project = tempfile::tempdir().unwrap();
    copy_gradle_fixture(&fixture, project.path());
    project
}

#[test]
#[ignore = "requires Gradle, JDK 21 and the pinned Semgrep"]
fn kotlin_source_rules_and_external_languages_run_in_kotlin_only_modules() {
    let project = kotlin_fixture("kotlin-single");
    let root = project.path();
    let test_sources = root.join("src/test/kotlin");
    fs::create_dir_all(&test_sources).unwrap();
    fs::write(
        test_sources.join("Ignored.kt"),
        "fun ignored() { println(42) }",
    )
    .unwrap();
    fs::write(
        root.join("unregistered.kt"),
        "fun ignored() { println(42) }",
    )
    .unwrap();
    fs::write(root.join("src/main/kotlin/script.kts"), "println(42)").unwrap();
    let clean = report(check(root).output().unwrap(), 0);
    assert_eq!(clean["files_scanned"], 1);

    let file = "src/main/kotlin/example/Example.kt";
    let source = "package example\n\nfun display() {\n    println(42)\n}\n";
    fs::write(root.join(file), source).unwrap();
    let flagged = report(check(root).output().unwrap(), 1);
    let finding = &flagged["diagnostics"][0];
    assert_eq!(finding["rule_id"], "kotlin.stdout-usage");
    assert_eq!(finding["path"], file);
    assert_eq!(finding["line"], 4);

    write_custom_ruleset(&root.join(".policy"));
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nruleset: {source: local, path: .policy}\nprofiles: [custom-java]\n",
    )
    .unwrap();
    let config = root.join(".policy/stderr.semgrep.yml");
    fs::write(&config, "rules:\n  - id: custom-stderr\n    languages: [kotlin]\n    message: Use a logger\n    severity: ERROR\n    pattern: println(...)\n").unwrap();
    let custom = report(check(root).output().unwrap(), 1);
    assert_eq!(custom["diagnostics"][0]["rule_id"], "custom.stderr");
    fs::write(config, "rules:\n  - id: custom-stderr\n    languages: [kotlin]\n    message: Invalid rule\n    severity: ERROR\n    pattern-regex: '['\n").unwrap();
    let failed = report(check(root).output().unwrap(), 3);
    assert_eq!(failed["failures"][0]["engine"], "semgrep");
    assert_eq!(failed["complete"], false);
}

const KOTLIN_SERVICE: &str = "service/src/custom/kotlin/relocated/LabelsService.kt";
const KOTLIN_ARCH_VIOLATION: &str = "package example.service.domain\n\nclass LabelsService {\n    fun format(value: String): String {\n        return example.service.adapter.LabelAdapter().map(value)\n    }\n}\n";

#[test]
#[ignore = "requires Gradle, JDK 21, the source engines and ArchUnit"]
fn kotlin_mixed_sources_report_physical_and_logical_locations() {
    let project = kotlin_fixture("kotlin-multimodule");
    let root = project.path();
    let clean = report(check(root).output().unwrap(), 0);
    assert_eq!(clean["files_scanned"], 4);
    let duplicate = root.join("service/src/shared/LabelsService.kt");
    fs::write(&duplicate, "/* outer\n /* nested */\n package example.service.domain\n*/\npackage example.service.other\nclass AnotherService\n").unwrap();
    fs::write(root.join(KOTLIN_SERVICE), KOTLIN_ARCH_VIOLATION).unwrap();
    let shared = "service/src/shared/Shared.kt";
    fs::write(
        root.join(shared),
        "package example.service\nfun display() { println(42) }\n",
    )
    .unwrap();
    let java = "service/src/shared/LabelAdapter.java";
    fs::write(root.join(java), "package example.service.adapter; public class LabelAdapter { public String map(String value) { System.out.println(value); try { throw new IllegalArgumentException(); } catch (IllegalArgumentException e) {} return value; } }\n").unwrap();

    let flagged = report(check(root).output().unwrap(), 1);
    assert_eq!(flagged["version"], 2);
    assert_eq!(flagged["files_scanned"], 5);
    let findings = flagged["diagnostics"].as_array().unwrap();
    for (id, path) in [
        ("kotlin.stdout-usage", shared),
        ("java.stdout-usage", java),
        ("java.empty-catch", java),
        ("architecture.domain-independence", KOTLIN_SERVICE),
    ] {
        assert!(
            findings
                .iter()
                .any(|entry| entry["rule_id"] == id && entry["path"] == path),
            "{flagged}"
        );
    }
    assert!(
        findings.iter().any(|entry| {
            entry["rule_id"] == "architecture.domain-independence" && entry["line"] == 5
        }),
        "{flagged}"
    );

    fs::write(
        &duplicate,
        "package example.service.domain\nclass AnotherService\n",
    )
    .unwrap();
    let inline = "service/src/custom/kotlin/relocated/InlineService.kt";
    fs::write(root.join(inline), "package example.service.domain\nclass InlineService { fun format(value: String): String = run { example.service.adapter.LabelAdapter().map(value) } }\n").unwrap();
    let ambiguous = report(check(root).output().unwrap(), 1);
    let findings = ambiguous["diagnostics"].as_array().unwrap();
    assert!(
        findings.iter().any(
            |entry| entry["class_name"] == "example.service.domain.LabelsService"
                && entry["module"] == ":service"
                && entry["path"].is_null()
                && entry["line"].is_null()
        ),
        "{ambiguous}"
    );
    assert!(
        findings
            .iter()
            .any(|entry| entry["path"] == inline && entry["line"].is_null()),
        "{ambiguous}"
    );
    let sarif = Command::new(env!("CARGO_BIN_EXE_dornguard"))
        .current_dir(root)
        .args(["check", "--format", "sarif"])
        .output()
        .unwrap();
    let sarif = report(sarif, 1);
    assert!(
        sarif["runs"][0]["results"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| {
                entry["locations"][0]["logicalLocations"][0]["fullyQualifiedName"]
                    == "example.service.domain.LabelsService"
                    && entry["locations"][0]["physicalLocation"].is_null()
            })
    );
    let baseline = Command::new(env!("CARGO_BIN_EXE_dornguard"))
        .current_dir(root)
        .args(["baseline", "create"])
        .output()
        .unwrap();
    assert!(
        baseline.status.success(),
        "{}",
        String::from_utf8_lossy(&baseline.stderr)
    );
    let filtered = report(
        check(root)
            .args(["--baseline", ".dornguard-baseline.json"])
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(filtered["suppressed_count"], findings.len());

    fs::write(
        root.join(KOTLIN_SERVICE),
        "package example.service.domain\nclass {",
    )
    .unwrap();
    let failed = report(check(root).output().unwrap(), 4);
    assert_eq!(failed["status"], "preparation_error");
}

#[test]
#[ignore = "requires Git, Gradle, JDK 21, the source engines and ArchUnit"]
fn kotlin_changed_base_keeps_old_module_ownership_and_checks_dependents() {
    let project = kotlin_fixture("kotlin-multimodule");
    let root = project.path();
    fs::write(root.join(KOTLIN_SERVICE), KOTLIN_ARCH_VIOLATION).unwrap();
    let old = "library/src/main/kotlin/example/library/Moved.kt";
    let tools = "tools/src/main/kotlin";
    let deleted = "tools/src/main/kotlin/Deleted.kt";
    let modified = "tools/src/main/kotlin/Modified.kt";
    fs::create_dir_all(root.join(tools)).unwrap();
    fs::write(root.join(old), "class Moved\n").unwrap();
    fs::write(root.join(deleted), "class Deleted\n").unwrap();
    fs::write(root.join(modified), "class Modified\n").unwrap();
    commit_fixture(root);
    git(root, &["branch", "pr-base"]);
    git(root, &["mv", old, "tools/src/main/kotlin/Moved.kt"]);
    git(root, &["rm", "--quiet", deleted]);
    git(
        root,
        &["commit", "--quiet", "-m", "Move and remove Kotlin sources"],
    );
    fs::write(root.join(modified), "fun changed() { println(42) }\n").unwrap();
    fs::write(
        root.join(tools).join("Added.kt"),
        "fun added() { println(42) }\n",
    )
    .unwrap();
    let run = || {
        check(root)
            .args(["--changed", "--base", "pr-base"])
            .output()
            .unwrap()
    };
    let flagged = report(run(), 1);
    assert_eq!(flagged["scope"], "changed");
    assert_eq!(flagged["complete"], true);
    let findings = flagged["diagnostics"].as_array().unwrap();
    for source in [KOTLIN_SERVICE, modified, "tools/src/main/kotlin/Added.kt"] {
        assert!(
            findings.iter().any(|finding| finding["path"] == source),
            "{flagged}"
        );
    }

    fs::write(root.join("Outside.kt"), "class Outside\n").unwrap();
    let fallback = report(run(), 1);
    assert_eq!(fallback["scope"], "full");
    assert!(
        fallback["fallback_reason"]
            .as_str()
            .unwrap()
            .contains("Outside.kt")
    );
}
