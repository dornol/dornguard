use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::{self, Write};

use dornguard_core::{CheckReport, Severity};
use serde_json::{Value, json};

pub fn write_sarif(report: &CheckReport, writer: &mut impl Write) -> io::Result<()> {
    let mut seen = BTreeSet::new();
    let rules = report
        .diagnostics
        .iter()
        .filter(|item| seen.insert(&item.rule_id))
        .map(|item| {
            json!({
                    "id": item.rule_id,
                    "help": {"text": item.fix},
                    "defaultConfiguration": {"level": level(item.severity)}
            })
        })
        .collect::<Vec<_>>();

    let results = report
        .diagnostics
        .iter()
        .map(|item| {
            let mut result = json!({
                "ruleId": item.rule_id,
                "level": level(item.severity),
                "message": {"text": item.message}
            });
            if let Some(path) = &item.path {
                let mut physical = json!({
                    "artifactLocation": {"uri": relative_uri(path), "uriBaseId": "%SRCROOT%"}
                });
                if let Some(line) = item.line {
                    physical["region"] = json!({"startLine": line});
                    if let Some(column) = item.column {
                        physical["region"]["startColumn"] = json!(column);
                    }
                    if let Some(line) = item.end_line {
                        physical["region"]["endLine"] = json!(line);
                    }
                    if let Some(column) = item.end_column {
                        physical["region"]["endColumn"] = json!(column);
                    }
                }
                result["locations"] = json!([{"physicalLocation": physical}]);
            } else if let Some(class_name) = &item.class_name {
                result["locations"] = json!([{"logicalLocations": [
                    {"fullyQualifiedName": class_name, "kind": "type"}
                ]}]);
                if let Some(module) = &item.module {
                    result["properties"] = json!({"module": module});
                }
            }
            if let Some(fingerprint) = &item.fingerprint {
                result["partialFingerprints"] = json!({"dornguardFingerprint/v1": fingerprint});
            }
            if let Some(subject) = &item.subject {
                result["properties"]["subject"] = json!(subject);
            }
            result
        })
        .collect::<Vec<_>>();

    let notifications = report
        .failures
        .iter()
        .map(|failure| ("error", failure))
        .chain(
            report
                .analysis_warnings
                .iter()
                .map(|warning| ("warning", warning)),
        )
        .map(|(level, failure)| {
            let mut item = json!({"level": level, "message": {"text": failure.message}});
            if let Some(path) = &failure.path {
                item["locations"] = json!([{"physicalLocation": {"artifactLocation": {
                    "uri": relative_uri(path), "uriBaseId": "%SRCROOT%"
                }}}]);
            }
            if let Some(kind) = &failure.error_type {
                item["descriptor"] = json!({"id": kind});
            }
            item
        })
        .collect::<Vec<_>>();
    let mut properties = json!({
        "scope": report.scope,
        "filesRequested": report.files_requested,
        "filesScanned": report.files_scanned,
        "suppressedCount": report.suppressed_count,
        "organizationPolicyEnforced": report.organization_policy_enforced
    });
    if let Some(filter) = &report.report_filter {
        properties["reportFilter"] = json!(filter);
    }
    if let Some(scope) = report.project_scope {
        properties["projectScope"] = json!(scope);
    }
    if let Some(scope) = report.organization_scope {
        properties["organizationScope"] = json!(scope);
    }
    if let Some(reason) = &report.fallback_reason {
        properties["fallbackReason"] = Value::String(reason.clone());
    }
    let sarif = json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "columnKind": "utf16CodeUnits",
            "tool": {"driver": {"name": "DornGuard", "version": env!("CARGO_PKG_VERSION"), "rules": rules}},
            "invocations": [{
                "executionSuccessful": report.complete,
                "exitCode": report.exit_code,
                "toolExecutionNotifications": notifications
            }],
            "results": results,
            "properties": properties
        }]
    });
    serde_json::to_writer(&mut *writer, &sarif)?;
    writeln!(writer)
}

fn level(severity: Severity) -> &'static str {
    match severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

fn relative_uri(path: &str) -> String {
    let mut uri = String::new();
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            uri.push(char::from(byte));
        } else {
            write!(uri, "%{byte:02X}").expect("writing to a String is infallible");
        }
    }
    uri
}
