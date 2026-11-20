use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use std::sync::{Mutex, PoisonError};

use serde_json::Value;
use tempfile::TempDir;

const CLEAN: &str =
    include_str!("../../../examples/java-single/src/main/java/example/Example.java");

// These fixtures use the same Maven local repository. Concurrent downloads can
// collide on Windows when Maven replaces repository update markers.
static MAVEN_TEST_LOCK: Mutex<()> = Mutex::new(());

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

#[test]
fn format_defaults_check_java_kotlin_and_javascript_without_a_build() {
    let project = project("class Example {\r\n  void f() {\r\n    if(true) {}  \r\n  }\r\n}");
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [format]\n",
    )
    .unwrap();
    fs::write(
        root.join("src/sample.kt"),
        "fun example() {\n  if(true) {\n        println(\"if(false)\")\n  }\n}\n",
    )
    .unwrap();
    fs::write(
        root.join("src/sample.js"),
        "function example() {\n\tif(true) {\n    console.log(`if(false)\n  \n\n\n text`);\n  }\n}\n\n\n",
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 1);
    assert_eq!(result["files_scanned"], 3);
    let findings = result["diagnostics"].as_array().unwrap();
    for (path, rule, line) in [
        ("src/Example.java", "line-ending", 1),
        ("src/Example.java", "indentation", 2),
        ("src/Example.java", "control-spacing", 3),
        ("src/Example.java", "trailing-whitespace", 3),
        ("src/Example.java", "final-newline", 5),
        ("src/sample.kt", "indentation", 2),
        ("src/sample.kt", "control-spacing", 2),
        ("src/sample.js", "indentation", 2),
        ("src/sample.js", "control-spacing", 2),
        ("src/sample.js", "blank-lines", 11),
    ] {
        assert!(
            findings.iter().any(|finding| {
                finding["path"] == path
                    && finding["rule_id"] == format!("format.{rule}")
                    && finding["line"] == line
            }),
            "{path}:{line} {rule}\n{result}"
        );
    }
    assert!(findings.iter().all(|finding| {
        finding["path"] != "src/sample.js" || !(3..=7).contains(&finding["line"].as_u64().unwrap())
    }));
}

#[test]
fn format_accepts_overrides_and_ignores_program_text_inside_literals() {
    let project = project(
        "class Example {\r\n  String sample = \"\"\"\r\n        if(x)   \r\n  \"\"\";\r\n  void f() {\r\n    if (true) {}\r\n  }\r\n}\r\n",
    );
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [format]\nformat: {java_indent: 2, line_ending: crlf}\n",
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 0);
    assert_eq!(result["files_scanned"], 1);
    assert!(result["diagnostics"].as_array().unwrap().is_empty());

    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [format]\nformat: {kotlin_indent: 0}\n",
    )
    .unwrap();
    let invalid = report(check(root).output().unwrap(), 2);
    assert!(
        invalid["failures"][0]["message"]
            .as_str()
            .unwrap()
            .contains("format.kotlin_indent")
    );
}

#[test]
fn jspecify_checks_effective_java_scopes_without_external_engines() {
    let project = project(CLEAN);
    let root = project.path();
    fs::remove_file(root.join("src/Example.java")).unwrap();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [jspecify]\n",
    )
    .unwrap();
    for (file, source) in [
        (
            "core/src/main/java/example/package-info.java",
            "@NullMarked\npackage example;\nimport org.jspecify.annotations.NullMarked;\n",
        ),
        (
            "core/src/main/java/example/Example.java",
            "package example;\nimport org.jspecify.annotations.*;\nclass Example {\n  record Covered() {}\n  @NullUnmarked class OptOut {\n    @NullMarked interface CoveredAgain {}\n  }\n  @NullUnmarked void f() { class Local {} }\n}\n",
        ),
        (
            "core/src/main/java/example/sub/Other.java",
            "package example.sub;\nimport other.NullMarked;\n@NullMarked class Other {}\n@org.jspecify.annotations.NullMarked enum Covered { ONE }\n@interface Marker {}\n",
        ),
        (
            "core/src/test/java/example/ExampleTest.java",
            "package example;\nclass ExampleTest {}\n",
        ),
        (
            "jpms/src/main/java/module-info.java",
            "import org.jspecify.annotations.NullMarked;\n@NullMarked module example.jpms { requires static org.jspecify; }\n",
        ),
        (
            "jpms/src/main/java/example/a/Modular.java",
            "package example.a;\nclass Modular {}\n",
        ),
        (
            "jpms/src/main/java/example/b/package-info.java",
            "@org.jspecify.annotations.NullUnmarked package example.b;\n",
        ),
        (
            "jpms/src/main/java/example/b/Legacy.java",
            "package example.b;\nclass Legacy {}\n",
        ),
    ] {
        let path = root.join(file);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }

    let result = report(check(root).output().unwrap(), 1);
    let diagnostics = result["diagnostics"].as_array().unwrap();
    let locations = diagnostics
        .iter()
        .map(|diagnostic| {
            assert_eq!(diagnostic["rule_id"], "java.jspecify-null-marked");
            assert_eq!(diagnostic["engine"], "builtin");
            (
                diagnostic["path"].as_str().unwrap(),
                diagnostic["line"].as_u64().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        locations,
        [
            ("core/src/main/java/example/Example.java", 5),
            ("core/src/main/java/example/Example.java", 8),
            ("core/src/main/java/example/sub/Other.java", 3),
            ("core/src/main/java/example/sub/Other.java", 5),
            ("core/src/test/java/example/ExampleTest.java", 2),
            ("jpms/src/main/java/example/b/Legacy.java", 2),
        ]
    );
    let selected = report(
        check(root)
            .arg("jpms/src/main/java/example/a/Modular.java")
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(selected["files_scanned"], 1);
}

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

#[test]
#[ignore = "requires the pinned Semgrep and PMD versions on PATH"]
fn spring_field_injection_allows_java_test_fixtures() {
    let project = project(CLEAN);
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [spring-boot]\n",
    )
    .unwrap();
    let service = "web/src/main/java/example/Service.java";
    let fixture = "web/src/test/java/example/ServiceIntegrationTest.java";
    for path in [service, fixture] {
        let file = root.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(
            file,
            r#"
import org.springframework.beans.factory.annotation.Autowired;

class Example {
    @Autowired
    private Object dependency;

    void run() {
        System.out.println(dependency);
    }
}
"#,
        )
        .unwrap();
    }

    let result = report(check(root).output().unwrap(), 1);
    assert_eq!(result["files_scanned"], 3);
    assert_eq!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| (
                item["path"].as_str().unwrap(),
                item["rule_id"].as_str().unwrap()
            ))
            .collect::<Vec<_>>(),
        [
            (service, "spring.field-injection"),
            (service, "java.stdout-usage"),
            (fixture, "java.stdout-usage"),
        ],
    );
}

#[test]
#[ignore = "requires the pinned Semgrep and PMD versions on PATH"]
fn java_profile_warns_about_broken_null_checks_and_hash_contracts() {
    let project = project(
        r#"class Example {
    boolean broken(String key) { return key != null || key.isEmpty(); }
    boolean safe(String key) { return key != null && key.isEmpty(); }
    @Override public boolean equals(Object other) { return other instanceof Example; }
}
class Complete {
    @Override public boolean equals(Object other) { return other instanceof Complete; }
    @Override public int hashCode() { return 1; }
}
"#,
    );
    let result = report(
        check(project.path())
            .args(["--profile", "java"])
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(result["complete"], true);
    assert_eq!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|finding| (
                finding["rule_id"].as_str().unwrap(),
                finding["line"].as_u64().unwrap(),
                finding["severity"].as_str().unwrap(),
            ))
            .collect::<Vec<_>>(),
        [
            ("java.broken-null-check", 2, "warning"),
            ("java.equals-hashcode-contract", 4, "warning"),
        ],
        "{result}"
    );
}

#[test]
#[ignore = "requires the pinned Semgrep and PMD versions on PATH"]
fn java_and_spring_warn_about_decimal_precision_finally_and_async_self_calls() {
    let project = project(
        r#"import java.math.BigDecimal;
import org.springframework.scheduling.annotation.Async;

class Example {
    void before() { work(); }

    @Async("executor")
    void work() {}
    void work(int value) {}

    void after(Example other) {
        this.work();
        work(1);
        other.work();
    }

    BigDecimal inexact() { return new BigDecimal(0.1); }
    BigDecimal exact() { return new BigDecimal("0.1"); }
    BigDecimal exactValue() { return BigDecimal.valueOf(0.1); }

    int eclipsed() {
        try { return 1; } finally { return 2; }
    }

    int preserved() {
        try { return 1; } finally { work(1); }
    }
}
"#,
    );
    let result = report(
        check(project.path())
            .args(["--profile", "spring-boot"])
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(result["complete"], true);
    assert_eq!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|finding| (
                finding["rule_id"].as_str().unwrap(),
                finding["line"].as_u64().unwrap(),
                finding["severity"].as_str().unwrap(),
            ))
            .collect::<Vec<_>>(),
        [
            ("spring.async-self-invocation", 5, "warning"),
            ("spring.async-self-invocation", 12, "warning"),
            ("java.bigdecimal-from-floating-point-literal", 17, "warning"),
            ("java.return-from-finally", 22, "warning"),
        ],
        "{result}"
    );
}

