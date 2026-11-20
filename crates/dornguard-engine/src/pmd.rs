use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::source::Sources;
use dornguard_config::{PRESERVE_STACK_TRACE_RULE, RESOURCE_RULE, UNNECESSARY_IMPORT_RULE};
use dornguard_core::{Diagnostic, EngineName, Outcome, PMD_JAVA_DEFAULT, PMD_JAVA_VERSIONS};
use miette::{IntoDiagnostic, Result, WrapErr, bail};
use serde::Deserialize;
use tokio::time::Instant;
use tree_sitter::{Parser, Tree};

use crate::{
    Engine, PMD_VERSION, Request, check_version, command, display_path, engine_failure, fail,
    relative_path, run_command,
};

const ENGINE: EngineName = EngineName::Pmd;
const CLOSE_RESOURCE: &str = "category/java/errorprone.xml/CloseResource";
const PRESERVE_STACK_TRACE: &str = "category/java/bestpractices.xml/PreserveStackTrace";
const NON_CLOSEABLE_EXECUTOR: &str = concat!(
    "self::VariableId[some $class in ('ExecutorService', 'ScheduledExecutorService', ",
    "'AbstractExecutorService', 'ThreadPoolExecutor', 'ScheduledThreadPoolExecutor', 'ForkJoinPool') ",
    "satisfies pmd-java:typeIsExactly(concat('java.util.concurrent.', $class))]"
);
const NON_CLOSEABLE_HTTP_CLIENT: &str =
    "self::VariableId[pmd-java:typeIsExactly('java.net.http.HttpClient')]";
// PMD treats an instanceof binding as a new resource. It misses a direct close
// on that binding at the start of the matching branch.
const CLOSED_PATTERN_RESOURCE: &str = concat!(
    "self::VariableId[@PatternBinding = true()]",
    "[let $binding := @Name return ",
    "parent::TypePattern/parent::PatternExpression/parent::InfixExpression",
    "[parent::IfStatement]/following-sibling::*[1]",
    "/(self::ExpressionStatement | self::Block/*[1][self::ExpressionStatement])",
    "/MethodCall[pmd-java:matchesSig('java.lang.AutoCloseable#close()') ",
    "and *[1]/self::VariableAccess[@Name = $binding]]]"
);
// A shutdown on this exact ExecutorService at the start of the immediately
// following finally block runs on both normal and exceptional exits.
const SHUTDOWN_EXECUTOR: &str = concat!(
    "self::VariableId[pmd-java:typeIs('java.util.concurrent.ExecutorService')]",
    "[let $resource := @Name return parent::VariableDeclarator/parent::LocalVariableDeclaration",
    "[count(VariableDeclarator) = 1]",
    "/following-sibling::*[1]/self::TryStatement[@TryWithResources = false()]",
    "[not(.//VariableAccess[@Name = $resource and @AccessType != 'READ'])]",
    "/FinallyClause/Block/*[1]/self::ExpressionStatement/MethodCall",
    "[pmd-java:matchesSig('java.util.concurrent.ExecutorService#shutdown()')",
    " or pmd-java:matchesSig('java.util.concurrent.ExecutorService#shutdownNow()')]",
    "[*[1]/self::VariableAccess[@Name = $resource]]]"
);
const CAUSE_PRESERVING_EXCEPTION: &str = concat!(
    "self::ConstructorCall[pmd-java:typeIsExactly('java.lang.ClassNotFoundException')]",
    "[@AnonymousClass = false()]",
    "[let $catch := ancestor::CatchClause[1], $caught := $catch/CatchParameter/@Name return ",
    "parent::ThrowStatement and ArgumentList[@Size = 2]/*[2]/self::VariableAccess[@Name = $caught] ",
    "and not($catch/Block//VariableAccess[@Name = $caught and @AccessType != 'READ'])]"
);
// A preceding direct log call in the same catch body runs on every
// path to that throw. Require a resolved throwable overload and the unchanged catch parameter.
const LOGGED_EXCEPTION: &str = concat!(
    "parent::ThrowStatement[parent::Block/parent::CatchClause]",
    "[let $caught := parent::Block/parent::CatchClause/CatchParameter/@Name return ",
    "not(parent::Block//VariableAccess[@Name = $caught and @AccessType != 'READ']) ",
    "and preceding-sibling::ExpressionStatement/MethodCall[",
    "(pmd-java:matchesSig('java.util.logging.Logger#log(java.util.logging.Level,java.lang.String,java.lang.Throwable)') ",
    "or (@MethodName = ('trace', 'debug', 'info', 'warn', 'error') ",
    "and pmd-java:matchesSig(concat('org.slf4j.Logger#', @MethodName, '(java.lang.String,java.lang.Throwable)')))) ",
    "and ArgumentList/*[last()]/self::VariableAccess[@Name = $caught]]]"
);

