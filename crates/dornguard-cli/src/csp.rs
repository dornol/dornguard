use std::collections::BTreeMap;
use std::io::{self, Write};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use scraper::{ElementRef, Html, Selector};
use serde::Serialize;
use sha2::{Digest, Sha256, Sha384, Sha512};
use tokio::process::Command;

use crate::TextOrJson;

const BODY_LIMIT: u64 = 4_194_304;

type Policy<'a> = BTreeMap<String, Vec<&'a str>>;

#[derive(Serialize)]
struct Report {
    version: u8,
    status: &'static str,
    enforcing_policies: usize,
    report_only_policies: usize,
    inline_scripts_checked: usize,
    findings: Vec<Finding>,
}

#[derive(Serialize)]
struct Finding {
    rule_id: &'static str,
    message: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    inline_script: Option<usize>,
}

pub async fn check(url: &str, format: TextOrJson) -> Result<u8> {
    if !(url.starts_with("https://") || url.starts_with("http://"))
        || url.chars().any(char::is_control)
    {
        bail!("Supply an HTTP or HTTPS page URL");
    }
    let temp = tempfile::tempdir().into_diagnostic()?;
    let headers = temp.path().join("headers");
    let body = temp.path().join("body");
    let mut command = Command::new("curl");
    command
        .kill_on_drop(true)
        .args([
            "--disable",
            "--silent",
            "--show-error",
            "--fail",
            "--globoff",
            "--compressed",
            "--proto",
            "=http,https",
            "--connect-timeout",
            "5",
            "--max-time",
            "20",
            "--max-filesize",
            "4194304",
            "--write-out",
            "%{http_code}",
            "--dump-header",
        ])
        .arg(&headers)
        .arg("--output")
        .arg(&body)
        .arg("--")
        .arg(url);
    let output = tokio::time::timeout(Duration::from_secs(25), command.output())
        .await
        .into_diagnostic()
        .wrap_err("CSP HTTP request timed out")?
        .into_diagnostic()
        .wrap_err("Could not start curl for the CSP check")?;
    if !output.status.success() {
        bail!(
            "CSP HTTP request failed ({})\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let status = String::from_utf8_lossy(&output.stdout);
    if !status
        .trim()
        .parse::<u16>()
        .is_ok_and(|code| (200..300).contains(&code))
    {
        bail!(
            "CSP check expected a successful HTML response, received HTTP {}",
            status.trim()
        );
    }
    let headers = std::fs::read_to_string(headers)
        .into_diagnostic()
        .wrap_err("Could not read response headers")?;
    let headers = response_headers(&headers);
    if !headers.get("content-type").is_some_and(|types| {
        types.last().is_some_and(|value| {
            value
                .split(';')
                .next()
                .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("text/html"))
        })
    }) {
        bail!("CSP check requires a text/html response");
    }
    if std::fs::metadata(&body).into_diagnostic()?.len() > BODY_LIMIT {
        bail!("CSP check body exceeds 4 MiB");
    }
    let body = std::fs::read_to_string(body)
        .into_diagnostic()
        .wrap_err("Could not read the response as UTF-8 HTML")?;
    let report = inspect(&headers, &body);
    write_report(&report, format, &mut io::stdout().lock())?;
    Ok(u8::from(!report.findings.is_empty()))
}

fn response_headers(raw: &str) -> BTreeMap<String, Vec<&str>> {
    let mut headers = BTreeMap::<String, Vec<_>>::new();
    for line in raw.lines() {
        if line.starts_with("HTTP/") {
            headers.clear();
        } else if let Some((key, value)) = line.split_once(':') {
            headers
                .entry(key.to_ascii_lowercase())
                .or_default()
                .push(value.trim());
        }
    }
    headers
}

fn policies<'a>(headers: &BTreeMap<String, Vec<&'a str>>, name: &str) -> Vec<Policy<'a>> {
    headers
        .get(name)
        .into_iter()
        .flatten()
        .flat_map(|value| value.split(','))
        .filter(|policy| !policy.trim().is_empty())
        .map(|policy| {
            let mut directives = BTreeMap::new();
            for directive in policy.split(';') {
                let mut words = directive.split_ascii_whitespace();
                if let Some(name) = words.next() {
                    directives
                        .entry(name.to_ascii_lowercase())
                        .or_insert_with(|| words.collect());
                }
            }
            directives
        })
        .collect()
}

fn script_sources<'a>(policy: &'a Policy<'a>, context: &str) -> Option<&'a [&'a str]> {
    policy
        .get(context)
        .or_else(|| policy.get("script-src"))
        .or_else(|| policy.get("default-src"))
        .map(Vec::as_slice)
}

