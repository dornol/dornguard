use crate::{build_check, changed, flyway, suppressions};
use std::collections::BTreeSet;
use std::fs::{self};
use std::io::{self};
use std::path::{Path, PathBuf};

use dornguard_config::{
    CONFIG_FILE, FLYWAY_RULE, JPA_RESPONSE_RULE, PartialParsing, ProjectConfig, Registry, Rule,
};
use dornguard_core::{EngineName, Failure, FailureKind, Outcome, Scope, SourceLanguage};
use dornguard_engine::{Engine, Eslint, Pmd, Request, Semgrep, check_builtin};
use ignore::WalkBuilder;
use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};

#[derive(Default)]
pub(crate) struct ProfileSelection<'a> {
    names: &'a [String],
    exclusive: bool,
}

impl<'a> ProfileSelection<'a> {
    pub(crate) fn from_cli(additional: &'a [String], only: &'a [String]) -> Self {
        Self {
            names: if only.is_empty() { additional } else { only },
            exclusive: !only.is_empty(),
        }
    }
}

pub(crate) async fn check(
    target: &Path,
    selection: ProfileSelection<'_>,
    config: Option<&Path>,
    module: Option<&str>,
    changed: Option<changed::Request<'_>>,
    organization: Option<&Path>,
    resolve_imports: bool,
) -> Outcome {
    let config_error = |error| failure(FailureKind::ConfigurationError, error);
    let prep_error = |error| failure(FailureKind::PreparationError, error);
    let target = match dunce::canonicalize(target)
        .into_diagnostic()
        .wrap_err_with(|| format!("Could not access {}", target.display()))
    {
        Ok(path) => path,
        Err(error) => return prep_error(error),
    };
    let project = match find_config(&target, config) {
        Ok(path) => path,
        Err(error) => return config_error(error),
    };
    let Some(root) = project.parent() else {
        return config_error(miette!("Configuration must be a file inside a project"));
    };
    let policy = if let Some(path) = organization {
        match trusted_policy(root, path) {
            Ok(path) => path,
            Err(error) => return config_error(error),
        }
    } else {
        project.clone()
    };
    let mut config = match Registry::load(&policy) {
        Ok(config) => config,
        Err(error) => return config_error(error),
    };
    if selection.exclusive {
        config.use_only_cli_profiles();
    }
    let profiles = selection.names;
    let registry = match Registry::for_config(&policy, &config) {
        Ok(registry) => registry,
        Err(error) => return config_error(error),
    };
    let resolved_rules = match registry.resolve(&config, None, profiles) {
        Ok(rules) => rules,
        Err(error) => return config_error(error),
    };
    let rules = resolved_rules.iter().collect::<Vec<_>>();
    let flyway_rule = rules.iter().copied().find(|rule| rule.id == FLYWAY_RULE);
    let mut jpa_enabled = rules.iter().any(|rule| rule.id == JPA_RESPONSE_RULE);
    let mut jspecify_enabled = rules
        .iter()
        .any(|rule| rule.id == "java.jspecify-null-marked");
    for module_name in config.modules.keys() {
        let module_rules = match registry.resolve(&config, Some(module_name), profiles) {
            Ok(rules) => rules,
            Err(error) => return config_error(error),
        };
        if module_rules
            .iter()
            .find(|rule| rule.id == FLYWAY_RULE)
            .map(|rule| rule.severity)
            != flyway_rule.map(|rule| rule.severity)
        {
            return config_error(miette!("The flyway rule is configured at project scope"));
        }
        jpa_enabled |= module_rules.iter().any(|rule| rule.id == JPA_RESPONSE_RULE);
        jspecify_enabled |= module_rules
            .iter()
            .any(|rule| rule.id == "java.jspecify-null-marked");
    }
    let global_kotlin = match registry.kotlin_profile_enabled(&config, None, profiles) {
        Ok(enabled) => enabled || rules.iter().any(|rule| rule.enables_kotlin()),
        Err(error) => return config_error(error),
    };
    let mut fallback_reason = None;
    let changes = if organization.is_none()
        && let Some(request) = changed
    {
        match changed::discover(root, &project, config.local_ruleset_path(), request).await {
            Ok(changes) if jspecify_enabled && changes.java_scope_changed() => {
                fallback_reason = Some("A Java package or module declaration changed".to_owned());
                None
            }
            Ok(changes)
                if jpa_enabled
                    && changes
                        .paths()
                        .any(|path| SourceLanguage::of(path) == Some(SourceLanguage::Java)) =>
            {
                fallback_reason = Some(
                    "A Java change may affect JPA response types in unchanged controllers"
                        .to_owned(),
                );
                None
            }
            Ok(changes) => Some(changes),
            Err(error) => {
                fallback_reason = Some(error.to_string());
                None
            }
        }
    } else {
        None
    };
    let scope = if changes.is_some() {
        Scope::Changed
    } else if organization.is_none() && changed.is_none() && (target != root || module.is_some()) {
        Scope::Selected
    } else {
        Scope::Full
    };
    let target = if changed.is_some() || organization.is_some() {
        root
    } else {
        &target
    };
    let git_outcome = if let Some(rule) = flyway_rule {
        match flyway::check(root, target, changed.and_then(|request| request.base), rule).await {
            Ok(outcome) => outcome,
            Err(error) => {
                let mut outcome = prep_error(error.wrap_err("Flyway Git comparison failed"));
                outcome.failures[0].set_rules([FLYWAY_RULE]);
                outcome
            }
        }
    } else {
        Outcome::default()
    };
    let uses_build = global_kotlin
        || module.is_some()
        || !config.modules.is_empty()
        || !config.module_dependencies.forbid.is_empty()
        || rules.iter().any(|rule| rule.engine == EngineName::Archunit);
    let tracked = if organization.is_some() {
        BTreeSet::new()
    } else {
        match changed::tracked_sources(root).await {
            Ok(tracked) => tracked,
            Err(error) => return prep_error(error),
        }
    };
    let sources = SourceFiles {
        root,
        tracked,
        required: organization.is_some(),
    };
    if uses_build || resolve_imports && rules.iter().any(|rule| rule.uses_pmd_classpath()) {
        let (module_git_outcome, git_outcome) = if module.is_some() {
            (git_outcome, Outcome::default())
        } else {
            (Outcome::default(), git_outcome)
        };
        let build = build_check::BuildSystem::detect(root);
        let maven_full = build == build_check::BuildSystem::Maven && changes.is_some();
        let mut outcome = build_check::check(build_check::Check {
            build,
            root,
            target,
            filter: module,
            registry: &registry,
            config: &config,
            profiles,
            global: &rules,
            global_kotlin,
            resolve_imports,
            sources: &sources,
            changes: if maven_full { None } else { changes.as_ref() },
            required: organization.is_some(),
            module_git_outcome,
        })
        .await;
        if !git_outcome.diagnostics.is_empty() {
            outcome.no_changes = false;
        }
        outcome.merge(git_outcome);
        if outcome.fallback_reason.is_none() {
            outcome.scope = if maven_full { Scope::Full } else { scope };
            outcome.fallback_reason = if maven_full {
                Some("Maven build checks scan the full reactor".to_owned())
            } else {
                fallback_reason
            };
        }
        if organization.is_none() {
            suppressions::apply(root, &config.ignore, &mut outcome);
        }
        return outcome;
    }
    let scoped_prep_error = |error| {
        let mut outcome = prep_error(error);
        outcome.scope = scope;
        outcome.fallback_reason = fallback_reason.clone();
        outcome
    };
    if rules.is_empty() {
        return scoped_prep_error(miette!(
            "No rules apply; select a profile with active rules"
        ));
    }
    let mut files = if rules.iter().all(|rule| rule.id == FLYWAY_RULE) {
        Vec::new()
    } else {
        match sources.collect(target, |language| {
            rules.iter().any(|rule| rule.applies_to(language))
        }) {
            Ok(files) => files,
            Err(error) => {
                let mut outcome = scoped_prep_error(error);
                outcome.merge(git_outcome);
                return outcome;
            }
        }
    };
    if let Some(changes) = &changes {
        files.retain(|file| changes.matches(file));
        if files.is_empty() && flyway_rule.is_none() {
            return Outcome {
                scope,
                no_changes: true,
                ..Outcome::default()
            };
        }
    }
    if files.is_empty() {
        if flyway_rule.is_some() {
            let mut outcome = git_outcome;
            outcome.scope = scope;
            outcome.fallback_reason = fallback_reason;
            outcome.no_changes =
                changes.is_some() && outcome.diagnostics.is_empty() && outcome.failures.is_empty();
            if organization.is_none() {
                suppressions::apply(root, &config.ignore, &mut outcome);
            }
            return outcome;
        }
        return scoped_prep_error(miette!("No source files apply to this check"));
    }
    let previous_count = files.len();
    files.retain(|file| !config.excludes_source(file));
    let mut outcome = run_source_engines(
        root,
        &files,
        &registry,
        &rules,
        &config,
        None,
        organization.is_some(),
    )
    .await;
    outcome.merge(git_outcome);
    outcome.no_changes = previous_count > 0 && files.is_empty() && flyway_rule.is_none();
    outcome.scope = scope;
    outcome.fallback_reason = fallback_reason;
    if organization.is_none() {
        suppressions::apply(root, &config.ignore, &mut outcome);
    }
    outcome
}

