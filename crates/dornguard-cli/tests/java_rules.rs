#![allow(clippy::unwrap_used)]
use serde_json::Value;
use std::fs;
use std::process::Command;

mod common;
use common::{CLEAN, check, commit_fixture, project, report, unnecessary_import_lines};

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
#[ignore = "requires the pinned PMD and Semgrep engines"]
fn java_review_finds_discarded_results_and_nested_junit_calls() {
    let project = project(
        r#"import static org.junit.jupiter.api.Assertions.assertThrows;
import org.junit.jupiter.api.Assertions;
import java.math.BigDecimal;
class Example {
    void test(String name, BigDecimal amount, BigDecimal fee, Mapper mapper, Request request) {
        name.trim();
        amount.add(fee);
        mapper.updateStatus(1);
        assertThrows(Exception.class, () -> approve(makeRequest()));
        Assertions.assertThrows(Exception.class, () -> mapper.approve(mapper.build()));
        org.junit.jupiter.api.Assertions.assertThrowsExactly(Exception.class, () -> new Request(makeRequest()));
        assertThrows(Exception.class, () -> { approve(makeRequest()); });
        assertThrows(Exception.class, () -> approve(request));
        Other.assertThrows(Exception.class, () -> approve(makeRequest()));
        String normalized = name.trim();
        BigDecimal charged = amount.add(fee);
    }
}
class Mapper {
    private int status;
    int updateStatus(int id) { status = id; return status; }
    Request build() { return new Request(); }
    void approve(Request request) {}
}
class LocalAssertions {
    void assertThrows(Class<?> exception, Runnable test) { test.run(); }
    void test() { assertThrows(Exception.class, () -> handle(prepare())); }
    void handle(Request request) {}
    Request prepare() { return new Request(); }
}
"#,
    );
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [java-review]\n",
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 0);
    let findings = result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                item["rule_id"].as_str().unwrap(),
                item["line"].as_u64().unwrap(),
                item["severity"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        findings,
        [
            ("java.ignored-return-value", 6, "warning"),
            ("java.ignored-return-value", 7, "warning"),
            ("java.assert-throws-nested-call", 9, "warning"),
            ("java.assert-throws-nested-call", 10, "warning"),
            ("java.assert-throws-nested-call", 11, "warning"),
            ("java.assert-throws-nested-call", 12, "warning"),
        ],
        "{result}"
    );
}

#[test]
#[ignore = "requires the pinned Semgrep engine"]
fn thymeleaf_partial_warning_preserves_incomplete_status_and_other_failures() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    let path = "src/main/resources/templates/broken.html";
    fs::create_dir_all(root.join(path).parent().unwrap()).unwrap();
    fs::write(root.join(path), r#"<html><body><th:block th:if="${a}"><br"#).unwrap();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [thymeleaf]\nthymeleaf_partial_parsing: warning\n",
    )
    .unwrap();
    let warning = report(check(root).output().unwrap(), 0);
    assert_eq!(warning["status"], "incomplete");
    assert_eq!(warning["complete"], false);
    assert_eq!(warning["files_scanned"], 0);
    assert_eq!(warning["analysis_warnings"][0]["path"], path);
    assert_eq!(
        warning["analysis_warnings"][0]["error_type"],
        "PartialParsing"
    );
    let baseline = Command::new(env!("CARGO_BIN_EXE_dornguard"))
        .current_dir(root)
        .args(["baseline", "create"])
        .output()
        .unwrap();
    assert_eq!(baseline.status.code(), Some(3));
    assert!(!root.join(".dornguard-baseline.json").is_file());

    let html = report(check(root).args(["--profile", "html"]).output().unwrap(), 3);
    assert!(
        html["failures"][0]["rule_ids"]
            .as_array()
            .unwrap()
            .iter()
            .any(|rule| rule.as_str().unwrap().starts_with("html.")),
        "{html}"
    );
    assert_eq!(html["analysis_warnings"][0]["path"], path);

    let authority = tempfile::tempdir().unwrap();
    let policy = authority.path().join("policy.yml");
    fs::write(&policy, "version: 1\nprofiles: [thymeleaf]\n").unwrap();
    let required = report(
        check(root)
            .arg("--organization-policy")
            .arg(policy)
            .output()
            .unwrap(),
        3,
    );
    assert_eq!(required["failures"][0]["path"], path);

    fs::write(
        root.join("src/main/resources/templates/violation.html"),
        r#"<html><body><a th:href="${@example.value()}">Link</a></body></html>"#,
    )
    .unwrap();
    let failing = report(check(root).output().unwrap(), 1);
    assert_eq!(failing["complete"], false);
    assert_eq!(failing["analysis_warnings"][0]["path"], path);
    assert_eq!(
        failing["diagnostics"][0]["rule_id"],
        "thymeleaf.restricted-bean-access"
    );
}
