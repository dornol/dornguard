#![allow(clippy::unwrap_used)]
use std::fs;
use std::path::Path;

mod common;
use common::{check, commit_fixture, copy_gradle_fixture, git, report, unnecessary_import_lines};

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
    // Test sources are skipped by default, so selecting one has nothing to report.
    let test = report(
        check(root)
            .args(["--module", ":api", test_path])
            .output()
            .unwrap(),
        1,
    );
    assert_eq!(test["files_requested"], 0, "{test}");
    assert!(
        test["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["path"] != test_path),
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
            .all(|item| item["path"] != test_path),
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

#[test]
#[ignore = "requires the pinned engines, JDK 21, Gradle and DORNGUARD_ARCHUNIT_RUNNER"]
fn package_cycles_report_only_dependencies_inside_a_cycle() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/spring-boot-multimodule");
    let project = tempfile::tempdir().unwrap();
    copy_gradle_fixture(&fixture, project.path());
    let root = project.path();

    let base = root.join("admin/src/main/java/example/admin");
    for (path, source) in [
        (
            "alpha/Alpha.java",
            "package example.admin.alpha;\npublic class Alpha {\n    public Object other() { return new example.admin.beta.Beta(); }\n}\n",
        ),
        (
            "beta/Beta.java",
            "package example.admin.beta;\npublic class Beta {\n    public Object other() { return new example.admin.alpha.Alpha(); }\n}\n",
        ),
        (
            "alpha/AlphaToo.java",
            "package example.admin.alpha;\npublic class AlphaToo {\n    public Object other() { return new example.admin.beta.Beta(); }\n}\n",
        ),
        (
            "gamma/Gamma.java",
            "package example.admin.gamma;\npublic class Gamma {\n    public Object other() { return new example.admin.alpha.Alpha(); }\n}\n",
        ),
    ] {
        let file = base.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, source).unwrap();
    }
    let config = fs::read_to_string(root.join(".dornguard.yml"))
        .unwrap()
        .replace(
            "profiles: [layered, rest-api]",
            "profiles: [layered, rest-api, package-cycles]",
        );
    fs::write(root.join(".dornguard.yml"), config).unwrap();

    let result = report(check(root).output().unwrap(), 1);
    assert_eq!(result["complete"], true, "{result}");
    let mut cycles = result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["rule_id"] == "architecture.package-cycle")
        .map(|finding| {
            (
                finding["class_name"].as_str().unwrap().to_owned(),
                finding["path"].as_str().unwrap().to_owned(),
                finding["line"].as_u64().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    cycles.sort_unstable();
    // Two classes in alpha depend on beta, but that is one package edge.
    let edge = result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|finding| {
            finding["rule_id"] == "architecture.package-cycle"
                && finding["class_name"] == "example.admin.alpha.Alpha"
        })
        .unwrap();
    let message = edge["message"].as_str().unwrap();
    assert!(
        message.contains(
            "Package example.admin.alpha depends on example.admin.beta through 2 class dependencies"
        ),
        "{message}"
    );
    assert_eq!(cycles.len(), 2, "{result}");
    assert_eq!(
        cycles,
        [
            (
                "example.admin.alpha.Alpha".to_owned(),
                "admin/src/main/java/example/admin/alpha/Alpha.java".to_owned(),
                3
            ),
            (
                "example.admin.beta.Beta".to_owned(),
                "admin/src/main/java/example/admin/beta/Beta.java".to_owned(),
                3
            ),
        ],
        "Gamma depends on the cycle but is not part of it: {result}"
    );
}

#[test]
#[ignore = "requires the pinned engines, JDK 21, Gradle and DORNGUARD_ARCHUNIT_RUNNER"]
fn hexagonal_layered_and_library_profiles_each_report_their_own_violations() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/spring-boot-multimodule");
    let project = tempfile::tempdir().unwrap();
    copy_gradle_fixture(&fixture, project.path());
    let root = project.path();

    let build = root.join("build.gradle.kts");
    let script = fs::read_to_string(&build).unwrap();
    fs::write(
        &build,
        format!(
            "{script}\nconfigure(listOf(project(\":api\"), project(\":common\"))) {{\n    repositories.mavenCentral()\n    dependencies {{\n        \"implementation\"(platform(\"org.springframework.boot:spring-boot-dependencies:3.5.16\"))\n        \"implementation\"(\"org.springframework:spring-context\")\n    }}\n}}\n"
        ),
    )
    .unwrap();
    for (path, source) in [
        (
            "api/src/main/java/example/api/inbound/Inbound.java",
            "package example.api.inbound;\npublic class Inbound {\n    Object other = new example.api.outbound.Outbound();\n}\n",
        ),
        (
            "api/src/main/java/example/api/outbound/Outbound.java",
            "package example.api.outbound;\npublic class Outbound {}\n",
        ),
        (
            "api/src/main/java/example/api/application/Orchestrator.java",
            "package example.api.application;\npublic class Orchestrator {\n    Object adapter = new example.api.adapter.BookAdapter();\n}\n",
        ),
        (
            "api/src/main/java/example/api/domain/Managed.java",
            "package example.api.domain;\n@org.springframework.stereotype.Component\npublic class Managed {}\n",
        ),
        (
            "admin/src/main/java/example/admin/repository/AdminRepository.java",
            "package example.admin.repository;\npublic final class AdminRepository {\n    Class<?> upward = example.admin.service.AdminService.class;\n}\n",
        ),
        (
            "common/src/main/java/example/common/Managed.java",
            "package example.common;\n@org.springframework.stereotype.Component\npublic class Managed {}\n",
        ),
    ] {
        let file = root.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, source).unwrap();
    }

    let result = report(check(root).output().unwrap(), 1);
    assert_eq!(result["complete"], true, "{result}");
    let mut findings = result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| {
            finding["rule_id"]
                .as_str()
                .is_some_and(|rule| rule.starts_with("architecture."))
        })
        .map(|finding| {
            (
                finding["rule_id"].as_str().unwrap().to_owned(),
                finding["class_name"].as_str().unwrap().to_owned(),
            )
        })
        .collect::<Vec<_>>();
    findings.sort_unstable();
    let expected = [
        (
            "architecture.adapter-isolation",
            "example.api.inbound.Inbound",
        ),
        (
            "architecture.application-independence",
            "example.api.application.Orchestrator",
        ),
        (
            "architecture.framework-independence",
            "example.api.domain.Managed",
        ),
        (
            "architecture.library-framework-independence",
            "example.common.Managed",
        ),
        (
            "architecture.repository-independence",
            "example.admin.repository.AdminRepository",
        ),
    ]
    .map(|(rule, class)| (rule.to_owned(), class.to_owned()));
    assert_eq!(findings, expected, "{result}");
}