#[test]
#[ignore = "requires the pinned Semgrep version on PATH"]
fn thymeleaf_output_review_is_opt_in_and_flags_unescaped_expressions() {
    let project = project(CLEAN);
    let root = project.path();
    let template = "src/main/resources/templates/example.html";
    fs::create_dir_all(root.join(template).parent().unwrap()).unwrap();
    fs::write(
        root.join(template),
        r#"<div th:utext="${description}"></div>
<div data-th-utext="${description}"></div>
<div th:utext="'prefix ' + ${description}"></div>
<div th:text="${description}">[[${description}]]</div>
<div>[(${description})]</div>
<div>[('prefix ' + ${description})]</div>
<div th:inline="none">[(${description})]</div>
<div data-th-inline="none"><b>[(${description})]</b></div>
<!-- <div th:utext="${description}">[(${description})]</div> -->
<div title="[(${description})]"></div>
<script>let template = '[(${description})]';</script>
<div th:utext="'constant'"></div>
<div th:utext="'${literal}'">[('${literal}')]</div>
"#,
    )
    .unwrap();
    let default = report(
        check(root)
            .args(["--profile", "thymeleaf"])
            .arg(template)
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(default["diagnostics"], serde_json::json!([]));

    let review = report(
        check(root)
            .args(["--profile", "thymeleaf-output-review"])
            .arg(template)
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(review["complete"], true);
    assert_eq!(
        review["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|finding| (
                finding["rule_id"].as_str().unwrap(),
                finding["line"].as_u64().unwrap(),
                finding["severity"].as_str().unwrap(),
            ))
            .collect::<Vec<_>>(),
        [
            ("thymeleaf.unescaped-output", 1, "warning"),
            ("thymeleaf.unescaped-output", 2, "warning"),
            ("thymeleaf.unescaped-output", 3, "warning"),
            ("thymeleaf.unescaped-output", 5, "warning"),
            ("thymeleaf.unescaped-output", 6, "warning"),
        ],
        "{review}"
    );
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

    let domain_path = "api/src/main/java/example/api/domain/Book.java";
    let domain = root.join(domain_path);
    let source = r#"package example.api.domain;
import example.common.*;
import java.net.*;
import static example.common.Titles.*;
import java.net.http.HttpClient;
/** See {@link Titles}. */
public record Book(String title) {
    public Titles lookup(Titles value) { normalize(title); return value; }
    void request() {
        var client = HttpClient.newHttpClient();
        client.followRedirects();
    }
}
"#;
    fs::write(&domain, source).unwrap();
    let controller_path = "admin/src/main/java/example/admin/controller/AdminController.java";
    let controller = root.join(controller_path);
    let controller_source = fs::read_to_string(&controller).unwrap().replace(
        "import org.springframework.web.bind.annotation.GetMapping;\nimport org.springframework.web.bind.annotation.RestController;",
        "import org.springframework.web.bind.annotation.*;",
    );
    fs::write(&controller, controller_source).unwrap();
    let build = root.join("build.gradle.kts");
    let script = fs::read_to_string(&build).unwrap();
    fs::write(
        &build,
        format!("{script}\nsubprojects {{ tasks.withType<JavaCompile>().configureEach {{ options.release.set(17) }} }}\nproject(\":api\") {{ tasks.named<JavaCompile>(\"compileTestJava\") {{ options.release.set(25) }} }}\n"),
    )
    .unwrap();
    let initial = report(check(root).output().unwrap(), 0);
    assert!(
        unnecessary_import_lines(&initial, controller_path).is_empty(),
        "{initial}"
    );
    assert_eq!(initial["files_scanned"], 7);
    assert_eq!(
        unnecessary_import_lines(&initial, domain_path),
        [3],
        "{initial}"
    );
    assert!(
        initial["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .all(
                |item| item["rule_id"] != "java.resource-not-closed" || item["path"] != domain_path
            ),
        "{initial}"
    );

    fs::OpenOptions::new()
        .write(true)
        .open(&controller)
        .unwrap()
        .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(60))
        .unwrap();
    let unchanged = report(check(root).output().unwrap(), 0);
    assert_eq!(unchanged["complete"], true);

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
    let template = "api/src/main/resources/templates/book.html";
    fs::create_dir_all(root.join(template).parent().unwrap()).unwrap();
    fs::write(
        root.join(template),
        r#"<a th:href="${@bookLinks.list()}">Books</a>"#,
    )
    .unwrap();
    let changed = report(
        check(root)
            .args(["--changed", "--base", "pr-base", "--profile", "thymeleaf"])
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

    assert!(
        changed["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["rule_id"] == "thymeleaf.restricted-bean-access" && item["path"] == template
            })
    );

    let source = fs::read_to_string(&controller).unwrap();
    fs::write(
        &controller,
        source.replace(
            "return service.ready();",
            "return new example.admin.repository.AdminRepository() != null && service.ready();",
        ),
    )
    .unwrap();
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
    let diagnostics = admin["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["engine"] == "archunit")
        .collect::<Vec<_>>();
    assert!(!diagnostics.is_empty());
    assert!(
        diagnostics
            .iter()
            .all(|diagnostic| diagnostic["rule_id"] == "architecture.controller-repository")
    );
    assert!(diagnostics.iter().all(|diagnostic| diagnostic["path"]
        == "admin/src/main/java/example/admin/controller/AdminController.java"));

    let test_path = "api/src/test/java/example/api/OnlyTest.java";
    fs::create_dir_all(root.join(test_path).parent().unwrap()).unwrap();
    fs::write(
        root.join(test_path),
        "class OnlyTest { void exercise() { System.out.println(1); try { Integer.parseInt(\"42\"); } catch (NumberFormatException _) {} } }\n",
    )
    .unwrap();
    let test = report(
        check(root)
            .args(["--module", ":api", test_path])
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(test["files_scanned"], 1, "{test}");
    let findings = test["diagnostics"].as_array().unwrap();
    assert!(
        findings
            .iter()
            .any(|item| item["rule_id"] == "java.stdout-usage" && item["path"] == test_path),
        "{test}"
    );
    assert!(
        findings.iter().all(|item| item["engine"] != "archunit"),
        "{test}"
    );

    assert!(
        findings
            .iter()
            .any(|item| item["rule_id"] == "java.empty-catch"),
        "{test}"
    );

    fs::write(
        domain,
        "package example.api.domain; public record Book(Missing value) {}",
    )
    .unwrap();
    let build_failure = report(check(root).args(["--module", ":api"]).output().unwrap(), 4);
    assert_eq!(build_failure["complete"], false);
    assert_eq!(build_failure["status"], "preparation_error");
    assert_eq!(build_failure["failures"][0]["stage"], "project_compile");
    assert_eq!(build_failure["failures"][0]["build_tool"], "Gradle");
    assert_eq!(build_failure["failures"][0]["modules"][0], ":api");
    assert!(
        build_failure["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["rule_id"] == "java.stdout-usage" && item["path"] == test_path),
        "{build_failure}"
    );
    assert!(
        build_failure["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["engine"] != "archunit")
    );
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

    fs::write(
        root.join(file),
        r#"package example

import kotlinx.coroutines.GlobalScope as ApplicationScope
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.launch
import kotlinx.coroutines.async
import java.lang.Thread as JvmThread

suspend fun load() {
    runBlocking { read() }
    worker.runBlocking { read() }
    ApplicationScope.launch { read() }
    kotlinx.coroutines.GlobalScope.async { read() }
}

suspend fun loadExpression() = kotlinx.coroutines.runBlocking { read() }
fun main() = runBlocking { read() }
fun unrelated() = worker.launch { read() }

suspend fun cancellation() {
    try { read() } catch (stop: kotlinx.coroutines.CancellationException) { logger.warn("cancel", stop) }
    try { read() } catch (stop: java.util.concurrent.CancellationException) { }
    try { read() } catch (stop: kotlin.coroutines.cancellation.CancellationException) { return }
    try { read() } catch (stop: kotlinx.coroutines.CancellationException) { logger.warn("cancel", stop); throw stop }
    try { read() } catch (stop: custom.CancellationException) { logger.warn("unrelated", stop) }
}

suspend fun blockingWait() {
    Thread.sleep(10)
    java.lang.Thread.sleep(10)
    JvmThread.sleep(10)
    worker.sleep(10)
}
suspend fun expressionWait() = Thread.sleep(10)
fun ordinaryWait() = Thread.sleep(10)

suspend fun localOperation() {}
fun synchronousOperation() {}
suspend fun wrap() {
    runCatching { localOperation() }
    runCatching { kotlinx.coroutines.delay(10) }
    runCatching { kotlinx.coroutines.yield() }.getOrThrow()
    runCatching { synchronousOperation() }
    runCatching { laterOperation() }
}
suspend fun laterOperation() {}
"#,
    )
    .unwrap();
    let warnings = report(check(root).output().unwrap(), 0);
    assert_eq!(warnings["complete"], true);
    let diagnostics = warnings["diagnostics"].as_array().unwrap();
    assert_eq!(
        diagnostics
            .iter()
            .filter(|finding| finding["rule_id"] != "kotlin.fully-qualified-type")
            .map(|finding| (
                finding["rule_id"].as_str().unwrap(),
                finding["line"].as_u64().unwrap()
            ))
            .collect::<Vec<_>>(),
        [
            ("kotlin.run-blocking-in-suspend", 10),
            ("kotlin.global-scope", 12),
            ("kotlin.global-scope", 13),
            ("kotlin.run-blocking-in-suspend", 16),
            ("kotlin.swallowed-cancellation", 21),
            ("kotlin.swallowed-cancellation", 22),
            ("kotlin.swallowed-cancellation", 23),
            ("kotlin.thread-sleep-in-suspend", 29),
            ("kotlin.thread-sleep-in-suspend", 30),
            ("kotlin.thread-sleep-in-suspend", 31),
            ("kotlin.thread-sleep-in-suspend", 34),
            ("kotlin.run-catching-cancellation", 40),
            ("kotlin.run-catching-cancellation", 41),
            ("kotlin.run-catching-cancellation", 44),
        ],
        "{warnings}"
    );
    assert!(
        diagnostics
            .iter()
            .all(|finding| finding["severity"] == "warning")
    );
    fs::write(
        root.join(file),
        r#"package example
import java.util.ArrayList as JavaList
import java.time.Instant
/* Outer /* nested */ java.util.TreeSet */
class Demo : java.util.ArrayList<String>() {
    val instant: java.time.Instant? = null
    val short: Instant? = null
    val label = "java.util.List"
    val raw = """java.util.Date"""
    val imported = JavaList<String>()
}
"#,
    )
    .unwrap();
    let qualified = report(check(root).output().unwrap(), 0);
    let qualified = qualified["diagnostics"].as_array().unwrap();
    assert_eq!(qualified.len(), 2);
    assert_eq!(qualified[0]["line"], 5);
    assert_eq!(qualified[1]["line"], 6);
    assert!(
        qualified
            .iter()
            .all(|item| item["rule_id"] == "kotlin.fully-qualified-type")
    );

    fs::write(root.join(file), source).unwrap();

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
    assert_eq!(flagged["version"], 3);
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

#[test]
#[ignore = "requires Git, Maven 3, JDK 21, Semgrep and the bundled JVM artifacts"]
fn maven_reactor_enforces_layered_and_hexagonal_profiles() {
    let _maven = MAVEN_TEST_LOCK
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let parent = tempfile::tempdir().unwrap();
    let root = parent.path().join("maven reactor");
    fs::create_dir_all(&root).unwrap();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [layered, hexagonal, thymeleaf, jsp]\n",
    )
    .unwrap();
    fs::write(
        root.join("pom.xml"),
        r#"<project>
          <modelVersion>4.0.0</modelVersion>
          <groupId>example</groupId><artifactId>root</artifactId><version>1</version>
          <packaging>pom</packaging>
          <modules><module>web</module><module>storage</module></modules>
          <properties><maven.compiler.release>21</maven.compiler.release></properties>
          <build><pluginManagement><plugins><plugin>
            <artifactId>maven-compiler-plugin</artifactId><version>3.13.0</version>
          </plugin></plugins></pluginManagement></build>
        </project>"#,
    )
    .unwrap();

    let child = |artifact, extra| {
        format!(
            r#"<project>
              <modelVersion>4.0.0</modelVersion>
              <parent><groupId>example</groupId><artifactId>root</artifactId><version>1</version></parent>
              <artifactId>{artifact}</artifactId>
              {extra}
            </project>"#
        )
    };
    let storage = root.join("storage/src/main/java/example");
    fs::create_dir_all(storage.join("repository")).unwrap();
    fs::create_dir_all(storage.join("infrastructure")).unwrap();
    fs::write(root.join("storage/pom.xml"), child("storage", "")).unwrap();
    fs::write(
        storage.join("repository/BookRepository.java"),
        "package example.repository; public class BookRepository { public @interface Route {} public static int count() { return 0; } }",
    )
    .unwrap();
    fs::write(
        storage.join("infrastructure/BookStorage.java"),
        "package example.infrastructure; public class BookStorage {}",
    )
    .unwrap();

    let web = root.join("web/custom sources/example");
    fs::create_dir_all(web.join("controller")).unwrap();
    fs::create_dir_all(web.join("domain")).unwrap();
    fs::write(
        root.join("web/pom.xml"),
        child(
            "web",
            r#"<dependencies><dependency>
              <groupId>example</groupId><artifactId>storage</artifactId><version>1</version>
              <scope>provided</scope>
            </dependency></dependencies>
            <build>
              <sourceDirectory>custom sources</sourceDirectory>
              <outputDirectory>${project.build.directory}/custom classes</outputDirectory>
              <plugins><plugin><artifactId>maven-compiler-plugin</artifactId>
                <configuration>
                  <release>17</release>
                  <excludes><exclude>**/Excluded.java</exclude></excludes>
                </configuration>
              </plugin></plugins>
            </build>"#,
        ),
    )
    .unwrap();
    let controller = web.join("controller/BookController.java");
    let domain = web.join("domain/Book.java");
    fs::write(
        &controller,
        "package example.controller; public class BookController { example.repository.BookRepository repository; }",
    )
    .unwrap();
    fs::write(
        &domain,
        "package example.domain; public class Book { example.infrastructure.BookStorage storage; }",
    )
    .unwrap();

    let template = root.join("web/src/main/resources/templates/books.htm");
    fs::create_dir_all(template.parent().unwrap()).unwrap();
    fs::write(&template, r#"<a th:href="${@bookLinks.list()}">Books</a>"#).unwrap();
    let jsp = root.join("web/src/main/webapp/books.jsp");
    fs::create_dir_all(jsp.parent().unwrap()).unwrap();
    fs::write(&jsp, "<%= books.size() %>").unwrap();
    let kotlin = root.join("web/src/main/kotlin/Book.kt");
    fs::create_dir_all(kotlin.parent().unwrap()).unwrap();
    fs::write(&kotlin, "fun sample() {\n  println(42)\n}\n").unwrap();

    fs::write(root.join(".gitignore"), "target/\n").unwrap();
    commit_fixture(&root);
    let result = report(
        check(&root)
            .args(["--changed", "--profile", "jspecify", "--profile", "format"])
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(result["scope"], "full");
    assert_eq!(
        result["fallback_reason"],
        "Maven build checks scan the full reactor"
    );
    assert_eq!(result["complete"], true);
    assert_eq!(result["files_scanned"], 7);
    assert!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| {
                finding["rule_id"] == "format.indentation"
                    && finding["path"] == "web/src/main/kotlin/Book.kt"
            })
    );
    assert!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| {
                finding["rule_id"] == "java.jspecify-null-marked"
                    && finding["path"] == "web/custom sources/example/domain/Book.java"
            })
    );
    assert!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| {
                finding["rule_id"] == "thymeleaf.restricted-bean-access"
                    && finding["path"] == "web/src/main/resources/templates/books.htm"
            })
    );
    assert!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|finding| {
                finding["rule_id"] == "jsp.scripting-element"
                    && finding["path"] == "web/src/main/webapp/books.jsp"
            })
    );
    for (rule, path) in [
        (
            "architecture.controller-repository",
            "web/custom sources/example/controller/BookController.java",
        ),
        (
            "architecture.domain-independence",
            "web/custom sources/example/domain/Book.java",
        ),
    ] {
        assert!(
            result["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|finding| {
                    finding["rule_id"] == rule
                        && finding["module"] == "example:web"
                        && finding["path"] == path
                }),
            "{result}"
        );
    }
    assert!(
        root.join("web/target/custom classes/example/domain/Book.class")
            .is_file()
    );

    fs::write(
        controller,
        "package example.controller; public class BookController {}",
    )
    .unwrap();
    fs::write(domain, "package example.domain; public class Book {}").unwrap();
    fs::write(template, r#"<a th:href="@{/books}">Books</a>"#).unwrap();
    fs::write(jsp, "<c:out value=\"books\"/>").unwrap();
    let mut selected = check(&root);
    selected.args(["--module", "example:web"]);
    let result = report(selected.output().unwrap(), 0);
    assert_eq!(result["complete"], true);
    assert_eq!(result["files_scanned"], 4);

    fs::write(
        web.join("domain/Book.java"),
        "package example.domain; public class Book { Missing missing; }",
    )
    .unwrap();
    fs::write(web.join("controller/BookController.java"), "package example.controller; public class BookController { void print() { System.out.println(1); } }\n").unwrap();
    let result = report(
        check(&root).args(["--profile", "java"]).output().unwrap(),
        4,
    );
    assert_eq!(result["status"], "preparation_error");
    assert_eq!(result["complete"], false);
    assert_eq!(result["failures"][0]["stage"], "project_compile");
    assert_eq!(result["failures"][0]["build_tool"], "Maven");
    assert!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["rule_id"] == "java.stdout-usage"
                && item["path"] == "web/custom sources/example/controller/BookController.java"),
        "{result}"
    );
    assert!(result["files_requested"].as_u64().unwrap() > 0);
    fs::write(
        web.join("controller/BookController.java"),
        "package example.controller; public class BookController {}",
    )
    .unwrap();

    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [java]\n",
    )
    .unwrap();
    let domain_path = "web/custom sources/example/domain/Book.java";
    fs::write(
        root.join(domain_path),
        r#"package example.domain;
import example.repository.*;
import java.net.*;
import static example.repository.BookRepository.*;
import java.net.http.HttpClient;
import example.infrastructure.*;
/** See {@link BookRepository}. */
public record Book(BookRepository repository) {
    @Route
    public BookRepository route() { count(); return repository; }
    void request() {
        HttpClient client = HttpClient.newHttpClient();
        client.followRedirects();
    }
}
"#,
    )
    .unwrap();
    let test_path = "web/src/test/java/example/domain/BookTest.java";
    fs::create_dir_all(root.join(test_path).parent().unwrap()).unwrap();
    fs::write(
        root.join(test_path),
        "package example.domain;\nimport java.net.*;\nimport java.io.InputStream;\nclass BookTest {}\n",
    )
    .unwrap();
    let excluded_path = "web/custom sources/example/domain/Excluded.java";
    fs::write(
        root.join(excluded_path),
        "package example.domain;\nimport absent.types.*;\nimport java.io.InputStream;\nrecord Excluded(Missing type) {}\n",
    )
    .unwrap();
    let resolved = report(check(&root).arg("--resolve-imports").output().unwrap(), 0);
    assert_eq!(resolved["files_scanned"], 6, "{resolved}");
    assert_eq!(
        unnecessary_import_lines(&resolved, domain_path),
        [3, 6],
        "{resolved}"
    );
    assert!(
        resolved["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .all(
                |item| item["rule_id"] != "java.resource-not-closed" || item["path"] != domain_path
            ),
        "{resolved}"
    );
    assert_eq!(
        unnecessary_import_lines(&resolved, test_path),
        [3],
        "{resolved}"
    );
    assert_eq!(
        unnecessary_import_lines(&resolved, excluded_path),
        [3],
        "{resolved}"
    );

    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [flyway]\n",
    )
    .unwrap();
    let migration = "src/main/resources/db/migration/V1__initial.sql";
    for module in ["", "web", "storage"] {
        let file = root.join(module).join(migration);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, "select 1;\n").unwrap();
    }
    git(&root, &["add", "-A"]);
    git(
        &root,
        &["commit", "--quiet", "-m", "Add versioned migrations"],
    );
    for module in ["", "web", "storage"] {
        fs::write(root.join(module).join(migration), "select 2;\n").unwrap();
    }
    for (module, expected) in [
        ("example:root", migration.to_owned()),
        ("example:web", format!("web/{migration}")),
    ] {
        let result = report(check(&root).args(["--module", module]).output().unwrap(), 0);
        let findings = result["diagnostics"].as_array().unwrap();
        assert_eq!(findings.len(), 1, "{result}");
        assert_eq!(findings[0]["path"], expected, "{result}");
    }
}

