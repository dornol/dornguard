//! `cargo clippy` with the project's own lint configuration, run once per Cargo workspace.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

use dornguard_config::{CLIPPY_RULE, RUSTC_LINT_RULE, Rule};
use dornguard_core::{Diagnostic, EngineName, Outcome, Severity, fingerprint};
use miette::{Result, bail, miette};
use serde::Deserialize;
use tokio::time::Instant;

use crate::source::Sources;
use crate::{
    Engine, Request, check_version, command, display_path, engine_failure, fail, nearest_manifest,
    relative_path, run_command,
};

const ENGINE: EngineName = EngineName::Clippy;

pub struct Clippy;

impl Engine for Clippy {
    async fn check(&self, request: Request<'_>) -> Outcome {
        let mut outcome = Outcome::default();
        if let Err(error) = scan(request, &mut outcome).await {
            fail(&mut outcome, ENGINE, format!("{error:#}"));
        }
        request.mark_incomplete(&mut outcome);
        outcome
    }
}

async fn scan(request: Request<'_>, outcome: &mut Outcome) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(600);
    check_version(ENGINE, request.root, deadline).await?;
    let rules = request
        .rules
        .iter()
        .map(|rule| (rule.id.as_str(), *rule))
        .collect::<BTreeMap<_, _>>();
    let mut packages = BTreeMap::<PathBuf, Vec<&Path>>::new();
    for file in request.files {
        let package = nearest_manifest(request.root, file, "Cargo.toml")
            .ok_or_else(|| miette!("No Cargo.toml found for {}", display_path(file)))?;
        packages.entry(package).or_default().push(file);
    }
    let mut workspaces = BTreeMap::<PathBuf, BTreeSet<&Path>>::new();
    for (package, files) in packages {
        let workspace = workspace_root(&request.root.join(&package), deadline).await?;
        workspaces.entry(workspace).or_default().extend(files);
    }
    let mut sources = Sources::default();
    for (workspace, files) in workspaces {
        let mut clippy = command(ENGINE, &workspace)?;
        clippy.args([
            "clippy",
            "--workspace",
            "--all-targets",
            "--message-format=json",
            "--quiet",
        ]);
        let output = run_command(ENGINE, &mut clippy, deadline).await?;
        let mut run = Run {
            root: request.root,
            workspace: &workspace,
            rules: &rules,
            requested: &files,
            sources: &mut sources,
            seen: BTreeSet::new(),
            build_errors: Vec::new(),
            denied: false,
            outcome,
        };
        for line in output.stdout.split(|byte| *byte == b'\n') {
            if let Ok(Event::CompilerMessage { message }) = serde_json::from_slice::<Event>(line) {
                run.record(message);
            }
        }
        let Run {
            build_errors,
            denied,
            ..
        } = run;
        if !build_errors.is_empty() || (!output.status.success() && !denied) {
            let detail = if build_errors.is_empty() {
                String::from_utf8_lossy(&output.stderr).trim().to_owned()
            } else {
                build_errors.join("\n")
            };
            let mut failure = engine_failure(
                ENGINE,
                format!(
                    "cargo clippy could not build the workspace in {} ({})\n{detail}",
                    relative_path(request.root, &workspace)
                        .map_or_else(|| ".".to_owned(), |path| display_path(&path)),
                    output.status
                ),
            );
            failure.file_count = Some(files.len());
            outcome.failures.push(failure);
        } else {
            outcome
                .scanned_files
                .extend(files.iter().map(|file| file.to_path_buf()));
        }
    }
    Ok(())
}

