use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use dornguard_config::Rule;
use dornguard_core::{
    Diagnostic, EngineName, FindingGroup, GROUP_EXAMPLES, Module, Outcome, fingerprint,
};
use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

use crate::{display_path, fail, relative_path, run_command};

const ENGINE: EngineName = EngineName::Archunit;
const SCHEMA_VERSION: u32 = 5;

pub struct ArchModule<'a> {
    pub module: &'a Module,
    pub rules: Vec<&'a Rule>,
    pub files: Vec<PathBuf>,
    pub kotlin: bool,
    pub check_class_timestamps: bool,
}

pub async fn check_architecture(root: &Path, modules: &[ArchModule<'_>]) -> Outcome {
    let mut outcome = Outcome::default();
    if !modules.is_empty()
        && let Err(error) = run(root, modules, &mut outcome).await
    {
        fail(&mut outcome, ENGINE, format!("{error:#}"));
    }
    for failure in &mut outcome.failures {
        let affected = modules
            .iter()
            .filter(|candidate| {
                failure.modules.is_empty() || failure.modules.contains(&candidate.module.path)
            })
            .collect::<Vec<_>>();
        failure.modules = affected
            .iter()
            .map(|candidate| candidate.module.path.clone())
            .collect();
        failure.modules.sort();
        failure.set_rules(
            affected
                .iter()
                .flat_map(|candidate| candidate.rules.iter().map(|rule| rule.id.as_str())),
        );
    }
    outcome.record_coverage(modules.iter().flat_map(|candidate| &candidate.files));
    outcome
}

pub async fn inspect_archunit(root: &Path) -> Result<String> {
    let java = crate::inspect_java(root).await?;
    let runner = runner_path()?;
    let mut command = crate::install::java_command(root);
    command.arg("-jar").arg(&runner).arg("--version");
    let output = run_command(
        ENGINE,
        &mut command,
        Instant::now() + Duration::from_secs(15),
    )
    .await?;
    let protocol = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() || protocol.trim().parse::<u32>().ok() != Some(SCHEMA_VERSION) {
        bail!(
            "ArchUnit runner {} did not report protocol {SCHEMA_VERSION}; install the matching runner",
            runner.display()
        );
    }
    Ok(format!("protocol {SCHEMA_VERSION}, {java}"))
}

fn build_request<'a>(root: &Path, modules: &'a [ArchModule<'_>]) -> Result<Request<'a>> {
    let mut request = Request {
        schema_version: SCHEMA_VERSION,
        modules: Vec::new(),
    };
    for candidate in modules {
        let module = candidate.module;
        let mut rules = Vec::new();
        for rule in &candidate.rules {
            let options = rule
                .options
                .as_ref()
                .ok_or_else(|| miette!("Missing ArchUnit options for {}", rule.id))?;
            rules.push(RuleInput {
                id: &rule.id,
                origins: &options.from,
                forbidden: &options.forbidden,
                cycles: options.cycles,
            });
        }
        request.modules.push(ModuleInput {
            path: &module.path,
            source_files: candidate.files.iter().map(|file| root.join(file)).collect(),
            kotlin: candidate.kotlin,
            check_class_timestamps: candidate.check_class_timestamps,
            class_dirs: &module.class_dirs,
            classpath: &module.classpath,
            rules,
        });
    }
    Ok(request)
}

/// Run the ArchUnit runner on a request and read its validated result.
async fn run_runner(root: &Path, request: &Request<'_>) -> Result<Report> {
    let temp = tempfile::tempdir().into_diagnostic()?;
    let request_path = temp.path().join("request.json");
    let result_path = temp.path().join("result.json");
    fs::write(
        &request_path,
        serde_json::to_vec(request).into_diagnostic()?,
    )
    .into_diagnostic()?;
    crate::inspect_java(root).await?;
    let runner = runner_path()?;
    let mut command = crate::install::java_command(root);
    command
        .arg("-jar")
        .arg(runner)
        .arg("--config")
        .arg(request_path)
        .arg("--output")
        .arg(&result_path);
    let output = run_command(
        ENGINE,
        &mut command,
        Instant::now() + Duration::from_secs(120),
    )
    .await?;
    if !output.status.success() {
        bail!("ArchUnit runner failed ({})", output.status);
    }
    let contents = fs::read(result_path)
        .into_diagnostic()
        .wrap_err("ArchUnit did not return a result")?;
    let result: Report = serde_json::from_slice(&contents)
        .into_diagnostic()
        .wrap_err("Invalid ArchUnit result")?;
    if result.schema_version != SCHEMA_VERSION {
        bail!(
            "Unsupported ArchUnit result schema {}",
            result.schema_version
        );
    }
    Ok(result)
}

/// Report modules the runner could not analyze or skipped, and count the files it covered.
fn record_module_status(
    root: &Path,
    modules: &[ArchModule<'_>],
    failures: Vec<RunnerFailure>,
    analyzed: &[String],
    outcome: &mut Outcome,
) {
    let mut failed = BTreeSet::new();
    for error in failures {
        fail(
            outcome,
            ENGINE,
            format!(
                "ArchUnit could not analyze {} ({})",
                error.module, error.reason
            ),
        );
        if let Some(failure) = outcome.failures.last_mut() {
            failure.path = error
                .path
                .and_then(|path| relative_path(root, &path))
                .as_deref()
                .map(display_path);
            failure.modules.push(error.module.clone());
        }
        failed.insert(error.module);
    }
    let analyzed = analyzed.iter().map(String::as_str).collect::<BTreeSet<_>>();
    for candidate in modules {
        if failed.contains(&candidate.module.path) {
            continue;
        }
        if !analyzed.contains(candidate.module.path.as_str()) {
            fail(
                outcome,
                ENGINE,
                format!("ArchUnit skipped module {}", candidate.module.path),
            );
            if let Some(failure) = outcome.failures.last_mut() {
                failure.modules.push(candidate.module.path.clone());
            }
        } else {
            outcome
                .scanned_files
                .extend(candidate.files.iter().cloned());
        }
    }
}

/// What one diagnostic stands for: a forbidden dependency of one class, or one package
/// edge of a cycle.
type GroupKey = (String, String, String, String);

/// The class dependencies that share a diagnostic.
struct Group {
    diagnostic: Diagnostic,
    /// The `(origin, target)` classes that gave the diagnostic its location.
    anchor: (String, String),
    pairs: BTreeSet<(String, String)>,
}

/// The package of a class name, or an empty string for the default package.
fn package_of(class: &str) -> &str {
    class.rsplit_once('.').map_or("", |(package, _)| package)
}

/// Turn runner findings into diagnostics.
///
/// A cycle reports one diagnostic per package edge, and a forbidden dependency one
/// per class, so a single cause is not counted once per class dependency. Each
/// diagnostic keeps the fingerprints of the finer diagnostics it replaces.
fn record_findings(
    root: &Path,
    modules: &[ArchModule<'_>],
    findings: Vec<Finding>,
    outcome: &mut Outcome,
) {
    let scopes: BTreeMap<_, _> = modules
        .iter()
        .map(|candidate| (candidate.module.path.as_str(), candidate))
        .collect();
    let mut groups: BTreeMap<GroupKey, Group> = BTreeMap::new();
    for finding in findings {
        let Some(candidate) = scopes.get(finding.module.as_str()) else {
            fail(
                outcome,
                ENGINE,
                "ArchUnit returned a diagnostic for an unrequested module",
            );
            continue;
        };
        let Some(rule) = candidate
            .rules
            .iter()
            .find(|rule| rule.id == finding.rule_id)
        else {
            fail(
                outcome,
                ENGINE,
                "ArchUnit returned a diagnostic for an unrequested architecture rule",
            );
            continue;
        };
        let path = match &finding.path {
            Some(path) => {
                let Some(path) =
                    relative_path(root, path).filter(|path| candidate.files.contains(path))
                else {
                    fail(
                        outcome,
                        ENGINE,
                        "ArchUnit returned a diagnostic outside the requested source files",
                    );
                    continue;
                };
                Some(display_path(&path))
            }
            None => None,
        };
        let line = finding.line.filter(|_| path.is_some());
        let legacy_fingerprint = if path.is_some() {
            fingerprint(&finding.identity)
        } else {
            fingerprint(&format!(
                "{}\0{}\0{}\0{}",
                finding.module, finding.origin, rule.id, finding.identity
            ))
        };
        let relationship_fingerprint = fingerprint(&format!(
            "{}\0{}\0{}\0{}",
            finding.module, rule.id, finding.origin, finding.target
        ));
        let cycle = rule.options.as_ref().is_some_and(|options| options.cycles);
        let (key, identity) = if cycle {
            let (origin, target) = (
                package_of(&finding.origin).to_owned(),
                package_of(&finding.target).to_owned(),
            );
            let identity = format!(
                "package-edge\0{}\0{}\0{origin}\0{target}",
                finding.module, rule.id
            );
            (
                (finding.module.clone(), rule.id.clone(), origin, target),
                identity,
            )
        } else {
            let identity = format!("class\0{}\0{}\0{}", finding.module, rule.id, finding.origin);
            (
                (
                    finding.module.clone(),
                    rule.id.clone(),
                    finding.origin.clone(),
                    String::new(),
                ),
                identity,
            )
        };
        let diagnostic = Diagnostic {
            path,
            module: Some(finding.module.clone()),
            class_name: Some(finding.origin.clone()),
            line,
            column: None,
            end_line: None,
            end_column: None,
            subject: None,
            rule_id: rule.id.clone(),
            engine: ENGINE,
            severity: rule.severity,
            confidence: rule.confidence,
            message: String::new(),
            fix: rule.fix.clone(),
            edit: None,
            legacy_fingerprints: vec![legacy_fingerprint, relationship_fingerprint],
            group: None,
            fingerprint: Some(fingerprint(&identity)),
        };
        let anchor = (finding.origin.clone(), finding.target.clone());
        let pair = anchor.clone();
        match groups.entry(key) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(Group {
                    diagnostic,
                    anchor,
                    pairs: BTreeSet::from([pair]),
                });
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let group = entry.get_mut();
                group.pairs.insert(pair);
                merge_representative(group, diagnostic, anchor);
            }
        }
    }
    for ((module, rule_id, _, _), mut group) in groups {
        let Some(rule) = scopes
            .get(module.as_str())
            .and_then(|candidate| candidate.rules.iter().find(|rule| rule.id == rule_id))
        else {
            continue;
        };
        let cycle = rule.options.as_ref().is_some_and(|options| options.cycles);
        group.diagnostic.message = group_message(&rule.description, &module, cycle, &group);
        group.diagnostic.group = Some(FindingGroup {
            occurrences: group.pairs.len(),
            examples: group_examples(cycle, &group),
        });
        outcome.diagnostics.push(group.diagnostic);
    }
}

/// Keep the most precise location in a group, remembering every legacy fingerprint.
fn merge_representative(group: &mut Group, diagnostic: Diagnostic, anchor: (String, String)) {
    let representative = &mut group.diagnostic;
    let precision = |item: &Diagnostic, anchor: &(String, String)| {
        (
            item.path.is_none(),
            item.line.is_none(),
            item.line,
            anchor.clone(),
        )
    };
    let mut legacy = std::mem::take(&mut representative.legacy_fingerprints);
    legacy.extend(diagnostic.legacy_fingerprints.iter().cloned());
    if precision(&diagnostic, &anchor) < precision(representative, &group.anchor) {
        *representative = diagnostic;
        group.anchor = anchor;
    }
    representative.legacy_fingerprints = legacy;
}

/// A few of the dependencies behind a diagnostic: a class pair for a cycle, a forbidden
/// type otherwise.
fn group_examples(cycle: bool, group: &Group) -> Vec<String> {
    group
        .pairs
        .iter()
        .take(GROUP_EXAMPLES)
        .map(|(origin, target)| {
            if cycle {
                format!("{origin} -> {target}")
            } else {
                target.clone()
            }
        })
        .collect()
}

fn group_message(description: &str, module: &str, cycle: bool, group: &Group) -> String {
    let count = group.pairs.len();
    if count == 1 {
        let (origin, target) = &group.anchor;
        return format!("{description} {origin} depends on {target} in {module}.");
    }
    let more = count.saturating_sub(GROUP_EXAMPLES);
    let more = if more > 0 {
        format!(" and {more} more")
    } else {
        String::new()
    };
    let examples = group_examples(cycle, group).join(", ");
    if cycle {
        let (origin, target) = &group.anchor;
        format!(
            "{description} Package {} depends on {} through {count} class dependencies in {module}, for example {examples}{more}.",
            package_of(origin),
            package_of(target),
        )
    } else {
        let origin = &group.anchor.0;
        format!(
            "{description} {origin} depends on {count} forbidden types in {module}: {examples}{more}."
        )
    }
}

async fn run(root: &Path, modules: &[ArchModule<'_>], outcome: &mut Outcome) -> Result<()> {
    let request = build_request(root, modules)?;
    let result = run_runner(root, &request).await?;
    record_module_status(root, modules, result.failures, &result.analyzed, outcome);
    record_findings(root, modules, result.findings, outcome);
    Ok(())
}

/// The running executable with symlinks resolved, so bundled files are found
/// next to the real binary even when it is installed through a link.
fn installed_exe() -> Result<PathBuf> {
    Ok(resolve_links(std::env::current_exe().into_diagnostic()?))
}

fn resolve_links(path: PathBuf) -> PathBuf {
    dunce::canonicalize(&path).unwrap_or(path)
}

#[cfg(all(test, unix))]
mod symlink_tests {
    use super::*;

    #[test]
    fn bundled_files_are_found_next_to_the_real_executable() {
        let dir = tempfile::tempdir().unwrap();
        let release = dir.path().join("releases");
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&release).unwrap();
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(release.join("dornguard"), "").unwrap();
        std::os::unix::fs::symlink(release.join("dornguard"), bin.join("dornguard")).unwrap();
        let resolved = resolve_links(bin.join("dornguard")).with_file_name("archunit-runner.jar");
        assert_eq!(
            resolved,
            dunce::canonicalize(&release)
                .unwrap()
                .join("archunit-runner.jar")
        );
    }
}

fn runner_path() -> Result<PathBuf> {
    let path = match std::env::var_os("DORNGUARD_ARCHUNIT_RUNNER") {
        Some(path) => PathBuf::from(path),
        None => installed_exe()?.with_file_name("archunit-runner.jar"),
    };
    let path = dunce::canonicalize(&path).into_diagnostic().wrap_err_with(|| {
        format!(
            "Could not find the ArchUnit runner at {}; set DORNGUARD_ARCHUNIT_RUNNER to its JAR",
            path.display()
        )
    })?;
    if let Some(checksum) = option_env!("DORNGUARD_ARCHUNIT_SHA256") {
        crate::install::verify_checksum(&path, checksum)
            .wrap_err("ArchUnit runner verification failed")?;
    }
    Ok(path)
}

pub fn maven_model_path() -> Result<PathBuf> {
    let runner = std::env::var_os("DORNGUARD_ARCHUNIT_RUNNER");
    let path = match &runner {
        Some(runner) => PathBuf::from(runner).with_file_name("maven-model.jar"),
        None => installed_exe()?.with_file_name("maven-model.jar"),
    };
    let path = dunce::canonicalize(&path).into_diagnostic().wrap_err_with(|| {
        let hint = if runner.is_some() {
            "maven-model.jar must be in the same directory as DORNGUARD_ARCHUNIT_RUNNER; the runner's jar task builds both"
        } else {
            "use a matching release bundle, or build the runner's jar task and set DORNGUARD_ARCHUNIT_RUNNER to archunit-runner.jar, whose directory also holds maven-model.jar"
        };
        format!(
            "Could not find the Maven extension at {}; {hint}",
            path.display()
        )
    })?;
    if let Some(checksum) = option_env!("DORNGUARD_MAVEN_MODEL_SHA256") {
        crate::install::verify_checksum(&path, checksum)
            .wrap_err("Maven extension verification failed")?;
    }
    Ok(path)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Request<'a> {
    schema_version: u32,
    modules: Vec<ModuleInput<'a>>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ModuleInput<'a> {
    path: &'a str,
    source_files: Vec<PathBuf>,
    kotlin: bool,
    check_class_timestamps: bool,
    class_dirs: &'a [PathBuf],
    classpath: &'a [PathBuf],
    rules: Vec<RuleInput<'a>>,
}

#[derive(Serialize)]
struct RuleInput<'a> {
    id: &'a str,
    origins: &'a [String],
    forbidden: &'a [String],
    cycles: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Report {
    schema_version: u32,
    analyzed: Vec<String>,
    findings: Vec<Finding>,
    failures: Vec<RunnerFailure>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Finding {
    module: String,
    rule_id: String,
    path: Option<PathBuf>,
    line: Option<u32>,
    origin: String,
    target: String,
    identity: String,
}

#[derive(Deserialize)]
struct RunnerFailure {
    module: String,
    path: Option<PathBuf>,
    reason: String,
}

#[cfg(test)]
mod group_tests {
    use super::*;

    fn group(pairs: &[(&str, &str)]) -> Group {
        let anchor = (pairs[0].0.to_owned(), pairs[0].1.to_owned());
        Group {
            diagnostic: Diagnostic {
                path: None,
                module: None,
                class_name: None,
                line: None,
                column: None,
                end_line: None,
                end_column: None,
                subject: None,
                rule_id: String::new(),
                engine: ENGINE,
                severity: dornguard_core::Severity::Error,
                confidence: dornguard_core::Confidence::default(),
                message: String::new(),
                fix: String::new(),
                edit: None,
                legacy_fingerprints: Vec::new(),
                group: None,
                fingerprint: None,
            },
            anchor,
            pairs: pairs
                .iter()
                .map(|(origin, target)| ((*origin).to_owned(), (*target).to_owned()))
                .collect(),
        }
    }

    #[test]
    fn packages_come_from_the_class_name() {
        assert_eq!(package_of("a.b.C"), "a.b");
        assert_eq!(package_of("a.b.C$Inner"), "a.b");
        assert_eq!(package_of("Top"), "");
    }

    #[test]
    fn a_single_dependency_keeps_the_original_message() {
        let group = group(&[("a.A", "b.B")]);
        assert_eq!(
            group_message("Rule.", ":m", true, &group),
            "Rule. a.A depends on b.B in :m."
        );
    }

    #[test]
    fn many_forbidden_types_are_counted_with_examples() {
        let group = group(&[
            ("a.A", "s.T1"),
            ("a.A", "s.T2"),
            ("a.A", "s.T3"),
            ("a.A", "s.T4"),
        ]);
        assert_eq!(
            group_message("Rule.", ":m", false, &group),
            "Rule. a.A depends on 4 forbidden types in :m: s.T1, s.T2, s.T3 and 1 more."
        );
    }

    #[test]
    fn examples_are_the_class_pairs_of_a_cycle_and_the_types_otherwise() {
        let pairs = [
            ("a.A1", "b.B"),
            ("a.A2", "b.B"),
            ("a.A3", "b.B"),
            ("a.A4", "b.B"),
        ];
        let group = group(&pairs);
        assert_eq!(
            group_examples(true, &group),
            ["a.A1 -> b.B", "a.A2 -> b.B", "a.A3 -> b.B"]
        );
        let forbidden = self::group(&[("a.A", "s.T1"), ("a.A", "s.T2")]);
        assert_eq!(group_examples(false, &forbidden), ["s.T1", "s.T2"]);
    }

    #[test]
    fn a_cycle_edge_names_both_packages() {
        let group = group(&[("a.A1", "b.B"), ("a.A2", "b.B")]);
        assert_eq!(
            group_message("Rule.", ":m", true, &group),
            "Rule. Package a depends on b through 2 class dependencies in :m, for example a.A1 -> b.B, a.A2 -> b.B."
        );
    }
}