#[test]
#[ignore = "requires Git and the pinned Semgrep version"]
fn jsp_profile_handles_changed_jsp_sources_without_running_other_regex_rules() {
    let project = project(CLEAN);
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [java-source, jsp]\n",
    )
    .unwrap();
    let web = root.join("src/main/webapp");
    fs::create_dir_all(&web).unwrap();
    commit_fixture(root);
    fs::write(
        web.join("books.jsp"),
        r#"<%@ page contentType="text/html" %>
<%-- <%= hidden %> <c:out value="hidden" escapeXml="false"/> --%>
<% int page = 1; %>
<%= com.example.Books.title() %>
<!-- <%= title %> -->
<c:out value="${title}" escapeXml="true"/>
<c:out value="${description}" escapeXml='false'/>
"#,
    )
    .unwrap();
    fs::write(
        web.join("layout.jspx"),
        r#"<jsp:root xmlns:jsp="http://java.sun.com/JSP/Page" version="2.3">
<!-- <jsp:scriptlet>int hidden = 1;</jsp:scriptlet> -->
<jsp:expression>title</jsp:expression>
</jsp:root>
"#,
    )
    .unwrap();
    fs::write(web.join("unrelated.html"), "<%= example %>").unwrap();

    let result = report(check(root).arg("--changed").output().unwrap(), 0);
    assert_eq!(result["scope"], "changed");
    assert_eq!(result["files_scanned"], 2);
    assert_eq!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|finding| (
                finding["rule_id"].as_str().unwrap(),
                finding["line"].as_u64().unwrap()
            ))
            .collect::<Vec<_>>(),
        [
            ("jsp.scripting-element", 3),
            ("jsp.scripting-element", 4),
            ("jsp.scripting-element", 5),
            ("jsp.unescaped-output", 7),
            ("jsp.scripting-element", 3),
        ],
        "{result}"
    );
}