fn keyword(sources: &[&str], keyword: &str) -> bool {
    sources
        .iter()
        .any(|value| value.eq_ignore_ascii_case(keyword))
}

fn qualified_source(source: &str) -> Option<(&str, &str)> {
    let (kind, value) = source
        .strip_prefix('\'')?
        .strip_suffix('\'')?
        .split_once('-')?;
    let base = value
        .strip_suffix("==")
        .or_else(|| value.strip_suffix('='))
        .unwrap_or(value);
    (!base.is_empty()
        && base
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'-' | b'_')))
    .then_some((kind, value))
}

fn inline_allowed(sources: &[&str]) -> bool {
    keyword(sources, "'unsafe-inline'")
        && !keyword(sources, "'strict-dynamic'")
        && !sources
            .iter()
            .filter_map(|source| qualified_source(source))
            .any(|(kind, value)| {
                !value.is_empty()
                    && matches!(
                        kind.to_ascii_lowercase().as_str(),
                        "nonce" | "sha256" | "sha384" | "sha512"
                    )
            })
}

fn permits_script(sources: &[&str], nonce: Option<&str>, text: &str) -> bool {
    inline_allowed(sources)
        || sources
            .iter()
            .filter_map(|source| qualified_source(source))
            .any(|(kind, value)| {
                let kind = kind.to_ascii_lowercase();
                if kind == "nonce" {
                    return !value.is_empty() && nonce == Some(value);
                }
                let digest = match kind.as_str() {
                    "sha256" => STANDARD.encode(Sha256::digest(text.as_bytes())),
                    "sha384" => STANDARD.encode(Sha384::digest(text.as_bytes())),
                    "sha512" => STANDARD.encode(Sha512::digest(text.as_bytes())),
                    _ => return false,
                };
                let normalized = value.replace('-', "+").replace('_', "/");
                digest.trim_end_matches('=') == normalized.trim_end_matches('=')
            })
}

fn executable(script: &ElementRef<'_>) -> bool {
    let element = script.value();
    if element.attr("src").is_some()
        || script
            .ancestors()
            .filter_map(ElementRef::wrap)
            .any(|ancestor| matches!(ancestor.value().name(), "template" | "noscript"))
    {
        return false;
    }
    matches!(
        element
            .attr("type")
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "" | "module"
            | "application/javascript"
            | "application/ecmascript"
            | "application/x-javascript"
            | "application/x-ecmascript"
            | "text/javascript"
            | "text/ecmascript"
            | "text/x-javascript"
            | "text/x-ecmascript"
            | "text/javascript1.0"
            | "text/javascript1.1"
            | "text/javascript1.2"
            | "text/javascript1.3"
            | "text/javascript1.4"
            | "text/javascript1.5"
            | "text/jscript"
            | "text/livescript"
    )
}

fn inspect(headers: &BTreeMap<String, Vec<&str>>, body: &str) -> Report {
    let enforced = policies(headers, "content-security-policy");
    let report_only = policies(headers, "content-security-policy-report-only");
    let mut findings = Vec::new();
    let mut add = |rule_id, message| {
        findings.push(Finding {
            rule_id,
            message,
            inline_script: None,
        })
    };
    if enforced.is_empty() {
        add(
            "csp.missing-enforced-policy",
            "The response has no enforced Content-Security-Policy header.",
        );
    } else {
        if ["script-src-elem", "script-src-attr", "script-src"]
            .iter()
            .any(|context| {
                enforced
                    .iter()
                    .all(|policy| script_sources(policy, context).is_none())
            })
        {
            add(
                "csp.missing-script-restriction",
                "The enforced policies leave a script context unrestricted.",
            );
        }
        if ["script-src-elem", "script-src-attr"]
            .iter()
            .any(|context| {
                let sources = || {
                    enforced
                        .iter()
                        .filter_map(|policy| script_sources(policy, context))
                };
                sources().any(inline_allowed) && sources().all(inline_allowed)
            })
        {
            add(
                "csp.unsafe-inline",
                "The effective policy allows unrestricted inline JavaScript in at least one context.",
            );
        }
        let eval_sources = || {
            enforced
                .iter()
                .filter_map(|policy| script_sources(policy, "script-src"))
        };
        if eval_sources().any(|values| keyword(values, "'unsafe-eval'"))
            && eval_sources().all(|values| keyword(values, "'unsafe-eval'"))
        {
            add(
                "csp.unsafe-eval",
                "The effective policy permits JavaScript evaluation from strings.",
            );
        }
    }

    let document = Html::parse_document(body);
    let selector = Selector::parse("script").expect("static CSS selector");
    let mut checked = 0;
    for script in document.select(&selector).filter(executable) {
        let text: String = script.text().collect();
        if text.trim().is_empty() {
            continue;
        }
        checked += 1;
        let nonce = script.value().attr("nonce");
        if enforced.iter().any(|policy| {
            script_sources(policy, "script-src-elem")
                .is_some_and(|sources| !permits_script(sources, nonce, &text))
        }) {
            findings.push(Finding {
                rule_id: "csp.inline-script-blocked",
                message: "An enforced policy does not authorize this inline script.",
                inline_script: Some(checked),
            });
        }
    }
    Report {
        version: 1,
        status: if findings.is_empty() {
            "passed"
        } else {
            "findings"
        },
        enforcing_policies: enforced.len(),
        report_only_policies: report_only.len(),
        inline_scripts_checked: checked,
        findings,
    }
}

