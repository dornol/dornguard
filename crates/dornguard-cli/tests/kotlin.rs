#![allow(clippy::unwrap_used)]
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::TempDir;

mod common;
use common::{check, commit_fixture, copy_gradle_fixture, git, report, write_custom_ruleset};

const KOTLIN_SERVICE: &str = "service/src/custom/kotlin/relocated/LabelsService.kt";
const KOTLIN_ARCH_VIOLATION: &str = "package example.service.domain\n\nclass LabelsService {\n    fun format(value: String): String {\n        return example.service.adapter.LabelAdapter().map(value)\n    }\n}\n";

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
    // A command-line entry point prints by design; other functions do not.
    let source = "package example\n\nfun main(args: Array<String>) {\n    println(args.size)\n}\n\nfun display() {\n    println(42)\n}\n";
    fs::write(root.join(file), source).unwrap();
    let flagged = report(check(root).output().unwrap(), 1);
    let finding = &flagged["diagnostics"][0];
    assert_eq!(finding["rule_id"], "kotlin.stdout-usage");
    assert_eq!(finding["path"], file);
    assert_eq!(finding["line"], 8);
    assert_eq!(flagged["diagnostics"].as_array().unwrap().len(), 1);

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
#[ignore = "requires Gradle, JDK 21 and the pinned Semgrep"]
fn kotlin_reports_printed_stack_traces_and_transactional_suspend_functions() {
    let project = kotlin_fixture("kotlin-single");
    let root = project.path();
    let file = "src/main/kotlin/example/Example.kt";
    fs::write(
        root.join(file),
        r#"package example

import org.springframework.transaction.annotation.Transactional

class Service {
    @Transactional
    suspend fun saveBlock(id: Long) { store(id) }

    @Transactional(readOnly = true)
    suspend fun loadExpression(id: Long): String = read(id)

    @Transactional
    fun saveBlocking(id: Long) { store(id) }

    suspend fun plain(id: Long) { store(id) }

    fun report(error: Exception) {
        error.printStackTrace()
        logger.error("failed", error)
    }
}
"#,
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 1);
    assert_eq!(result["complete"], true, "{result}");
    let mut findings = result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| {
            (
                finding["line"].as_u64().unwrap(),
                finding["rule_id"].as_str().unwrap(),
                finding["path"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    findings.sort_unstable();
    assert_eq!(
        findings,
        [
            // The fixture does not apply the Kotlin Spring plugin.
            (6, "kotlin.missing-spring-plugin", file),
            (6, "kotlin.transactional-suspend", file),
            (9, "kotlin.transactional-suspend", file),
            (18, "kotlin.print-stack-trace", file),
        ],
        "{result}"
    );
}

#[test]
#[ignore = "requires Gradle, JDK 21 and the pinned Semgrep"]
fn kotlin_reports_field_injection_private_transactions_and_data_class_entities() {
    let project = kotlin_fixture("kotlin-single");
    let root = project.path();
    let file = "src/main/kotlin/example/Example.kt";
    fs::write(
        root.join(file),
        r#"package example

import jakarta.inject.Inject
import jakarta.persistence.Entity
import jakarta.persistence.Id
import org.springframework.beans.factory.annotation.Autowired
import org.springframework.beans.factory.annotation.Value
import org.springframework.transaction.annotation.Transactional

@Entity
data class Book(@Id val id: Long, val title: String)

@Entity(name = "authors")
data class Author(@Id val id: Long) {
    fun label() = "author $id"
}

@Entity
class Shelf(@Id val id: Long)

data class BookView(val id: Long)

class Catalog(private val shelves: Any) {
    @Autowired
    lateinit var books: Any

    @Autowired(required = false)
    private lateinit var metrics: Any

    @Inject
    lateinit var clock: Any

    @Autowired
    var audit: Any? = null

    @Value("\${app.name}")
    lateinit var name: String

    @Transactional
    private fun save() {}

    @Transactional(readOnly = true)
    private fun load(): Any = shelves

    @Transactional
    fun publish() {}
}

class Library @Autowired constructor(private val catalog: Catalog)
"#,
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 1);
    assert_eq!(result["complete"], true, "{result}");
    let mut findings = result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| {
            (
                finding["line"].as_u64().unwrap(),
                finding["rule_id"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    findings.sort_unstable();
    assert_eq!(
        findings,
        [
            (10, "kotlin.jpa-data-class-entity"),
            // The fixture applies neither the Kotlin JPA nor the Spring plugin.
            (10, "kotlin.missing-jpa-plugin"),
            (13, "kotlin.jpa-data-class-entity"),
            (24, "kotlin.field-injection"),
            (27, "kotlin.field-injection"),
            (30, "kotlin.field-injection"),
            (33, "kotlin.field-injection"),
            (39, "kotlin.missing-spring-plugin"),
            (39, "kotlin.private-transactional"),
            (42, "kotlin.private-transactional"),
        ],
        "{result}"
    );
}

#[test]
#[ignore = "requires Gradle, JDK 21 and the pinned Semgrep"]
fn kotlin_reports_week_based_years_and_days_of_the_year_in_date_patterns() {
    let project = kotlin_fixture("kotlin-single");
    let root = project.path();
    let file = "src/main/kotlin/example/Example.kt";
    fs::write(
        root.join(file),
        r#"package example

import java.text.SimpleDateFormat
import java.time.format.DateTimeFormatter
import java.util.Locale

val weekYear = DateTimeFormatter.ofPattern("YYYY-MM-dd")
val dayOfYear = DateTimeFormatter.ofPattern("yyyy-MM-DD", Locale.ROOT)
val legacy = SimpleDateFormat("dd/MM/YYYY")
val iso = DateTimeFormatter.ofPattern("yyyy-MM-dd'T'HH:mm:ss")
val isoWeek = DateTimeFormatter.ofPattern("YYYY-'W'ww")
val ordinal = DateTimeFormatter.ofPattern("yyyy-DDD")
"#,
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 1);
    assert_eq!(result["complete"], true, "{result}");
    let findings = result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| {
            (
                finding["line"].as_u64().unwrap(),
                finding["rule_id"].as_str().unwrap(),
            )
        })
        .collect::<Vec<_>>();
    assert_eq!(
        findings,
        [
            (7, "kotlin.date-pattern-typo"),
            (8, "kotlin.date-pattern-typo"),
            (9, "kotlin.date-pattern-typo"),
        ],
        "{result}"
    );
}

#[test]
#[ignore = "requires Gradle, JDK 21 and the pinned Semgrep"]
fn kotlin_reports_sql_built_from_templates_and_concatenation() {
    let project = kotlin_fixture("kotlin-single");
    let root = project.path();
    let file = "src/main/kotlin/example/Example.kt";
    fs::write(
        root.join(file),
        r#"package example

class Repository(val jdbc: JdbcTemplate, val signature: Signature) {
    fun load(id: String, table: String) {
        jdbc.query("select * from t where id = '$id'", mapper)
        jdbc.queryForList("select * from " + table)
        jdbc.execute("drop table %s".format(table))
        jdbc.query("select * from t where id = ?", mapper, id)
        signature.update("id=$id".toByteArray())
    }
}
"#,
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 1);
    assert_eq!(result["complete"], true, "{result}");
    let mut lines = result["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|finding| finding["rule_id"] == "kotlin.sql-injection")
        .map(|finding| finding["line"].as_u64().unwrap())
        .collect::<Vec<_>>();
    lines.sort_unstable();
    assert_eq!(lines, [5, 6, 7], "{result}");
}

#[test]
#[ignore = "requires Gradle, JDK 21 and the pinned Semgrep"]
fn kotlin_spring_and_jpa_plugins_open_classes_and_add_constructors() {
    let project = kotlin_fixture("kotlin-single");
    let root = project.path();
    let build = fs::read_to_string(root.join("build.gradle.kts")).unwrap();
    fs::write(
        root.join("build.gradle.kts"),
        build.replace(
            "    kotlin(\"jvm\") version \"2.4.21\"\n",
            "    kotlin(\"jvm\") version \"2.4.21\"\n    kotlin(\"plugin.spring\") version \"2.4.21\"\n    kotlin(\"plugin.jpa\") version \"2.4.21\"\n",
        ),
    )
    .unwrap();
    fs::write(
        root.join("src/main/kotlin/example/Example.kt"),
        r#"package example

import jakarta.persistence.Entity
import jakarta.persistence.Id
import org.springframework.context.annotation.Configuration
import org.springframework.transaction.annotation.Transactional

@Configuration
class AppConfig

@Transactional
class Catalog

@Entity
class Shelf(@Id val id: Long)
"#,
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 0);
    assert_eq!(result["complete"], true, "{result}");
    assert!(
        result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .all(|finding| !finding["rule_id"]
                .as_str()
                .unwrap()
                .starts_with("kotlin.missing-")),
        "{result}"
    );
}

#[test]
#[ignore = "requires Gradle, JDK 21 and the pinned Semgrep"]
fn kotlin_reports_unmanaged_scopes_and_opt_in_not_null_assertions() {
    let project = kotlin_fixture("kotlin-single");
    let root = project.path();
    let file = "src/main/kotlin/example/Example.kt";
    fs::write(
        root.join(file),
        r#"package example

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel

class Poller {
    private val scope = CoroutineScope(Dispatchers.Default)

    fun label(name: String?): Int = name!!.length
}

class Closing {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    fun close() {
        scope.cancel()
    }
}

class Shorthand {
    private val scope = CoroutineScope(Dispatchers.Default)

    fun close() = scope.cancel()

    fun text(): String = "a!!b"
}
"#,
    )
    .unwrap();
    let findings = |profile: &str| {
        let result = report(
            check(root).args(["--profile", profile]).output().unwrap(),
            0,
        );
        assert_eq!(result["complete"], true, "{result}");
        let mut found = result["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .map(|finding| {
                (
                    finding["line"].as_u64().unwrap(),
                    finding["rule_id"].as_str().unwrap().to_owned(),
                )
            })
            .collect::<Vec<_>>();
        found.sort_unstable();
        found
    };
    assert_eq!(
        findings("kotlin-review"),
        [
            (9, "kotlin.unmanaged-coroutine-scope".to_owned()),
            (11, "kotlin.not-null-assertion".to_owned()),
        ]
    );
}