#[test]
#[ignore = "requires Git and the pinned Semgrep version"]
fn thymeleaf_profile_reports_restricted_bean_access_in_changed_templates() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [thymeleaf]\n",
    )
    .unwrap();
    let dir = root.join("src/main/resources/templates");
    fs::create_dir_all(&dir).unwrap();
    let page = dir.join("books.html");
    let safe = r#"<a th:href="@{/books}">Books</a>
<a th:href="@{/books(id=${@links.currentId()})}">Books</a>
<i th:text="${@labels.title()}"></i>
<a th:href="${'https://example.com/@author'}">Link</a>
<a th:href="${assetUrl}">Link</a>
<!-- <a th:href="${@example.link()}">Comment</a> -->
<pre>th:href="${@example.text()}"</pre>
"#;
    fs::write(&page, safe).unwrap();
    fs::write(
        root.join("unrelated.html"),
        r#"<a th:href="${@links.outside()}">Outside</a>"#,
    )
    .unwrap();
    commit_fixture(root);
    let clean = report(check(root).output().unwrap(), 0);
    assert_eq!(clean["files_scanned"], 1);

    fs::write(
        &page,
        format!(
            r#"{safe}<a th:href="${{@links.book()}}">Book</a>
<script data-th-src='@{{${{@links.script()}}}}'></script>
<div th:utext="${{@renderer.html()}}" th:onclick="${{@clickHandler.code()}}"></div>
<section th:replace="~{{layout :: ${{@fragments.current()}}}}"></section>
"#
        ),
    )
    .unwrap();

    let violations = report(check(root).arg("--changed").output().unwrap(), 1);
    assert_eq!(violations["scope"], "changed");
    assert_eq!(violations["complete"], true);
    assert_eq!(violations["files_scanned"], 1);
    let findings = violations["diagnostics"].as_array().unwrap();
    assert_eq!(findings.len(), 5, "{violations}");
    for finding in findings {
        assert_eq!(finding["path"], "src/main/resources/templates/books.html");
        assert_eq!(finding["rule_id"], "thymeleaf.restricted-bean-access");
        assert_eq!(finding["severity"], "error");
    }
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
fn java_and_spring_profiles_warn_about_interrupts_and_transaction_proxies() {
    let project = project(
        r#"import org.springframework.transaction.annotation.Transactional;

class Example {
    void before() {
        work();
    }

    @Transactional(readOnly = true)
    void work() {}

    void work(int other) {}

    @Transactional
    private void internal() {}

    void after(Example another) {
        this.work();
        this.work(1);
        another.work();
    }

    class Inner {
        void after(Example another) {
            this.work();
        }
        void work() {}
    }

    void interrupted() throws InterruptedException {
        try { pause(); }
        catch (InterruptedException first) { logger.warn("Interrupted", first); }

        try { pause(); }
        catch (java.lang.InterruptedException second) { this.logger.error("Interrupted", second); return; }

        try { pause(); }
        catch (InterruptedException restored) { logger.warn("Interrupted", restored); Thread.currentThread().interrupt(); }

        try { pause(); }
        catch (InterruptedException rethrown) { logger.warn("Interrupted", rethrown); throw rethrown; }

        try { pause(); }
        catch (InterruptedException handled) { handler.warn(handled); }

        try { pause(); }
        catch (IllegalStateException unrelated) { logger.warn("Not interrupted", unrelated); }
    }

    @custom.Transactional
    private void customTransaction() {}

    void pause() throws InterruptedException {}
}
"#,
    );
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1
profiles: [spring-boot]
",
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 0);
    assert_eq!(result["status"], "passed");
    assert_eq!(result["complete"], true);
    let diagnostics = result["diagnostics"].as_array().unwrap();
    assert_eq!(
        diagnostics
            .iter()
            .filter(|finding| finding["rule_id"] != "java.fully-qualified-type")
            .map(|finding| (
                finding["rule_id"].as_str().unwrap(),
                finding["line"].as_u64().unwrap()
            ))
            .collect::<Vec<_>>(),
        [
            ("spring.transactional-self-invocation", 5),
            ("spring.private-transactional", 13),
            ("spring.transactional-self-invocation", 17),
            ("java.ignored-interrupt", 31),
            ("java.ignored-interrupt", 34),
        ],
        "{result}"
    );
    assert!(
        diagnostics
            .iter()
            .all(|finding| finding["severity"] == "warning")
    );
}

#[test]
#[ignore = "requires the pinned Semgrep and PMD engines"]
fn unnecessary_import_skips_unused_wildcards_but_reports_duplicates_and_explicit_imports() {
    let project = project(
        "import example.docs.\n    /* use in Javadoc */ *;\n\
         import static java.util.Objects.*;\n\
         import static java.util.Objects.*;\n\
         import java.io.InputStream;\n\
         /** See {@link DocType}. */\n\
         class Example {}\n",
    );
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [java]\n",
    )
    .unwrap();

    let result = report(check(root).output().unwrap(), 0);
    assert_eq!(
        unnecessary_import_lines(&result, "src/Example.java"),
        [4, 5],
        "{result}"
    );
}