#[derive(Default)]
pub struct Pmd<'a> {
    pub classpath: Option<&'a [PathBuf]>,
    pub compile_sources: Option<&'a BTreeSet<PathBuf>>,
    pub java_target: Option<u16>,
    pub java_source: Option<u16>,
    pub allow_logged_exceptions: bool,
    pub resource_owners: &'a [String],
    pub managed_resource_factories: &'a [String],
}

impl Engine for Pmd<'_> {
    async fn check(&self, request: Request<'_>) -> Outcome {
        let mut outcome = Outcome::default();
        if let Err(error) = scan(request, self, &mut outcome).await {
            fail(&mut outcome, ENGINE, format!("{error:#}"));
        }
        let coverage = if self.classpath.is_some() {
            &mut outcome.pmd_coverage.compiled_classpath
        } else {
            &mut outcome.pmd_coverage.source_only
        };
        coverage.extend(outcome.scanned_files.iter().map(|path| display_path(path)));
        request.mark_incomplete(&mut outcome);
        outcome
    }
}

async fn scan(request: Request<'_>, settings: &Pmd<'_>, outcome: &mut Outcome) -> Result<()> {
    let version = settings.java_source.unwrap_or(PMD_JAVA_DEFAULT);
    if !PMD_JAVA_VERSIONS.contains(&version) {
        bail!(
            "PMD {PMD_VERSION} cannot parse Java {version}; set java_source to a supported version if the sources are compatible"
        );
    }
    let language = if version < 5 {
        format!("java-1.{version}")
    } else {
        format!("java-{version}")
    };
    let deadline = Instant::now() + Duration::from_secs(120);
    check_version(ENGINE, request.root, deadline).await?;
    let temp = tempfile::tempdir()
        .into_diagnostic()
        .wrap_err("Could not create PMD configuration directory")?;
    let mut refs = Vec::new();
    let mut rules = BTreeMap::new();
    let mut suppressions = BTreeMap::new();
    for (index, rule) in request.rules.iter().enumerate() {
        let path = temp.path().join(format!("{index}.xml"));
        let config = request.registry.engine_config(rule)?;
        let suppression = match rule.id.as_str() {
            RESOURCE_RULE => {
                let mut conditions = vec![CLOSED_PATTERN_RESOURCE, SHUTDOWN_EXECUTOR];
                if settings.java_target.is_some_and(|target| target < 21) {
                    conditions.push(NON_CLOSEABLE_HTTP_CLIENT);
                }
                if settings.java_target.is_some_and(|target| target < 19) {
                    conditions.push(NON_CLOSEABLE_EXECUTOR);
                }
                let mut xpath = conditions.join(" | ");
                if let Some(signatures) = configured_signatures(settings.resource_owners) {
                    xpath.push_str(&format!(concat!(
                        " | self::VariableId[let $resource := @Name return ",
                        "parent::VariableDeclarator/parent::LocalVariableDeclaration[count(VariableDeclarator) = 1]",
                        "/following-sibling::*[1]/(self::ReturnStatement | self::ExpressionStatement)",
                        "/*[self::ConstructorCall or self::MethodCall][{}]",
                        "[ArgumentList[@Size = 1]/VariableAccess[@Name = $resource]]]"
                    ), signatures));
                }
                if let Some(signatures) = configured_signatures(settings.managed_resource_factories)
                {
                    xpath.push_str(&format!(concat!(
                        " | self::VariableId[let $resource := @Name return ",
                        "parent::VariableDeclarator/MethodCall[{}]",
                        " and not(parent::VariableDeclarator/parent::LocalVariableDeclaration/",
                        "parent::Block//VariableAccess[@Name = $resource and @AccessType != 'READ'])]"
                    ), signatures));
                }
                Some((CLOSE_RESOURCE, Cow::Owned(xpath)))
            }
            PRESERVE_STACK_TRACE_RULE => Some((
                PRESERVE_STACK_TRACE,
                if settings.allow_logged_exceptions {
                    Cow::Owned(format!("{CAUSE_PRESERVING_EXCEPTION} | {LOGGED_EXCEPTION}"))
                } else {
                    Cow::Borrowed(CAUSE_PRESERVING_EXCEPTION)
                },
            )),
            _ => None,
        };
        let contents = suppression.map_or(Cow::Borrowed(config), |(reference, xpath)| {
            let tag = format!(r#"<rule ref="{reference}"/>"#);
            if !config.contains(&tag) {
                return Cow::Borrowed(config);
            }
            let configured = config.replace(
                &tag,
                &format!(r#"<rule ref="{reference}"><properties><property name="violationSuppressXPath" value="{xpath}"/></properties></rule>"#),
            );
            suppressions.insert(rule.id.as_str(), xpath);
            Cow::Owned(configured)
        });
        fs::write(&path, contents.as_bytes())
            .into_diagnostic()
            .wrap_err("Could not write PMD ruleset")?;
        refs.push(path.display().to_string());
        rules.insert(rule.implementation()?.rule_id.as_str(), *rule);
    }

    let file_list = temp.path().join("files.txt");
    let mut list = BufWriter::new(File::create(&file_list).into_diagnostic()?);
    for file in request.files {
        let path = request.root.join(file);
        let path = path.to_str().filter(|path| !path.contains(['\r', '\n']));
        let Some(path) = path else {
            bail!("PMD file lists require UTF-8 paths without line breaks");
        };
        writeln!(list, "{path}").into_diagnostic()?;
    }
    list.flush().into_diagnostic()?;

    let mut check = command(ENGINE, request.root)?;
    check
        .args([
            "check",
            "--no-cache",
            "--no-progress",
            "--no-fail-on-violation",
            "--show-suppressed",
            "--format",
            "json",
            "--encoding",
            "UTF-8",
            "--use-version",
            &language,
            "--file-list",
        ])
        .arg(file_list)
        .arg("--rulesets")
        .arg(refs.join(","));
    if let Some(classpath) = settings.classpath {
        let path = temp.path().join("classpath.txt");
        let mut list = BufWriter::new(File::create(&path).into_diagnostic()?);
        for entry in classpath {
            let entry = entry
                .to_str()
                .filter(|entry| !entry.contains(['\r', '\n']) && *entry == entry.trim_end())
                .ok_or_else(|| miette::miette!("PMD cannot represent a classpath entry"))?;
            writeln!(list, "{entry}").into_diagnostic()?;
        }
        list.flush().into_diagnostic()?;
        check.arg("--aux-classpath").arg(classpath_argument(&path)?);
    }
    let output = run_command(ENGINE, &mut check, deadline).await?;
    let Ok(report): std::result::Result<Scan, _> = serde_json::from_slice(&output.stdout) else {
        bail!("PMD returned missing or invalid JSON ({})", output.status);
    };
    if report.format_version != 0 || report.pmd_version != PMD_VERSION {
        bail!("PMD returned an unsupported report version");
    }
    let expected: BTreeSet<_> = request.files.iter().collect();
    let mut failed_files = BTreeSet::new();
    let mut scan_failed = !output.status.success()
        && (output.status.code() != Some(5) || report.processing_errors.is_empty());
    if scan_failed {
        fail(
            outcome,
            ENGINE,
            format!("PMD scan failed ({})", output.status),
        );
    }
    if !report.configuration_errors.is_empty() {
        scan_failed = true;
        fail(
            outcome,
            ENGINE,
            format!(
                "PMD reported {} configuration errors",
                report.configuration_errors.len()
            ),
        );
    }
    for error in report.processing_errors {
        let mut failure = engine_failure(ENGINE, "PMD could not complete analysis");
        failure.error_type = Some(
            error
                .message
                .split_once(':')
                .map_or("ProcessingError", |(kind, _)| kind)
                .to_owned(),
        );
        if let Some(path) =
            relative_path(request.root, &error.filename).filter(|path| expected.contains(path))
        {
            failure.path = Some(display_path(&path));
            failed_files.insert(path);
        } else {
            scan_failed = true;
        }
        outcome.failures.push(failure);
    }

    if !scan_failed {
        for file in request.files {
            if failed_files.contains(file) {
                continue;
            } else if request.root.join(file).is_file() {
                outcome.scanned_files.insert(file.clone());
            } else {
                let mut failure =
                    engine_failure(ENGINE, "A required PMD input disappeared during the check");
                failure.path = Some(display_path(file));
                failure.error_type = Some("MissingInput".to_owned());
                outcome.failures.push(failure);
            }
        }
    }
    let first_finding_failure = outcome.failures.len();
    let mut sources = Sources::default();
    let mut java = Parser::new();
    java.set_language(&tree_sitter_java::LANGUAGE.into())
        .into_diagnostic()?;
    for item in report.files.into_iter().chain(report.suppressed_violations) {
        let Some(path) =
            relative_path(request.root, &item.filename).filter(|path| expected.contains(path))
        else {
            fail(
                outcome,
                ENGINE,
                "PMD returned a diagnostic outside the requested files",
            );
            continue;
        };
        let mut java_tree = None;
        for finding in item.violations {
            let Some(rule) = rules.get(finding.rule.as_str()) else {
                fail(outcome, ENGINE, "PMD returned an unregistered rule ID");
                continue;
            };
            if finding.beginline == 0 || finding.begincolumn == 0 {
                fail(
                    outcome,
                    ENGINE,
                    "PMD returned an invalid diagnostic position",
                );
                continue;
            }
            if rule.id == UNNECESSARY_IMPORT_RULE
                && finding.is_unused_wildcard_import()
                && (settings.classpath.is_none()
                    || !settings
                        .compile_sources
                        .is_some_and(|files| files.contains(&request.root.join(&path))))
            {
                continue;
            }
            if finding.suppression_type.as_deref() == Some("XPath")
                && suppressions
                    .get(rule.id.as_str())
                    .map(|xpath| xpath.as_ref())
                    .zip(finding.user_message.as_deref())
                    .is_some_and(|(expected, reported)| expected == reported)
            {
                continue;
            }
            let source = sources.get(request.root, &path);
            let start =
                source.and_then(|source| source.offset((finding.beginline, finding.begincolumn)));
            // PMD 7.29 emits an exclusive end column, including at end of line.
            let end_offset = source
                .and_then(|source| source.offset((finding.endline, finding.endcolumn)))
                .filter(|end| start.is_some_and(|start| start < *end));
            let (fingerprint, legacy_fingerprints) = source
                .and_then(|source| Some(source.pmd_fingerprints(start?, end_offset?)))
                .unwrap_or_default();
            let original_start = (finding.beginline, finding.begincolumn);
            let throw_range = (rule.id == PRESERVE_STACK_TRACE_RULE)
                .then(|| {
                    let source = source?;
                    let tree = java_tree.get_or_insert_with(|| java.parse(source.text(), None));
                    throw_statement(tree.as_ref()?, start?)
                })
                .flatten();
            let reported_start = source
                .and_then(|source| source.position(throw_range.as_ref()?.start))
                .or_else(|| start.map(|_| original_start));
            let reported_end = source.and_then(|source| {
                source.position(throw_range.map(|range| range.end).or(end_offset)?)
            });
            let subject = finding.subject(&rule.id).map(str::to_owned);
            outcome.diagnostics.push(Diagnostic {
                path: Some(display_path(&path)),
                module: None,
                class_name: None,
                line: Some(reported_start.map_or(finding.beginline, |(line, _)| line)),
                column: reported_start.map(|(_, column)| column),
                end_line: reported_end.map(|(line, _)| line),
                end_column: reported_end.map(|(_, column)| column),
                message: crate::diagnostic_message(&rule.description, subject.as_deref()),
                subject,
                rule_id: rule.id.clone(),
                engine: rule.engine,
                severity: rule.severity,
                fix: rule.fix.clone(),
                fingerprint,
                edit: None,
                legacy_fingerprints,
            });
        }
    }
    if first_finding_failure != outcome.failures.len() {
        outcome.scanned_files.clear();
    }
    Ok(())
}

fn configured_signatures(signatures: &[String]) -> Option<String> {
    (!signatures.is_empty()).then(|| {
        signatures
            .iter()
            .map(|signature| format!("pmd-java:matchesSig('{signature}')"))
            .collect::<Vec<_>>()
            .join(" or ")
    })
}

fn throw_statement(tree: &Tree, offset: usize) -> Option<Range<usize>> {
    let mut node = tree
        .root_node()
        .descendant_for_byte_range(offset, offset + 1)?;
    loop {
        match node.kind() {
            "throw_statement" => return Some(node.byte_range()),
            "catch_clause" => {
                let mut pending = vec![node.child_by_field_name("body")?];
                let mut statement = None;
                while let Some(child) = pending.pop() {
                    match child.kind() {
                        "throw_statement" if statement.replace(child.byte_range()).is_some() => {
                            return None;
                        }
                        "throw_statement" | "catch_clause" | "lambda_expression" | "class_body" => {
                        }
                        _ => pending.extend(crate::jspecify::children(child)),
                    }
                }
                return statement;
            }
            _ => node = node.parent()?,
        }
    }
}

fn classpath_argument(path: &Path) -> Result<String> {
    let path = path
        .to_str()
        .ok_or_else(|| miette::miette!("PMD classpath file path must be UTF-8"))?;
    let mut argument = String::from("file:");
    // PMD parses a leading slash after "file:" as a URI. Windows paths
    // start with a drive letter or backslash and are read as native paths.
    if path.starts_with('/') {
        use std::fmt::Write as _;
        for byte in path.bytes() {
            if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
                argument.push(char::from(byte));
            } else {
                write!(argument, "%{byte:02X}").expect("writing to a String cannot fail");
            }
        }
    } else {
        argument.push_str(path);
    }
    Ok(argument)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Scan {
    format_version: u8,
    pmd_version: String,
    files: Vec<FileResults>,
    suppressed_violations: Vec<FileResults>,
    processing_errors: Vec<ProcessingError>,
    configuration_errors: Vec<serde_json::Value>,
}

#[derive(Deserialize)]
struct ProcessingError {
    filename: PathBuf,
    message: String,
}

#[derive(Deserialize)]
struct FileResults {
    filename: PathBuf,
    violations: Vec<Finding>,
}

#[derive(Deserialize)]
struct Finding {
    rule: String,
    #[serde(default)]
    description: String,
    #[serde(rename = "suppressiontype")]
    suppression_type: Option<String>,
    #[serde(rename = "usermsg")]
    user_message: Option<String>,
    beginline: u32,
    begincolumn: u32,
    endline: u32,
    endcolumn: u32,
}

impl Finding {
    fn subject(&self, id: &str) -> Option<&str> {
        match id {
            UNNECESSARY_IMPORT_RULE => self
                .description
                .split_once('\'')?
                .1
                .split_once('\'')
                .map(|(name, _)| name),
            PRESERVE_STACK_TRACE_RULE => self
                .description
                .strip_prefix("Thrown exception does not preserve the stack trace of exception '")?
                .split_once('\'')
                .map(|(name, _)| name),
            _ => None,
        }
    }

    fn is_unused_wildcard_import(&self) -> bool {
        // PMD 7.29 reports unused and duplicate imports with distinct messages.
        // Its pretty-printed import names are independent of source layout.
        self.description.ends_with(".*'")
            && (self.description.starts_with("Unused import '")
                || self.description.starts_with("Unused static import '"))
    }
}
