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
