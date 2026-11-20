#![allow(clippy::unwrap_used)]
use std::fs;

mod common;
use common::{check, project, report};

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
fn malformed_sources_report_partial_formatting_and_keep_line_diagnostics() {
    let project = project("class Example {");
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [format]\n",
    )
    .unwrap();
    fs::write(root.join("src/example.js"), "function example( {\n").unwrap();
    let result = report(check(root).output().unwrap(), 3);
    assert_eq!(result["complete"], false);
    assert_eq!(result["files_requested"], 2);
    assert_eq!(result["files_scanned"], 0);
    assert_eq!(result["diagnostics"][0]["rule_id"], "format.final-newline");
    let failures = result["failures"].as_array().unwrap();
    assert_eq!(failures.len(), 2, "{result}");
    for failure in failures {
        assert_eq!(failure["error_type"], "PartialParsing");
        let ids = failure["rule_ids"].as_array().unwrap();
        assert!(ids.iter().any(|id| id == "format.indentation"));
        assert!(ids.iter().all(|id| id != "format.final-newline"));
    }
}

#[test]
fn format_edits_are_byte_ranges_tied_to_the_exact_source() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    fs::write(
        root.join(".dornguard.yml"),
        "version: 1\nprofiles: [format]\nformat: {line_ending: crlf}\n",
    )
    .unwrap();
    let file = root.join("fixture.js");
    let source = "const label = `머리  \r\n몸통`;  ";
    fs::write(&file, source).unwrap();
    let report = report(check(root).output().unwrap(), 1);
    let findings = report["diagnostics"].as_array().unwrap();
    assert_eq!(findings.len(), 2, "{report}");
    let mut edits: Vec<_> = findings.iter().map(|finding| &finding["edit"]).collect();
    edits.sort_by_key(|edit| std::cmp::Reverse(edit["start_byte"].as_u64().unwrap()));
    let mut next = source.to_owned();
    for edit in edits {
        assert_eq!(edit["source_sha256"], dornguard_core::fingerprint(source));
        let start = edit["start_byte"].as_u64().unwrap() as usize;
        let end = edit["end_byte"].as_u64().unwrap() as usize;
        next.replace_range(start..end, edit["replacement"].as_str().unwrap());
    }
    assert_eq!(next, "const label = `머리  \r\n몸통`;\r\n");
    assert_ne!(
        dornguard_core::fingerprint(&next),
        dornguard_core::fingerprint(source)
    );
    fs::write(&file, next).unwrap();
    assert!(check(root).output().unwrap().status.success());
}

#[test]
fn formatting_handles_tabs_control_spacing_and_minified_defaults() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    fs::write(root.join("ui.js"), "if(true) {\n\trun();\n}\n").unwrap();
    fs::write(root.join("vendor.min.js"), "if(true){run()}").unwrap();
    let config = "version: 1\nprofiles: [format]\n";
    fs::write(root.join(".dornguard.yml"), config).unwrap();
    let default = report(check(root).output().unwrap(), 1);
    assert_eq!(default["files_requested"], 1);
    assert!(
        default["rule_counts"]["format.indentation"]["errors"]
            .as_u64()
            .unwrap()
            > 0
    );

    fs::write(root.join(".dornguard.yml"), format!("{config}format:\n  overrides:\n    - files: [ui.js]\n      indent_style: tabs\n      control_spacing: none\n")).unwrap();
    let expected = report(check(root).output().unwrap(), 0);
    assert_eq!(expected["files_scanned"], 1);
    fs::write(root.join("ui.js"), "if (true) {\n\trun();\n}\n").unwrap();
    let wrong = report(check(root).output().unwrap(), 1);
    assert_eq!(wrong["diagnostics"][0]["rule_id"], "format.control-spacing");
    assert!(
        wrong["diagnostics"][0]["fix"]
            .as_str()
            .unwrap()
            .contains("directly")
    );

    fs::write(
        root.join(".dornguard.yml"),
        format!("{config}format: {{include_minified: true, exclude: [ui.js]}}\n"),
    )
    .unwrap();
    let included = report(check(root).output().unwrap(), 1);
    assert_eq!(included["files_requested"], 1);
    assert_eq!(included["diagnostics"][0]["path"], "vendor.min.js");
}

#[test]
#[ignore = "requires the pinned Semgrep engine"]
fn formatting_exceptions_keep_other_rules_active() {
    let project = tempfile::tempdir().unwrap();
    let root = project.path();
    for folder in ["ignored", "excluded"] {
        fs::create_dir(root.join(folder)).unwrap();
        fs::write(
            root.join(folder).join("Example.java"),
            "class Example {\n void run() {\n  System.out.println(1);\n }\n}\n",
        )
        .unwrap();
    }
    fs::write(
        root.join(".dornguard.yml"),
        r#"version: 1
profiles: [java-source, format]
format:
  exclude: ["excluded/**"]
ignore:
  - path: ignored/
    rule: "format.*"
    reason: Preserve legacy formatting
"#,
    )
    .unwrap();
    let result = report(check(root).output().unwrap(), 1);
    assert_eq!(result["suppressed_count"], 3);
    let findings = result["diagnostics"].as_array().unwrap();
    assert_eq!(findings.len(), 2);
    for (finding, path) in findings
        .iter()
        .zip(["excluded/Example.java", "ignored/Example.java"])
    {
        assert_eq!(finding["rule_id"], "java.stdout-usage");
        assert_eq!(finding["path"], path);
    }
}
