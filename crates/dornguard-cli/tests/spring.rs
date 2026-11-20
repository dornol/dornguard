#![allow(clippy::unwrap_used)]
use serde_json::Value;
use std::fs;

mod common;
use common::{CLEAN, check, project, report};

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