/// The root directory of the Cargo workspace that contains `package`.
async fn workspace_root(package: &Path, deadline: Instant) -> Result<PathBuf> {
    let mut locate = command(ENGINE, package)?;
    locate.args(["locate-project", "--workspace", "--message-format", "plain"]);
    let output = run_command(ENGINE, &mut locate, deadline).await?;
    if !output.status.success() {
        bail!(
            "cargo could not find the workspace of {} ({})\n{}",
            package.display(),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let manifest = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
    manifest
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| miette!("cargo returned an invalid workspace manifest path"))
}

#[derive(Deserialize)]
#[serde(tag = "reason", rename_all = "kebab-case")]
enum Event {
    CompilerMessage {
        message: Message,
    },
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
struct Message {
    message: String,
    level: String,
    code: Option<Code>,
    #[serde(default)]
    spans: Vec<Span>,
    #[serde(default)]
    children: Vec<Message>,
}

#[derive(Deserialize)]
struct Code {
    code: String,
}

#[derive(Deserialize)]
struct Span {
    file_name: String,
    byte_start: usize,
    byte_end: usize,
    line_start: u32,
    is_primary: bool,
    suggested_replacement: Option<String>,
}

/// The rule that reports a lint, or `None` for a compiler error such as `E0308`.
fn lint_rule(code: &str) -> Option<&'static str> {
    if code.starts_with("clippy::") {
        Some(CLIPPY_RULE)
    } else if code.starts_with('E') && code[1..].bytes().all(|byte| byte.is_ascii_digit()) {
        None
    } else {
        Some(RUSTC_LINT_RULE)
    }
}

struct Run<'a, 'b> {
    root: &'a Path,
    workspace: &'a Path,
    rules: &'a BTreeMap<&'a str, &'a Rule>,
    requested: &'a BTreeSet<&'a Path>,
    sources: &'b mut Sources,
    seen: BTreeSet<(PathBuf, usize, usize, String)>,
    build_errors: Vec<String>,
    /// A lint at `deny` level made Cargo exit with an error.
    denied: bool,
    outcome: &'b mut Outcome,
}

impl Run<'_, '_> {
    fn record(&mut self, message: Message) {
        if !matches!(message.level.as_str(), "warning" | "error") {
            return;
        }
        let primary = message.spans.iter().find(|span| span.is_primary);
        let lint = message.code.as_ref().map(|code| code.code.as_str());
        let Some((lint, rule_id)) = lint.and_then(|lint| Some((lint, lint_rule(lint)?))) else {
            if message.level == "error" {
                let location = primary.map_or_else(String::new, |span| {
                    format!("{}:{}: ", span.file_name, span.line_start)
                });
                let error = format!("{location}{}", message.message);
                // Each target that includes the file reports the same error.
                if !self.build_errors.contains(&error) {
                    self.build_errors.push(error);
                }
            }
            return;
        };
        if message.level == "error" {
            self.denied = true;
        }
        let Some(rule) = self.rules.get(rule_id) else {
            return;
        };
        let Some(span) = primary else {
            return;
        };
        let Some(path) = self.project_path(&span.file_name) else {
            return;
        };
        if !self.seen.insert((
            path.clone(),
            span.byte_start,
            span.byte_end,
            lint.to_owned(),
        )) {
            return;
        }
        let root = self.root;
        let source = self.sources.get(root, &path);
        let start = source.and_then(|source| source.position(span.byte_start));
        let end = source
            .filter(|_| span.byte_start < span.byte_end)
            .and_then(|source| source.position(span.byte_end));
        let identity =
            source.and_then(|source| source.range_fingerprint(span.byte_start, span.byte_end));
        let help = message
            .children
            .iter()
            .filter(|child| child.level == "help")
            .find(|child| !child.message.starts_with("for further information"))
            .map(|child| {
                let replacement = child
                    .spans
                    .iter()
                    .filter_map(|span| span.suggested_replacement.as_deref())
                    .find(|text| !text.is_empty() && !text.contains('\n'));
                match replacement {
                    Some(text) => format!("{}: `{text}`", child.message),
                    None => child.message.clone(),
                }
            });
        self.outcome.diagnostics.push(Diagnostic {
            path: Some(display_path(&path)),
            module: None,
            class_name: None,
            line: Some(start.map_or(span.line_start, |(line, _)| line)),
            column: start.map(|(_, column)| column),
            end_line: end.map(|(line, _)| line),
            end_column: end.map(|(_, column)| column),
            subject: Some(lint.to_owned()),
            rule_id: rule.id.clone(),
            engine: ENGINE,
            // Lints the project denies fail the check whatever the rule's default.
            severity: if message.level == "error" {
                Severity::Error
            } else {
                rule.severity
            },
            confidence: rule.confidence,
            message: format!("{} ({lint})", message.message),
            fix: help.map_or_else(|| rule.fix.clone(), |help| capitalized_sentence(&help)),
            edit: None,
            legacy_fingerprints: Vec::new(),
            group: None,
            fingerprint: identity.map(|identity| fingerprint(&format!("{lint}\n{identity}"))),
        });
    }

    fn project_path(&self, file_name: &str) -> Option<PathBuf> {
        let absolute = self.workspace.join(file_name);
        relative_path(self.root, &absolute)
            .or_else(|| relative_path(self.root, &dunce::canonicalize(&absolute).ok()?))
            .filter(|path| self.requested.contains(path.as_path()))
    }
}

fn capitalized_sentence(text: &str) -> String {
    let mut chars = text.trim().chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut sentence = first.to_uppercase().chain(chars).collect::<String>();
    if !sentence.ends_with(['.', '!', '?']) {
        sentence.push('.');
    }
    sentence
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lints_are_split_between_clippy_and_rustc_rules() {
        assert_eq!(lint_rule("clippy::len_zero"), Some(CLIPPY_RULE));
        assert_eq!(lint_rule("unused_variables"), Some(RUSTC_LINT_RULE));
        assert_eq!(lint_rule("E0308"), None);
    }

    #[test]
    fn compiler_messages_are_read_from_cargo_events() {
        let line = br#"{"reason":"compiler-message","message":{"message":"length comparison to zero","level":"warning","code":{"code":"clippy::len_zero","explanation":null},"spans":[{"file_name":"src/main.rs","byte_start":10,"byte_end":20,"line_start":4,"is_primary":true,"suggested_replacement":null}],"children":[]}}"#;
        let Ok(Event::CompilerMessage { message }) = serde_json::from_slice::<Event>(line) else {
            panic!("compiler message expected");
        };
        assert_eq!(message.code.unwrap().code, "clippy::len_zero");
        assert!(matches!(
            serde_json::from_slice::<Event>(br#"{"reason":"build-finished","success":true}"#),
            Ok(Event::Other)
        ));
    }

    #[test]
    fn help_becomes_a_sentence() {
        assert_eq!(
            capitalized_sentence("using `is_empty` is clearer"),
            "Using `is_empty` is clearer."
        );
    }
}