pub(crate) async fn run_source_engines(
    root: &Path,
    files: &[PathBuf],
    registry: &Registry,
    rules: &[&dornguard_config::Rule],
    config: &ProjectConfig,
    java_source: Option<u16>,
    required: bool,
) -> Outcome {
    // Rules skip test sources unless they declare `skip_java_tests: false`.
    let (tests, main): (Vec<_>, Vec<_>) = files
        .iter()
        .cloned()
        .partition(|path| dornguard_config::is_test_source(path));
    let test_rules = rules
        .iter()
        .copied()
        .filter(|rule| !rule.skips_tests())
        .collect::<Vec<_>>();
    let mut outcome = Outcome::default();
    if !main.is_empty() || tests.is_empty() || test_rules.is_empty() {
        outcome = run_engines(root, &main, registry, rules, config, java_source, required).await;
    }
    if !tests.is_empty() && !test_rules.is_empty() {
        outcome.merge(
            run_engines(
                root,
                &tests,
                registry,
                &test_rules,
                config,
                java_source,
                required,
            )
            .await,
        );
    }
    outcome
}

async fn run_engines(
    root: &Path,
    files: &[PathBuf],
    registry: &Registry,
    rules: &[&dornguard_config::Rule],
    config: &ProjectConfig,
    java_source: Option<u16>,
    required: bool,
) -> Outcome {
    let warn_on_thymeleaf_partial =
        !required && config.thymeleaf_partial_parsing == PartialParsing::Warning;
    let check_group = async |engine, files: Vec<PathBuf>, accepts_rule: fn(&Rule) -> bool| {
        let languages = files
            .iter()
            .filter_map(|path| SourceLanguage::of(path))
            .collect::<std::collections::BTreeSet<_>>();
        let rules = rules
            .iter()
            .copied()
            .filter(|rule| {
                rule.engine == engine
                    && accepts_rule(rule)
                    && languages.iter().any(|&language| rule.applies_to(language))
            })
            .collect::<Vec<_>>();
        if rules.is_empty() {
            return Outcome::default();
        }
        let files = files
            .into_iter()
            .filter(|path| {
                SourceLanguage::of(path)
                    .is_some_and(|language| rules.iter().any(|rule| rule.applies_to(language)))
            })
            .collect::<Vec<_>>();
        let request = Request {
            root,
            files: &files,
            rules: &rules,
            registry,
            required,
        };
        match engine {
            EngineName::Semgrep => {
                Semgrep::with_thymeleaf(&config.thymeleaf_sanitizers, warn_on_thymeleaf_partial)
                    .check(request)
                    .await
            }
            EngineName::Pmd => {
                Pmd {
                    java_target: config.java_target,
                    java_source: config.java_source.or(java_source),
                    allow_logged_exceptions: config.allow_logged_exceptions,
                    resource_owners: &config.java_resource_owners,
                    managed_resource_factories: &config.java_managed_resource_factories,
                    ..Pmd::default()
                }
                .check(request)
                .await
            }
            EngineName::Eslint => Eslint.check(request).await,
            EngineName::Builtin => check_builtin(request, &config.format).await,
            _ => unreachable!("source runner requires a source engine"),
        }
    };
    let semgrep = async {
        // Keep template rules off ordinary HTML and generic JVM rules off web sources.
        let mut outcome = Outcome::default();
        for group in [
            &[SourceLanguage::Java, SourceLanguage::Kotlin][..],
            &[SourceLanguage::Template],
            &[SourceLanguage::Html],
            &[SourceLanguage::Javascript],
            &[SourceLanguage::Jsp],
        ] {
            let files: Vec<_> = files
                .iter()
                .filter(|path| SourceLanguage::of(path).is_some_and(|kind| group.contains(&kind)))
                .cloned()
                .collect();
            if warn_on_thymeleaf_partial && group == [SourceLanguage::Template] {
                outcome.merge(
                    check_group(EngineName::Semgrep, files.clone(), |rule| {
                        rule.id.starts_with("thymeleaf.")
                    })
                    .await,
                );
                outcome.merge(
                    check_group(EngineName::Semgrep, files, |rule| {
                        !rule.id.starts_with("thymeleaf.")
                    })
                    .await,
                );
            } else {
                outcome.merge(check_group(EngineName::Semgrep, files, |_| true).await);
            }
        }
        outcome
    };
    let (mut outcome, pmd, eslint, builtin) = tokio::join!(
        semgrep,
        check_group(EngineName::Pmd, files.to_vec(), |_| true),
        check_group(EngineName::Eslint, files.to_vec(), |_| true),
        check_group(EngineName::Builtin, files.to_vec(), |_| true),
    );
    outcome.merge(pmd);
    outcome.merge(eslint);
    outcome.merge(builtin);
    outcome
}

