#![allow(clippy::unwrap_used)]
use std::fs;
use std::process::Command;

mod common;
use common::{CLEAN, check, commit_fixture, project, report};

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