#[test]
#[ignore = "requires the pinned engines, JDK 21, Gradle and DORNGUARD_ARCHUNIT_RUNNER"]
fn a_baselined_cycle_stays_baselined_when_its_reported_file_changes() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/spring-boot-multimodule");
    let project = tempfile::tempdir().unwrap();
    copy_gradle_fixture(&fixture, project.path());
    let root = project.path();

    let base = root.join("admin/src/main/java/example/admin");
    let class = |package: &str, name: &str, target: &str, padding: &str| {
        format!(
            "package example.admin.{package};\npublic class {name} {{\n{padding}    public Object other() {{ return new {target}(); }}\n}}\n"
        )
    };
    let write = |path: &str, source: String| {
        let file = base.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, source).unwrap();
    };
    // Two classes of alpha depend on beta, so they form one package edge.
    write(
        "alpha/Alpha.java",
        class("alpha", "Alpha", "example.admin.beta.Beta", ""),
    );
    write(
        "alpha/AlphaToo.java",
        class("alpha", "AlphaToo", "example.admin.beta.Beta", ""),
    );
    write(
        "beta/Beta.java",
        class("beta", "Beta", "example.admin.alpha.Alpha", ""),
    );
    let config = fs::read_to_string(root.join(".dornguard.yml"))
        .unwrap()
        .replace(
            "profiles: [layered, rest-api]",
            "profiles: [layered, rest-api, package-cycles]",
        );
    fs::write(root.join(".dornguard.yml"), config).unwrap();
    let cycles = |result: &serde_json::Value| {
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item["rule_id"] == "architecture.package-cycle")
            .count()
    };

    let recorded = std::process::Command::new(env!("CARGO_BIN_EXE_dornguard"))
        .current_dir(root)
        .args(["baseline", "create"])
        .output()
        .unwrap();
    assert!(recorded.status.success(), "{recorded:?}");

    // Pushing Alpha's dependency down a line makes AlphaToo the group's first location.
    write(
        "alpha/Alpha.java",
        class("alpha", "Alpha", "example.admin.beta.Beta", "\n"),
    );
    let result = report(
        check(root)
            .args(["--baseline", ".dornguard-baseline.json"])
            .output()
            .unwrap(),
        0,
    );
    assert_eq!(cycles(&result), 0, "{result}");
}