fn unnecessary_import_lines(report: &Value, path: &str) -> Vec<u64> {
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

#[test]
#[ignore = "requires Maven 3, JDK 21, both source engines and the bundled JVM artifacts"]
fn maven_tracks_effective_compile_inputs_and_checks_custom_test_sources() {
    let _maven = MAVEN_TEST_LOCK
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    let main = root.join("src/main/java/example");
    let test = root.join("custom tests/example");
    fs::create_dir_all(&main).unwrap();
    fs::create_dir_all(&test).unwrap();
    let pom = r#"<project>
        <modelVersion>4.0.0</modelVersion>
        <groupId>example</groupId><artifactId>scope</artifactId><version>1</version>
        <properties><maven.compiler.release>17</maven.compiler.release><maven.compiler.testRelease>25</maven.compiler.testRelease></properties>
        <build><testSourceDirectory>custom tests</testSourceDirectory>
          <plugins><plugin><artifactId>maven-compiler-plugin</artifactId><version>3.13.0</version>
            <configuration><useIncrementalCompilation>false</useIncrementalCompilation>OPTIONS</configuration>
          </plugin></plugins>
        </build>
    </project>"#;
    fs::write(root.join("pom.xml"), pom.replace("OPTIONS", "")).unwrap();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [spring-boot, layered]\n",
    )
    .unwrap();
    fs::write(
        main.join("Example.java"),
        "package example; import java.net.*; public record Example(String name) {}\n",
    )
    .unwrap();
    fs::write(
        main.join("Other.java"),
        "package example; public class Other {}\n",
    )
    .unwrap();
    fs::write(
        test.join("BrokenTest.java"),
        "package example; import org.springframework.beans.factory.annotation.Autowired; class BrokenTest { @Autowired Object dependency; void example() { try { operation(); } catch (Exception _) {} } void operation() throws Exception {} }\n",
    ).unwrap();

    let first = report(check(root).output().unwrap(), 1);
    assert_eq!(first["files_scanned"], 3, "{first}");
    assert_eq!(
        unnecessary_import_lines(&first, "src/main/java/example/Example.java"),
        [1],
        "{first}"
    );
    let test_path = "custom tests/example/BrokenTest.java";
    let findings = first["diagnostics"].as_array().unwrap();
    assert!(
        findings
            .iter()
            .any(|item| item["path"] == test_path && item["rule_id"] == "java.empty-catch"),
        "{first}"
    );
    assert!(
        findings
            .iter()
            .all(|item| item["rule_id"] != "spring.field-injection"),
        "{first}"
    );

    fs::write(
        root.join("pom.xml"),
        pom.replace(
            "OPTIONS",
            "<excludes><exclude>**/Example.java</exclude></excludes>",
        ),
    )
    .unwrap();
    fs::write(
        main.join("Example.java"),
        "package example; import missing.types.*; public record Example(Needed name) {}\n",
    )
    .unwrap();
    let resolved = report(
        check(root)
            .args(["--resolve-imports", "--module", "example:scope"])
            .output()
            .unwrap(),
        3,
    );
    assert_eq!(resolved["complete"], false);
    assert_eq!(
        resolved["failures"][0]["path"],
        "src/main/java/example/Example.java"
    );
    assert!(root.join("target/classes/example/Example.class").is_file());
    assert!(
        unnecessary_import_lines(&resolved, "src/main/java/example/Example.java").is_empty(),
        "{resolved}"
    );
    assert_eq!(resolved["files_scanned"], 1, "{resolved}");

    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [java]\n",
    )
    .unwrap();
    fs::write(
        main.join("Other.java"),
        "package example; import java.net.*; public class Other {}\n",
    )
    .unwrap();
    let baseline = root.join(".dornguard-baseline.json");
    let create = || {
        let mut command = Command::new(env!("CARGO_BIN_EXE_dornguard"));
        command
            .current_dir(root)
            .args(["baseline", "create", "--resolve-imports"]);
        command
    };
    let output = create().output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let saved = fs::read(&baseline).unwrap();
    let baseline_json: Value = serde_json::from_slice(&saved).unwrap();
    assert!(
        baseline_json["violations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["path"] == "src/main/java/example/Other.java"
                && item["rule_id"] == "java.unnecessary-import"),
        "{baseline_json}"
    );
    let matched = report(
        check(root)
            .args(["--resolve-imports", "--baseline"])
            .arg(&baseline)
            .output()
            .unwrap(),
        0,
    );
    assert!(
        matched["diagnostics"].as_array().unwrap().is_empty(),
        "{matched}"
    );

    fs::write(
        main.join("Other.java"),
        "package example; public class Other { Missing missing; }\n",
    )
    .unwrap();
    let failed = create().arg("--force").output().unwrap();
    assert_eq!(failed.status.code(), Some(4));
    assert_eq!(fs::read(&baseline).unwrap(), saved);
}