fn write_report(report: &Report, format: TextOrJson, output: &mut impl Write) -> Result<()> {
    match format {
        TextOrJson::Json => {
            serde_json::to_writer_pretty(&mut *output, report).into_diagnostic()?;
            writeln!(output).into_diagnostic()?;
        }
        TextOrJson::Text => {
            writeln!(
                output,
                "DornGuard CSP: {} ({} enforced, {} report-only, {} inline scripts)",
                report.status,
                report.enforcing_policies,
                report.report_only_policies,
                report.inline_scripts_checked
            )
            .into_diagnostic()?;
            for finding in &report.findings {
                if let Some(script) = finding.inline_script {
                    writeln!(
                        output,
                        "{} (inline script #{script}): {}",
                        finding.rule_id, finding.message
                    )
                    .into_diagnostic()?;
                } else {
                    writeln!(output, "{}: {}", finding.rule_id, finding.message)
                        .into_diagnostic()?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_only_is_distinct_from_enforcement_and_proxy_headers_are_discarded() {
        let raw = "HTTP/1.1 200 Connection established\r\nContent-Security-Policy: script-src 'none'\r\n\r\nHTTP/2 200\r\nContent-Security-Policy-Report-Only: script-src 'none'\r\n";
        let report = inspect(&response_headers(raw), "<script>run()</script>");
        assert_eq!(report.enforcing_policies, 0);
        assert_eq!(report.report_only_policies, 1);
        assert_eq!(report.inline_scripts_checked, 1);
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].rule_id, "csp.missing-enforced-policy");
    }

    #[test]
    fn nonce_and_hash_authorize_executable_inline_scripts() {
        let script = "console.log('ready')";
        let hash = STANDARD.encode(Sha256::digest(script.as_bytes()));
        let raw = format!(
            "Content-Security-Policy: default-src 'none'; script-src-elem 'nonce-YWJj' 'sha256-{hash}'\r\n"
        );
        let html = format!(
            "<script nonce='YWJj' type='module'>run()</script><script>{script}</script>\
             <script type='application/ld+json'>{{}}</script>\
             <script src='external.js'></script><template><script>inert()</script></template>"
        );
        let report = inspect(&response_headers(&raw), &html);
        assert_eq!(report.inline_scripts_checked, 2);
        assert!(report.findings.is_empty());
    }

    #[test]
    fn every_enforced_policy_must_allow_a_script() {
        let raw = "Content-Security-Policy: script-src 'unsafe-inline' 'unsafe-eval'\r\nContent-Security-Policy: default-src 'none'; script-src-elem 'nonce-YWJj' 'unsafe-inline'\r\n";
        let report = inspect(
            &response_headers(raw),
            "<script nonce='bWlzbWF0Y2g='>run()</script>",
        );
        assert_eq!(report.enforcing_policies, 2);
        assert_eq!(report.findings.len(), 1);
        assert_eq!(report.findings[0].rule_id, "csp.inline-script-blocked");
        assert_eq!(report.findings[0].inline_script, Some(1));
    }

    #[test]
    fn unrestricted_inline_and_string_evaluation_are_findings() {
        let raw = "Content-Security-Policy: script-src 'unsafe-inline' 'unsafe-eval' 'sha256-invalid!'\r\n";
        let report = inspect(&response_headers(raw), "<script>run()</script>");
        assert_eq!(
            report
                .findings
                .iter()
                .map(|item| item.rule_id)
                .collect::<Vec<_>>(),
            ["csp.unsafe-inline", "csp.unsafe-eval"],
        );
    }
}
