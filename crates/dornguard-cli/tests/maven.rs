use serde_json::Value;
use std::fs;
use std::process::Command;
use std::sync::PoisonError;

mod common;
use common::{MAVEN_TEST_LOCK, check, commit_fixture, git, report, unnecessary_import_lines};

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
        resolved["pmd_coverage"]["compiled_classpath"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == domain_path),
        "{resolved}"
    );
    assert!(
        resolved["pmd_coverage"]["source_only"]
            .as_array()
            .unwrap()
            .iter()
            .any(|path| path == test_path),
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
    let metadata = "src/main/java/example/package-info.java";
    fs::write(root.join(metadata), "package example;\n").unwrap();
    let exclude_metadata = |value: String| {
        value.replace(
        "<version>3.13.0</version>",
        "<version>3.13.0</version><configuration><excludes><exclude>**/package-info.java</exclude></excludes></configuration>",
    )
    };
    fs::write(
        root.join("pom.xml"),
        exclude_metadata(pom.replace("OPTIONS", "")),
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

    let script = "docs/scripts/Tool.java";
    fs::create_dir_all(root.join(script).parent().unwrap()).unwrap();
    fs::write(
        root.join(script),
        "class Tool { void run() { System.out.println(1); } }\n",
    )
    .unwrap();
    fs::write(
        root.join(".dornguard.yml"),
        include_str!("../../../schemas/examples/maven-layered.yml"),
    )
    .unwrap();
    for args in [&[][..], &["docs/scripts"][..]] {
        let result = report(check(root).args(args).output().unwrap(), 1);
        assert!(
            result["diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|item| { item["path"] == script && item["rule_id"] == "java.stdout-usage" }),
            "{result}"
        );
    }
    let module = report(
        check(root)
            .args(["--module", "example:coverage"])
            .output()
            .unwrap(),
        1,
    );
    assert!(
        module["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .all(|item| item["path"] != script)
    );
    fs::write(root.join(".dornguard.yml"), config).unwrap();

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