#[test]
#[ignore = "requires the pinned Semgrep and PMD engines"]
fn source_only_can_select_java_25_syntax() {
    let project = project(
        "class Example { void parse() { try { Integer.parseInt(\"42\"); } catch (NumberFormatException _) {} } }\n",
    );
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [java]\n",
    )
    .unwrap();
    let failed = report(check(root).output().unwrap(), 3);
    assert_eq!(failed["failures"][0]["error_type"], "ParseException");
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [java]\njava_source: 25\n",
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 1);
    assert_eq!(result["diagnostics"][0]["rule_id"], "java.empty-catch");
    assert_eq!(result["complete"], true);
    assert_eq!(result["files_scanned"], 1);
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
#[ignore = "requires the pinned Semgrep and PMD engines"]
fn source_diagnostics_report_utf16_ranges_and_subjects() {
    let source = concat!(
        "import\n    java.io.InputStream;\n",
        "class Example {\n",
        "\tObject run() { /* 한😀 */ return java.util.\n",
        "\t    stream.Collectors.toList(); }\n",
        "\tvoid wrap() { try {} catch (Exception e) { /* 한😀 */ throw new IllegalArgumentException(); } }\n",
        "}\n",
    );
    let project = project(&source.replace('\n', "\r\n"));
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [java]\n",
    )
    .unwrap();
    let report = report(check(root).output().unwrap(), 0);
    let sarif = Command::new(env!("CARGO_BIN_EXE_dornguard"))
        .current_dir(root)
        .args(["check", "--format", "sarif"])
        .output()
        .unwrap();
    assert!(sarif.status.success());
    let sarif: Value = serde_json::from_slice(&sarif.stdout).unwrap();
    let run = &sarif["runs"][0];
    assert_eq!(run["columnKind"], "utf16CodeUnits");
    assert_eq!(report["version"], 3);
    let lines = source
        .lines()
        .map(|line| line.encode_utf16().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    for (id, subject, span) in [
        (
            "java.unnecessary-import",
            "java.io.InputStream",
            "import\n    java.io.InputStream;",
        ),
        (
            "java.fully-qualified-type",
            "java.util.stream.Collectors",
            "java.util.\n\t    stream.Collectors",
        ),
        (
            "java.preserve-stack-trace",
            "e",
            "throw new IllegalArgumentException();",
        ),
    ] {
        let item = report["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["rule_id"] == id)
            .unwrap();
        assert_eq!(item["subject"], subject);
        assert!(item["message"].as_str().unwrap().contains(subject));
        let number = |name: &str| item[name].as_u64().unwrap() as usize;
        let first = number("line") - 1;
        let last = number("end_line") - 1;
        let sliced = lines[first..=last]
            .iter()
            .enumerate()
            .map(|(index, line)| {
                let start = if index == 0 { number("column") - 1 } else { 0 };
                let end = if first + index == last {
                    number("end_column") - 1
                } else {
                    line.len()
                };
                String::from_utf16(&line[start..end]).unwrap()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(sliced, span, "{item}");
        assert!(item["fingerprint"].is_string());
        let sarif_item = run["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["ruleId"] == id)
            .unwrap();
        assert_eq!(sarif_item["properties"]["subject"], subject);
        let region = &sarif_item["locations"][0]["physicalLocation"]["region"];
        for (json, sarif) in [
            ("line", "startLine"),
            ("column", "startColumn"),
            ("end_line", "endLine"),
            ("end_column", "endColumn"),
        ] {
            assert_eq!(region[sarif], item[json]);
        }
    }
}

#[test]
#[ignore = "requires the pinned Semgrep and PMD engines and a JDK 21 runtime"]
fn java_resource_targets_and_optional_exception_logging_are_conservative() {
    let source = r#"import java.io.IOException;
import java.net.http.HttpClient;
import java.sql.Connection;
import java.sql.DriverManager;
import java.sql.SQLException;
import java.util.logging.Level;
import java.util.logging.Logger;

class Example {
    void http() {
        var client = HttpClient.newHttpClient();
        client.followRedirects();
    }

    record Lease(Connection connection) implements AutoCloseable {
        public void close() throws SQLException { connection.close(); }
    }

    Lease open() throws SQLException {
        Connection managed = DriverManager.getConnection("jdbc:example");
        return new Lease(managed);
    }

    void caller() throws SQLException {
        Lease lease = open();
        try { lease.connection().isValid(1); }
        finally { lease.close(); }
    }

    void leak() throws SQLException {
        Connection leaked = DriverManager.getConnection("jdbc:example");
        leaked.isValid(1);
    }

    static final class Workbook implements AutoCloseable {
        void write() throws IOException { throw new IOException(); }
        public void close() {}
    }

    void exportWithUnsafeHelper() throws IOException {
        Workbook unsafe = new Workbook();
        writeAndClose(unsafe);
    }

    void writeAndClose(Workbook workbook) throws IOException {
        workbook.write();
        workbook.close();
    }

    void exportWithResourceScope() throws IOException {
        try (Workbook workbook = new Workbook()) {
            workbook.write();
        }
    }

    void closesPattern(Object resource) throws Exception {
        if (resource instanceof AutoCloseable handled) {
            handled.close();
        }
    }

    void closesConditionally(Object resource, boolean close) throws Exception {
        if (resource instanceof AutoCloseable unhandled) {
            if (close) unhandled.close();
        }
    }

    void logged() {
        try { fail(); }
        catch (IOException logged) {
            Logger.getGlobal().log(Level.WARNING, "request failed", logged);
            System.nanoTime();
            throw new IllegalArgumentException();
        }
    }

    void sometimesLogged(boolean log) {
        try { fail(); }
        catch (IOException conditional) {
            if (log) Logger.getGlobal().log(Level.WARNING, "request failed", conditional);
            throw new IllegalArgumentException();
        }
    }

    void logsMessageOnly() {
        try { fail(); }
        catch (IOException original) {
            Logger.getGlobal().log(Level.WARNING, original.getMessage());
            throw new IllegalArgumentException();
        }
    }

    void reassigned() {
        try { fail(); }
        catch (IOException reassigned) {
            reassigned = new IOException();
            Logger.getGlobal().log(Level.WARNING, "request failed", reassigned);
            throw new IllegalArgumentException();
        }
    }

    void preserves() {
        try { fail(); }
        catch (IOException original) {
            throw new IllegalArgumentException(original);
        }
    }

    Class<?> find(String name) throws ClassNotFoundException {
        try { return Class.forName(name); }
        catch (ClassNotFoundException e) {
            throw new ClassNotFoundException(name, e);
        }
    }

    void fail() throws IOException {}
}

class Other {
    static class HttpClient implements AutoCloseable {
        void used() {}
        public void close() {}
    }
    void custom() {
        HttpClient custom = new HttpClient();
        custom.used();
    }
}
"#;
    let project = project(source);
    let root = project.path();
    let findings = |release, allow_logged| {
        fs::write(root.join(".dornguard.yml"), format!("version: 1\nprofiles: [java]\njava_target: {release}\nallow_logged_exceptions: {allow_logged}\n")).unwrap();
        let result = report(check(root).output().unwrap(), 0);
        let diagnostics = result["diagnostics"].as_array().unwrap();
        let resources = diagnostics
            .iter()
            .filter(|item| item["rule_id"] == "java.resource-not-closed")
            .map(|item| {
                let line = item["line"].as_u64().unwrap() as usize;
                source.lines().nth(line - 1).unwrap().trim()
            })
            .collect::<Vec<_>>();
        let exceptions = diagnostics
            .iter()
            .filter(|item| item["rule_id"] == "java.preserve-stack-trace")
            .map(|item| item["subject"].as_str().unwrap().to_owned())
            .collect::<Vec<_>>();
        (resources, exceptions)
    };
    let (resources, exceptions) = findings(21, false);
    assert_eq!(
        resources,
        [
            "var client = HttpClient.newHttpClient();",
            "Connection leaked = DriverManager.getConnection(\"jdbc:example\");",
            "Workbook unsafe = new Workbook();",
            "if (resource instanceof AutoCloseable unhandled) {",
            "HttpClient custom = new HttpClient();"
        ]
    );
    assert_eq!(
        exceptions,
        ["logged", "conditional", "original", "reassigned"]
    );
    let (resources, exceptions) = findings(17, true);
    assert_eq!(
        resources,
        [
            "Connection leaked = DriverManager.getConnection(\"jdbc:example\");",
            "Workbook unsafe = new Workbook();",
            "if (resource instanceof AutoCloseable unhandled) {",
            "HttpClient custom = new HttpClient();"
        ]
    );
    assert_eq!(exceptions, ["conditional", "original", "reassigned"]);
}

#[test]
#[ignore = "requires the pinned Semgrep and PMD engines"]
fn spring_profile_warns_about_unnecessary_imports_and_shared_formatters() {
    let source = r#"package example;

import java.io.InputStream;
import java.lang.String;
import java.util.List;
import java.util.List;
import java.text.DateFormat;
import java.text.SimpleDateFormat;
import java.time.format.DateTimeFormatter;
import org.springframework.beans.factory.config.ConfigurableBeanFactory;
import org.springframework.context.annotation.Configuration;
import org.springframework.context.annotation.Scope;
import org.springframework.stereotype.Service;
import org.springframework.web.bind.annotation.RestController;
import org.springframework.web.context.annotation.RequestScope;

@Service("dates")
class Dates {
    private final SimpleDateFormat shared = new SimpleDateFormat("yyyy");
    private final DateFormat indirect = new SimpleDateFormat("yyyy");
    private SimpleDateFormat fromConstructor;
    private final DateTimeFormatter safe = DateTimeFormatter.ISO_LOCAL_DATE;
    private final ThreadLocal<SimpleDateFormat> perThread = ThreadLocal.withInitial(() -> new SimpleDateFormat("yyyy"));
    private final List<String> values = List.of();
    void render() {
        SimpleDateFormat methodLocal = new SimpleDateFormat("yyyy");
    }
    class Nested {
        SimpleDateFormat nested;
    }
}

@RestController
@Scope(scopeName = ConfigurableBeanFactory.SCOPE_SINGLETON)
class ExplicitSingleton {
    SimpleDateFormat explicit;
}

@Service
@Scope("prototype")
class Prototype {
    SimpleDateFormat prototype;
}

@Service
@RequestScope
class PerRequest {
    SimpleDateFormat perRequest;
}

@Configuration
class ConfigurationBean {
    SimpleDateFormat inConfiguration;
}

@custom.Service
class Unmanaged {
    SimpleDateFormat unmanaged;
}
"#;
    let project = project(source);
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [spring-boot]\n",
    )
    .unwrap();

    let result = report(check(root).output().unwrap(), 0);
    assert_eq!(result["status"], "passed");
    assert_eq!(result["complete"], true);
    assert_eq!(result["files_scanned"], 1);
    let diagnostics = result["diagnostics"].as_array().unwrap();
    let reported = |rule: &str| {
        diagnostics
            .iter()
            .filter(|finding| finding["rule_id"] == rule)
            .map(|finding| {
                let line = finding["line"].as_u64().unwrap() as usize;
                source.lines().nth(line - 1).unwrap().trim()
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        reported("java.unnecessary-import"),
        [
            "import java.io.InputStream;",
            "import java.lang.String;",
            "import java.util.List;"
        ],
        "{result}"
    );
    assert_eq!(
        reported("spring.shared-simple-date-format"),
        [
            "private final SimpleDateFormat shared = new SimpleDateFormat(\"yyyy\");",
            "private final DateFormat indirect = new SimpleDateFormat(\"yyyy\");",
            "private SimpleDateFormat fromConstructor;",
            "SimpleDateFormat explicit;",
            "SimpleDateFormat inConfiguration;",
        ],
        "{result}"
    );
    assert!(
        diagnostics
            .iter()
            .all(|finding| finding["severity"] == "warning")
    );
}

#[test]
#[ignore = "requires the pinned Semgrep and PMD engines"]
fn java_and_spring_profiles_review_exception_causes_resources_and_request_state() {
    let source = r#"package example;

import java.io.IOException;
import java.io.InputStream;
import java.nio.file.Files;
import java.nio.file.Path;
import java.sql.SQLException;
import org.springframework.context.annotation.Scope;
import org.springframework.transaction.annotation.Transactional;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;
import org.springframework.web.context.annotation.RequestScope;

@RestController
class Books {
    private String lastUser;

    @GetMapping("/books")
    String handler(@RequestParam String user) {
        this.lastUser = user;
        return user;
    }

    String helper(String user) {
        this.lastUser = user;
        return user;
    }

    @custom.GetMapping
    String customHandler(String user) {
        this.lastUser = user;
        return user;
    }

    @GetMapping("/local")
    String local(String user) {
        String copy = user;
        this.lastUser = "constant";
        return copy;
    }

    class Nested {
        private String lastUser;
        @GetMapping
        String handler(String user) {
            this.lastUser = user;
            return user;
        }
    }

    int open(Path file) throws IOException {
        InputStream in = Files.newInputStream(file);
        return in.read();
    }

    int closed(Path file) throws IOException {
        try (InputStream in = Files.newInputStream(file)) {
            return in.read();
        }
    }

    int invalid(String value) {
        try { return Integer.parseInt(value); }
        catch (NumberFormatException failure) { throw new IllegalStateException(failure.getMessage()); }
    }

    int wrapped(String value) {
        try { return Integer.parseInt(value); }
        catch (NumberFormatException failure) { throw new IllegalStateException("Invalid", failure); }
    }

    @Transactional
    void update() throws IOException {
        if (lastUser == null) {
            throw new IOException("rollback review");
        }
    }

    @Transactional()
    void updateSql() throws SQLException {
        if (lastUser == null) throw new SQLException("rollback review");
    }

    @Transactional(rollbackFor = IOException.class)
    void configured() throws IOException {
        throw new IOException("already configured");
    }

    @Transactional
    void caught() {
        try { throw new IOException("caught"); }
        catch (IOException failure) { lastUser = failure.getMessage(); }
    }

    @Transactional
    Task deferred() {
        return () -> { throw new IOException("deferred"); };
    }

    interface Task {
        void run() throws IOException;
    }
}

@RestController
@RequestScope
class PerRequest {
    private String user;
    @GetMapping
    String save(String arg) {
        this.user = arg;
        return arg;
    }
}

@RestController
@Scope("prototype")
class Prototype {
    private String user;
    @GetMapping
    String save(String arg) {
        this.user = arg;
        return arg;
    }
}

@RestController
@Scope("singleton")
class ExplicitSingleton {
    private String user;
    @GetMapping
    String save(String arg) {
        this.user = arg + "!";
        this.user = arg;
        return arg;
    }
}
"#;
    let project = project(source);
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [spring-boot]\n",
    )
    .unwrap();

    let asserted_findings = |outcome: &Value| {
        assert_eq!(outcome["complete"], true);
        let findings = outcome["diagnostics"].as_array().unwrap();
        assert!(findings.iter().all(|item| item["severity"] == "warning"));
        findings
            .iter()
            .filter(|finding| finding["rule_id"] != "java.fully-qualified-type")
            .map(|finding| {
                let line = finding["line"].as_u64().unwrap() as usize;
                (
                    finding["rule_id"].as_str().unwrap().to_owned(),
                    source.lines().nth(line - 1).unwrap().trim(),
                )
            })
            .collect::<Vec<_>>()
    };
    let default = report(check(root).output().unwrap(), 0);
    assert_eq!(
        asserted_findings(&default),
        [
            (
                "spring.singleton-request-state".to_owned(),
                "this.lastUser = user;"
            ),
            (
                "java.resource-not-closed".to_owned(),
                "InputStream in = Files.newInputStream(file);"
            ),
            (
                "java.preserve-stack-trace".to_owned(),
                "catch (NumberFormatException failure) { throw new IllegalStateException(failure.getMessage()); }"
            ),
            (
                "spring.singleton-request-state".to_owned(),
                "this.user = arg;"
            ),
        ],
        "{default}"
    );

    let enabled = report(
        check(root)
            .args(["--profile", "spring-rollback"])
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(
        asserted_findings(&enabled)
            .into_iter()
            .filter(|(id, _)| id == "spring.checked-exception-rollback")
            .map(|(_, line)| line)
            .collect::<Vec<_>>(),
        [
            "throw new IOException(\"rollback review\");",
            "if (lastUser == null) throw new SQLException(\"rollback review\");",
        ],
        "{enabled}"
    );
}

#[test]
#[ignore = "requires the pinned Semgrep engine"]
fn java_fully_qualified_types_are_reported_in_source_code() {
    let project = project(
        r#"package example; import java.util.Map;
class Example extends java.util.ArrayList<String> {
    // java.util.Date
    /* java.io.InputStream */
    java.util.List<java.time.Instant> staticallyTyped() {
        return java.util.List.of();
    }
    @java.lang.Deprecated
    demo.Widget widget;
    Class<?> entry = java.util.Map.Entry.class;
    String string = "java.util.Date";
    String textBlock = """
        java.util.Date
        """;
    Map<?, ?> map;
    record Payment(int DEFAULT_TIMEOUT) {}
    record Config(Payment payment) {}
    int timeout(Config config) {
        return config.payment.DEFAULT_TIMEOUT;
    }
    int localTimeout() {
        Config config = new Config(new Payment(30));
        return config.payment.DEFAULT_TIMEOUT;
    }
    void namespaces(Config java) {
        java.util.Map<?, ?> map = null;
    }
}
"#,
    );
    let result = report(check(project.path()).output().unwrap(), 0);
    assert_eq!(result["complete"], true);
    let findings = result["diagnostics"].as_array().unwrap();
    assert!(
        findings
            .iter()
            .all(|item| item["rule_id"] == "java.fully-qualified-type")
    );
    assert!(findings.iter().all(|item| item["severity"] == "warning"));
    assert_eq!(
        findings
            .iter()
            .map(|item| item["line"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [2, 5, 5, 6, 8, 9, 10, 26],
        "{result}"
    );
}

#[test]
#[ignore = "requires the pinned Semgrep engine"]
fn html_profile_flags_inline_javascript_and_skips_non_executable_blocks() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [html, thymeleaf]\n",
    )
    .unwrap();
    const PAGE: &str = r#"<script>run()</script>
<script type="module">run()</script>
<script SRC="/app.js"></script>
<script th:src="@{/app.js}"></script>
<script type="application/ld+json">{"name":"demo"}</script>
<script type="application/json">{"name":"demo"}</script>
<script></script>
<button onclick="run()">Click</button>
<input data-th-oninput="${useHandler}">
<span once="text">text</span>
<input th:src="${@outsideTemplate.file()}">
"#;
    fs::write(root.join("index.html"), PAGE).unwrap();
    let result = report(check(root).output().unwrap(), 0);
    assert_eq!(result["complete"], true);
    assert_eq!(result["files_scanned"], 1);
    let findings = result["diagnostics"].as_array().unwrap();
    assert_eq!(
        findings
            .iter()
            .map(|item| (
                item["rule_id"].as_str().unwrap(),
                item["line"].as_u64().unwrap()
            ))
            .collect::<Vec<_>>(),
        [
            ("html.inline-script", 1),
            ("html.inline-script", 2),
            ("html.inline-event-handler", 8),
            ("html.inline-event-handler", 9),
        ],
        "{result}"
    );
}

#[test]
#[ignore = "requires the pinned Semgrep and PMD engines"]
fn spring_self_calls_check_unambiguous_parameterized_methods() {
    let project = project(
        r#"import org.springframework.scheduling.annotation.Async;
import org.springframework.transaction.annotation.Transactional;
class Example {
    @Transactional void save(String name, int count) {}
    void save(int count) {}
    @Async void send(String payload) {}
    @Async void ambiguous(String payload) {}
    void ambiguous(int count) {}
    @Async void variadic(String... payloads) {}
    @Async static void staticTarget(String payload) {}
    void use(Example other) {
        this.save("book", 1);
        send("message");
        save(1);
        other.send("other");
        ambiguous(1);
        variadic("skip");
        staticTarget("skip");
        Runnable nested = new Runnable() {
            @Override public void run() { send("outer"); }
        };
    }
}
class Inherited extends Example {
    @Async void inherited(String value) {}
    void useInherited() { inherited("skip"); }
}
"#,
    );
    let result = report(
        check(project.path())
            .args(["--profile", "spring-boot"])
            .output()
            .unwrap(),
        0,
    );
    assert!(result["complete"].as_bool().unwrap(), "{result}");
    let calls = result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| {
            item["rule_id"]
                .as_str()
                .unwrap()
                .ends_with("-self-invocation")
        })
        .map(|item| {
            (
                item["line"].as_u64().unwrap(),
                item["subject"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(calls, [(12, "save"), (13, "send")], "{result}");
}

#[test]
fn jpa_web_checks_resolved_entity_types_across_sources() {
    let project = project(
        r#"package example.web;
import example.model.Book;
import example.model.Legacy;
import java.util.List;
import org.springframework.http.ResponseEntity;
import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RestController;

@RestController
class Example {
    @GetMapping Book direct() { return null; }
    @GetMapping ResponseEntity<List<Book>> wrapped() { return null; }
    @GetMapping Legacy[] array() { return null; }
    @GetMapping List<? extends Book> wildcard() { return null; }
    @GetMapping BookDto dto() { return null; }
    @GetMapping <Book> Book typeVariable() { return null; }
    Book helper() { return null; }
}
class BookDto {}
@org.springframework.stereotype.Controller
class Mvc {
    @GetMapping Book view() { return null; }
    @GetMapping ResponseEntity<Book> explicitResponse() { return null; }
    @GetMapping @org.springframework.web.bind.annotation.ResponseBody Book responseBody() { return null; }
}
"#,
    );
    let root = project.path();
    let model = root.join("src/model");
    fs::create_dir_all(&model).unwrap();
    let book = model.join("Book.java");
    fs::write(
        &book,
        "package example.model; @jakarta.persistence.Entity public class Book {}\n",
    )
    .unwrap();
    fs::write(
        model.join("Legacy.java"),
        "package example.model; import javax.persistence.Entity; @Entity public class Legacy {}\n",
    )
    .unwrap();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [jpa-web]\n",
    )
    .unwrap();
    let result = report(check(root).arg("src/Example.java").output().unwrap(), 0);
    let diagnostics = result["diagnostics"].as_array().unwrap();
    assert_eq!(
        diagnostics
            .iter()
            .map(|item| item["line"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        [11, 12, 13, 14, 23, 24],
        "{result}"
    );
    assert_eq!(diagnostics[2]["subject"], "example.model.Legacy");
    assert_eq!(result["files_scanned"], 1);

    fs::write(
        root.join("src/LocalBook.java"),
        "package example.web; class Book {}\n",
    )
    .unwrap();
    let controller = root.join("src/Example.java");
    let source = fs::read_to_string(&controller)
        .unwrap()
        .replace("import example.model.Book;", "import example.model.*;");
    fs::write(controller, source).unwrap();
    let shadowed = report(check(root).arg("src/Example.java").output().unwrap(), 0);
    let findings = shadowed["diagnostics"].as_array().unwrap();
    assert_eq!(findings.len(), 1, "{shadowed}");
    assert_eq!(findings[0]["subject"], "example.model.Legacy");
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
fn exclusive_format_uses_file_overrides_and_skips_vendor_sources() {
    let project = project("class Example {}\n");
    let root = project.path();
    fs::create_dir_all(root.join("legacy")).unwrap();
    fs::write(root.join("main.js"), "if (true) {\n  run();\n}\n").unwrap();
    fs::write(root.join("legacy/main.js"), "if (true) {\n    run();\n}\n").unwrap();
    fs::write(root.join("legacy/vendor.min.js"), [0xff]).unwrap();
    fs::write(root.join("vendor.min.js"), [0xff]).unwrap();
    fs::write(
        root.join(".dornguard.yml"),
        r#"
version: 1
profiles: [spring-boot]
exclude: ["**/*.min.js"]
format:
  overrides:
    - files: ["legacy/**"]
      indent_size: 4
"#,
    )
    .unwrap();
    let clean = report(
        check(root)
            .args(["--only-profile", "format"])
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(clean["files_requested"], 3, "{clean}");
    assert!(clean["analysis_notes"].is_null());
    assert_eq!(clean["rule_counts"], serde_json::json!({}));
    fs::write(root.join("legacy/main.js"), "if (true) {\n  run();\n}\n").unwrap();
    let findings = report(
        check(root)
            .args(["--only-profile", "format"])
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(findings["diagnostics"][0]["path"], "legacy/main.js");
    assert_eq!(findings["rule_counts"]["format.indentation"]["errors"], 1);
}

#[test]
#[ignore = "requires the pinned Semgrep engine"]
fn thymeleaf_allows_scoped_trusted_output_and_html_comment_suppression() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    let dir = root.join("src/main/resources/templates");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        root.join(".dornguard.yml"),
        r#"
version: 1
profiles: [thymeleaf-output-review]
thymeleaf_sanitizers: ["@htmlSanitizer.clean"]
"#,
    )
    .unwrap();
    fs::write(
        dir.join("sample.html"),
        r#"
<section th:with="safe=${@htmlSanitizer.clean(value)}">
    <div th:utext="${safe}"></div>
    <div th:each="safe : ${items}" th:utext="${safe}"></div>
    <div th:with="safe=${raw}" th:utext="${safe}"></div>
</section>
<div data-th-utext="${@htmlSanitizer.clean(value)}"></div>
<div th:utext="${@htmlSanitizer.clean(value)} + ${raw}"></div>
<div th:utext="${safe}"></div>
<!-- dornguard-ignore thymeleaf.unescaped-output -- Source is reviewed -->
<div th:utext="${reviewed}"></div>
"#,
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 1);
    assert_eq!(result["suppressed_count"], 1, "{result}");
    assert_eq!(
        result["rule_counts"]["thymeleaf.restricted-bean-access"]["errors"], 2,
        "{result}"
    );
    assert_eq!(
        result["rule_counts"]["thymeleaf.unescaped-output"]["warnings"], 4,
        "{result}"
    );
    let lines = result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["rule_id"] == "thymeleaf.unescaped-output")
        .map(|finding| finding["line"].as_u64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(lines, [4, 5, 8, 9], "{result}");

    fs::write(
        dir.join("broken.html"),
        r#"<html><body><th:block th:if="${a}"><br"#,
    )
    .unwrap();
    let partial = report(check(root).output().unwrap(), 3);
    assert_eq!(partial["complete"], false);
    assert_eq!(partial["files_requested"], 2);
    assert_eq!(partial["files_scanned"], 1);
    assert!(
        partial["failures"]
            .as_array()
            .unwrap()
            .iter()
            .any(|failure| failure["error_type"] == "PartialParsing"
                && failure["path"] == "src/main/resources/templates/broken.html"
                && failure["rule_ids"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|id| id == "thymeleaf.unescaped-output")),
        "{partial}"
    );
}

#[test]
#[ignore = "requires the pinned Java source engines"]
fn equivalent_transactions_keep_only_calls_with_distinct_boundaries() {
    let project = project(
        r#"
import org.springframework.transaction.annotation.Transactional;
import org.springframework.transaction.annotation.Propagation;
class Example {
    @Transactional
    public void outer() {
        equivalent();
        requiresNew();
        readOnly();
        Runnable deferred = () -> equivalent();
    }
    @Transactional(propagation = Propagation.REQUIRED, readOnly = false)
    public void equivalent() {}
    @Transactional(propagation = Propagation.REQUIRES_NEW)
    public void requiresNew() {}
    @Transactional(readOnly = true)
    public void readOnly() {}
    @Transactional
    public void catches() {
        try {
            equivalent();
        } catch (RuntimeException e) {
            throw new IllegalStateException(e);
        }
    }
}
"#,
    );
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [spring-boot]\n",
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 0);
    let lines = result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["rule_id"] == "spring.transactional-self-invocation")
        .map(|finding| finding["line"].as_u64().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(lines, [8, 9, 10, 21], "{result}");
}

#[test]
#[ignore = "requires the pinned Java source engines and a JDK 21 runtime"]
fn resources_accept_configured_ownership_and_executor_cleanup_without_hiding_leaks() {
    let project = project(
        r#"
package example;
import java.sql.Connection;
import java.sql.DriverManager;
import java.sql.SQLException;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
class Example {
    void transfers() throws SQLException {
        Connection transferred = DriverManager.getConnection("jdbc:example");
        Handle.take(transferred);
    }
    void conditional(boolean transfer) throws SQLException {
        Connection leaked = DriverManager.getConnection("jdbc:example");
        if (transfer) Handle.take(leaked);
    }
    void stopped() {
        ExecutorService stopped = Executors.newSingleThreadExecutor();
        try { stopped.execute(() -> {}); } finally { stopped.shutdown(); }
    }
    void live() {
        ExecutorService live = Executors.newSingleThreadExecutor();
        live.execute(() -> {});
    }
    void managed() throws SQLException {
        Connection shared = AppResources.connection();
        shared.getCatalog();
    }
}
class AppResources {
    static Connection connection() { return null; }
}
record Handle(Connection connection) {
    static void take(Connection connection) {}
}
"#,
    );
    let root = project.path();
    let config = "version: 1\nprofiles: [java]\njava_target: 21\n";
    let findings = |extra| {
        fs::write(root.join(".dornguard.yml"), format!("{config}{extra}")).unwrap();
        let result = report(check(root).output().unwrap(), 0);
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|finding| finding["rule_id"] == "java.resource-not-closed")
            .map(|finding| finding["line"].as_u64().unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(findings(""), [10, 14, 22, 26]);
    assert_eq!(
        findings(concat!(
            "java_resource_owners: ['example.Handle#take(java.sql.Connection)']\n",
            "java_managed_resource_factories: ['example.AppResources#connection()']\n",
        )),
        [14, 22]
    );
}

#[test]
#[ignore = "requires Maven 3, JDK 21 and the bundled JVM artifacts"]
fn maven_architecture_rejects_uncompiled_sources_and_groups_the_same_dependency() {
    let _maven = MAVEN_TEST_LOCK
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    let pom = r#"<project>
        <modelVersion>4.0.0</modelVersion>
        <groupId>example</groupId><artifactId>coverage</artifactId><version>1</version>
        <properties><maven.compiler.release>17</maven.compiler.release>OPTIONS</properties>
        <build><plugins><plugin><artifactId>maven-compiler-plugin</artifactId><version>3.13.0</version></plugin></plugins></build>
    </project>"#;
    let config = "version: 1\nprofiles: [layered]\n";
    let controller = "src/main/java/example/controller/Controller.java";
    let repo = "src/main/java/example/repository/Repo.java";
    fs::write(root.join(".dornguard.yml"), config).unwrap();
    for path in [controller, repo] {
        fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
    }
    fs::write(root.join("pom.xml"), pom.replace("OPTIONS", "")).unwrap();
    fs::write(
        root.join(repo),
        "package example.repository; public class Repo { public void query() {} }\n",
    )
    .unwrap();
    fs::write(
        root.join(controller),
        "package example.controller; public class Controller {}\n",
    )
    .unwrap();
    let clean = report(check(root).output().unwrap(), 0);
    assert_eq!(clean["complete"], true);
    fs::write(
        root.join("pom.xml"),
        pom.replace("OPTIONS", "<maven.main.skip>true</maven.main.skip>"),
    )
    .unwrap();
    fs::write(
        root.join(controller),
        r#"package example.controller;
import example.repository.Repo;
public class Controller {
    private final Repo repo;
    Controller(Repo repo) { this.repo = repo; }
    void query() { repo.query(); }
}
"#,
    )
    .unwrap();
    let skipped = report(check(root).output().unwrap(), 3);
    assert_eq!(skipped["complete"], false);
    assert!(
        skipped["failures"][0]["rule_ids"]
            .as_array()
            .unwrap()
            .iter()
            .any(|id| id == "architecture.controller-repository"),
        "{skipped}"
    );
    fs::write(root.join("pom.xml"), pom.replace("OPTIONS", "")).unwrap();
    let violation = report(check(root).output().unwrap(), 1);
    assert_eq!(
        violation["diagnostics"].as_array().unwrap().len(),
        1,
        "{violation}"
    );
    assert_eq!(violation["diagnostics"][0]["path"], controller);
    assert_eq!(violation["diagnostics"][0]["line"], 6);

    fs::OpenOptions::new()
        .write(true)
        .open(root.join(controller))
        .unwrap()
        .set_modified(std::time::SystemTime::now() + std::time::Duration::from_secs(60))
        .unwrap();
    let stale = report(check(root).output().unwrap(), 3);
    assert_eq!(stale["complete"], false);
    assert_eq!(stale["failures"][0]["path"], controller);
    assert!(
        stale["failures"][0]["message"]
            .as_str()
            .unwrap()
            .contains("older than its source"),
        "{stale}"
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
    fs::write(&config, "version: 1\nprofiles: [format]\n").unwrap();
    let before = report(check(root).output().unwrap(), 1);
    assert_eq!(before["diagnostics"].as_array().unwrap().len(), 10);

    fs::write(
        &config,
        r#"version: 1
profiles: [format]
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
    fs::write(&policy, "version: 1\nprofiles: [format]\n").unwrap();
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
