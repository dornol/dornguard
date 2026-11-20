#![allow(clippy::unwrap_used)]
use serde_json::Value;
use std::fs;
use std::process::Command;

mod common;
use common::{check, project, report, unnecessary_import_lines};

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
fn print_stack_trace_is_reported_in_java_source() {
    let project = project(
        "class Example {\n    void run(Exception e) {\n        e.printStackTrace();\n        log.error(\"failed\", e);\n    }\n}\n",
    );
    let result = report(check(project.path()).output().unwrap(), 1);
    assert_eq!(result["complete"], true, "{result}");
    let findings = result["diagnostics"].as_array().unwrap();
    assert_eq!(findings.len(), 1, "{result}");
    assert_eq!(findings[0]["rule_id"], "java.print-stack-trace");
    assert_eq!(findings[0]["line"], 3);
}