pub(crate) fn find_config(target: &Path, config: Option<&Path>) -> Result<PathBuf> {
    discover_config(target, config)?
        .ok_or_else(|| miette!("No {CONFIG_FILE} found; run dornguard init at the project root"))
}

pub(crate) fn discover_config(target: &Path, config: Option<&Path>) -> Result<Option<PathBuf>> {
    if let Some(path) = config {
        return dunce::canonicalize(path)
            .map(Some)
            .into_diagnostic()
            .wrap_err_with(|| format!("Could not access configuration {}", path.display()));
    }
    let directory = if target.is_dir() {
        Some(target)
    } else {
        target.parent()
    }
    .ok_or_else(|| miette!("No project directory found"))?;
    for ancestor in directory.ancestors() {
        let path = ancestor.join(CONFIG_FILE);
        match fs::symlink_metadata(&path) {
            Ok(_) => return Ok(Some(path)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(error).into_diagnostic().wrap_err_with(|| {
                    format!("Could not access configuration {}", path.display())
                });
            }
        }
    }
    Ok(None)
}

pub(crate) fn current_registry() -> Result<Registry> {
    let cwd = dunce::canonicalize(".")
        .into_diagnostic()
        .wrap_err("Could not resolve the current directory")?;
    if let Some(path) = cwd
        .ancestors()
        .map(|ancestor| ancestor.join(CONFIG_FILE))
        .find(|path| path.is_file())
    {
        let config = Registry::load(&path)?;
        Registry::for_config(&path, &config)
    } else {
        Registry::builtin()
    }
}

