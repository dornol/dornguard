//! `dornguard rules explain`: what an agent needs to act on a rule's findings.

use dornguard_config::{OPEN_IN_VIEW_RULE, Registry, Rule};
use dornguard_core::{Confidence, EngineName, Severity};
use miette::{IntoDiagnostic, Result};
use serde_json::{Value, json};

use crate::cli::TextOrJson;

pub(crate) fn explain(registry: &Registry, ids: &[String], format: TextOrJson) -> Result<()> {
    let rules = ids
        .iter()
        .map(|id| registry.rule(id))
        .collect::<Result<Vec<_>>>()?;
    match format {
        TextOrJson::Json => {
            let entries = rules
                .iter()
                .map(|rule| entry(registry, rule))
                .collect::<Vec<_>>();
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({ "rules": entries })).into_diagnostic()?
            );
        }
        TextOrJson::Text => {
            for (index, rule) in rules.iter().enumerate() {
                if index > 0 {
                    println!();
                }
                print!("{}", text(registry, rule));
            }
        }
    }
    Ok(())
}

fn profiles(registry: &Registry, rule: &Rule) -> Vec<String> {
    registry
        .profiles()
        .filter(|profile| profile.rules.contains(&rule.id))
        .map(|profile| profile.name.clone())
        .collect()
}

fn severity(rule: &Rule) -> &'static str {
    match rule.severity {
        Severity::Error => "error",
        Severity::Warning => "warning",
    }
}

fn confidence(rule: &Rule) -> (&'static str, &'static str) {
    match rule.confidence {
        Confidence::High => ("high", "a finding is a real problem; fix the code"),
        Confidence::Medium => (
            "medium",
            "the rule infers intent from code shape; confirm the finding, then fix it or suppress it with a reason",
        ),
    }
}

/// How to suppress one finding of `rule` where the code is correct.
pub(crate) fn suppression(rule: &Rule) -> String {
    let id = &rule.id;
    let entry = format!(
        "add to .dornguard.yml: ignore: [{{ path: <reported path>, rule: {id}, reason: \"<why the code is correct>\" }}]"
    );
    let located = !matches!(rule.engine, EngineName::Archunit | EngineName::Gradle)
        && !matches!(
            id.as_str(),
            OPEN_IN_VIEW_RULE | "java.jspecify-null-marked" | "flyway.changed-versioned-migration"
        )
        && !id.starts_with("jsp.");
    if id.starts_with("rust.") {
        format!(
            "#[allow(<lint>, reason = \"<why the code is correct>\")] on the item, using the lint named in the finding, or {entry}"
        )
    } else if !located {
        entry
    } else if id.starts_with("html.") || id.starts_with("thymeleaf.") {
        format!(
            "<!-- dornguard-ignore {id} -- <why the code is correct> --> on its own line before the finding, or {entry}"
        )
    } else {
        format!(
            "// dornguard-ignore {id} -- <why the code is correct> on its own line before the finding, or {entry}"
        )
    }
}

fn entry(registry: &Registry, rule: &Rule) -> Value {
    let (confidence, meaning) = confidence(rule);
    let guidance = rule.guidance.as_ref();
    json!({
        "id": rule.id,
        "engine": rule.engine,
        "severity": severity(rule),
        "confidence": confidence,
        "confidence_meaning": meaning,
        "profiles": profiles(registry, rule),
        "description": rule.description,
        "reports": guidance.map(|guidance| &guidance.reports),
        "not_reported": guidance.and_then(|guidance| guidance.not_reported.as_ref()),
        "fix": rule.fix,
        "example": guidance.map(|guidance| json!({ "bad": guidance.bad, "good": guidance.good })),
        "suppress_when": guidance.map(|guidance| &guidance.suppress),
        "suppress_with": suppression(rule),
        "options": guidance.and_then(|guidance| guidance.options.as_ref()),
    })
}

fn indented(code: &str) -> String {
    code.trim_end()
        .lines()
        .map(|line| {
            if line.is_empty() {
                "\n".to_owned()
            } else {
                format!("    {line}\n")
            }
        })
        .collect()
}

fn text(registry: &Registry, rule: &Rule) -> String {
    let (confidence, meaning) = confidence(rule);
    let profiles = profiles(registry, rule);
    let mut out = format!("{}  {}\n", rule.id, rule.description);
    out.push_str(&format!(
        "Severity: {} · Confidence: {confidence} ({meaning})\n",
        severity(rule)
    ));
    if !profiles.is_empty() {
        out.push_str(&format!("Profiles: {}\n", profiles.join(", ")));
    }
    let guidance = rule.guidance.as_ref();
    if let Some(guidance) = guidance {
        out.push_str(&format!("Reports: {}\n", guidance.reports.trim()));
        if let Some(not_reported) = &guidance.not_reported {
            out.push_str(&format!("Not reported: {}\n", not_reported.trim()));
        }
    }
    out.push_str(&format!("Fix: {}\n", rule.fix.trim()));
    if let Some(guidance) = guidance {
        out.push_str("Bad:\n");
        out.push_str(&indented(&guidance.bad));
        out.push_str("Good:\n");
        out.push_str(&indented(&guidance.good));
        out.push_str(&format!("Suppression: {}\n", guidance.suppress.trim()));
    }
    out.push_str(&format!("Suppress with: {}\n", suppression(rule)));
    if let Some(options) = guidance.and_then(|guidance| guidance.options.as_ref()) {
        out.push_str(&format!("Configuration keys: {}\n", options.trim()));
    }
    out
}