pub(crate) fn trusted_policy(root: &Path, path: &Path) -> Result<PathBuf> {
    let path = dunce::canonicalize(path)
        .into_diagnostic()
        .wrap_err("Could not access the organization policy")?;
    let mut worktree = root.to_path_buf();
    for directory in root.ancestors() {
        if directory.join(".git").exists() {
            worktree = directory.to_path_buf();
            break;
        }
    }
    if path.starts_with(&worktree) {
        bail!("Organization policy must be outside the project worktree");
    }
    let config = Registry::load(&path)?;
    config.validate_trusted_ruleset(&path, &worktree)?;
    Ok(path)
}

pub(crate) struct SourceFiles<'a> {
    pub(crate) root: &'a Path,
    tracked: BTreeSet<PathBuf>,
    required: bool,
}

impl SourceFiles<'_> {
    pub(crate) fn collect(
        &self,
        target: &Path,
        accepts: impl Fn(SourceLanguage) -> bool,
    ) -> Result<Vec<PathBuf>> {
        let root = self.root;
        let required = self.required;
        if !target.starts_with(root) {
            bail!("The check target must be inside the configuration directory");
        }
        let mut files = BTreeSet::new();
        let mut walker = WalkBuilder::new(target);
        let web = [
            SourceLanguage::Html,
            SourceLanguage::Template,
            SourceLanguage::Javascript,
            SourceLanguage::Jsp,
        ]
        .into_iter()
        .any(&accepts);
        walker
            .require_git(false)
            .git_global(false)
            .git_exclude(false)
            .ignore(false)
            .hidden(!required)
            .git_ignore(!required)
            .filter_entry(move |entry| {
                entry.file_name() != ".git" && (!web || entry.file_name() != "node_modules")
            });
        let allowed = |relative: &Path| {
            SourceLanguage::of(relative).is_some_and(&accepts)
                && !relative.components().any(|part| {
                    part.as_os_str() == ".git" || web && part.as_os_str() == "node_modules"
                })
        };
        for entry in walker.build() {
            let entry = entry
                .into_diagnostic()
                .wrap_err("Could not enumerate source files")?;
            if let Some(error) = entry.error() {
                bail!("Could not read project ignore rules: {error}");
            }
            let relative = entry.path().strip_prefix(root).into_diagnostic()?;
            if entry.file_type().is_some_and(|kind| kind.is_file()) && allowed(relative) {
                if relative.to_str().is_none() {
                    bail!(
                        "Source path is not representable as UTF-8: {}",
                        entry.path().display()
                    );
                }
                files.insert(relative.to_path_buf());
            }
        }
        let relative_target = target.strip_prefix(root).into_diagnostic()?;
        for path in self
            .tracked
            .iter()
            .filter(|path| path.starts_with(relative_target) && allowed(path))
        {
            if files.contains(path) {
                continue;
            }
            let absolute = root.join(path);
            match fs::symlink_metadata(&absolute) {
                Ok(metadata) if metadata.is_file() => {
                    // The walker does not follow symlinks, including parents of tracked paths.
                    if dunce::canonicalize(&absolute).into_diagnostic()? == absolute {
                        files.insert(path.clone());
                    }
                }
                Ok(_) => {}
                Err(error)
                    if error.kind() == io::ErrorKind::NotFound
                        || error.kind() == io::ErrorKind::NotADirectory => {}
                Err(error) => {
                    return Err(error).into_diagnostic().wrap_err_with(|| {
                        format!("Could not inspect tracked source {}", path.display())
                    });
                }
            }
        }
        Ok(files.into_iter().collect())
    }
}

pub(crate) fn failure(kind: FailureKind, error: miette::Report) -> Outcome {
    Outcome {
        failures: vec![Failure::new(kind, format!("{error:#}"))],
        ..Outcome::default()
    }
}
