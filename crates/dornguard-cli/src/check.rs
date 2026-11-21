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

/// A project's configuration, loaded for one check.
struct Loaded {
    target: PathBuf,
    project: PathBuf,
    config: ProjectConfig,
    registry: Registry,
}

fn load_project(
    target: &Path,
    selection: &ProfileSelection<'_>,
    config: Option<&Path>,
    organization: Option<&Path>,
) -> std::result::Result<Loaded, Box<Outcome>> {
    let config_error = |error| Box::new(failure(FailureKind::ConfigurationError, error));
    let target = dunce::canonicalize(target)
        .into_diagnostic()
        .wrap_err_with(|| format!("Could not access {}", target.display()))
        .map_err(|error| Box::new(failure(FailureKind::PreparationError, error)))?;
    let project = find_config(&target, config).map_err(config_error)?;
    let root = project
        .parent()
        .ok_or_else(|| config_error(miette!("Configuration must be a file inside a project")))?;
    let policy = match organization {
        Some(path) => trusted_policy(root, path).map_err(config_error)?,
        None => project.clone(),
    };
    let mut project_config = Registry::load(&policy).map_err(config_error)?;
    if selection.exclusive {
        project_config.use_only_cli_profiles();
    }
    let registry = Registry::for_config(&policy, &project_config).map_err(config_error)?;
    Ok(Loaded {
        target,
        project,
        config: project_config,
        registry,
    })
}

/// Rule features that change how a check is scoped.
struct Features {
    jpa: bool,
    jspecify: bool,
    kotlin: bool,
}

fn scope_features(
    registry: &Registry,
    config: &ProjectConfig,
    profiles: &[String],
    rules: &[&Rule],
) -> Result<Features> {
    let flyway_severity = rules
        .iter()
        .find(|rule| rule.id == FLYWAY_RULE)
        .map(|rule| rule.severity);
    let has = |rules: &[&Rule], id: &str| rules.iter().any(|rule| rule.id == id);
    let mut jpa = has(rules, JPA_RESPONSE_RULE);
    let mut jspecify = has(rules, "java.jspecify-null-marked");
    for module_name in config.modules.keys() {
        let resolved = registry.resolve(config, Some(module_name), profiles)?;
        let module_rules = resolved.iter().collect::<Vec<_>>();
        let module_flyway = module_rules
            .iter()
            .find(|rule| rule.id == FLYWAY_RULE)
            .map(|rule| rule.severity);
        if module_flyway != flyway_severity {
            bail!("The flyway rule is configured at project scope");
        }
        jpa |= has(&module_rules, JPA_RESPONSE_RULE);
        jspecify |= has(&module_rules, "java.jspecify-null-marked");
    }
    let kotlin = registry.kotlin_profile_enabled(config, None, profiles)?
        || rules.iter().any(|rule| rule.enables_kotlin());
    Ok(Features {
        jpa,
        jspecify,
        kotlin,
    })
}

/// Find the changed files to check, or say why the check must scan everything.
async fn changes_in_scope(
    root: &Path,
    project: &Path,
    config: &ProjectConfig,
    request: changed::Request<'_>,
    features: &Features,
) -> (Option<changed::Changes>, Option<String>) {
    match changed::discover(root, project, config.local_ruleset_path(), request).await {
        Ok(changes) if features.jspecify && changes.java_scope_changed() => (
            None,
            Some("A Java package or module declaration changed".to_owned()),
        ),
        Ok(changes) if features.jpa && changes.may_change_entities(root) => (
            None,
            Some("A Java change may affect JPA response types in unchanged controllers".to_owned()),
        ),
        Ok(changes) => (Some(changes), None),
        Err(error) => (None, Some(error.to_string())),
    }
}

/// What one check resolved before choosing between build-backed and source-only engines.
struct Prepared<'a> {
    root: &'a Path,
    target: &'a Path,
    config: &'a ProjectConfig,
    registry: &'a Registry,
    profiles: &'a [String],
    rules: &'a [&'a Rule],
    sources: SourceFiles<'a>,
    module: Option<&'a str>,
    changes: Option<changed::Changes>,
    scope: Scope,
    fallback_reason: Option<String>,
    flyway_rule: Option<&'a Rule>,
    required: bool,
    resolve_imports: bool,
}

impl Prepared<'_> {
    fn apply_suppressions(&self, outcome: &mut Outcome) {
        if !self.required {
            suppressions::apply(self.root, &self.config.ignore, outcome);
        }
    }

    fn scoped_prep_error(&self, error: miette::Report) -> Outcome {
        let mut outcome = failure(FailureKind::PreparationError, error);
        outcome.scope = self.scope;
        outcome.fallback_reason = self.fallback_reason.clone();
        outcome
    }

    async fn check_with_build(self, git_outcome: Outcome, global_kotlin: bool) -> Outcome {
        let (module_git_outcome, git_outcome) = if self.module.is_some() {
            (git_outcome, Outcome::default())
        } else {
            (Outcome::default(), git_outcome)
        };
        let build = build_check::BuildSystem::detect(self.root);
        let maven_full = build == build_check::BuildSystem::Maven && self.changes.is_some();
        let mut outcome = build_check::check(build_check::Check {
            build,
            root: self.root,
            target: self.target,
            filter: self.module,
            registry: self.registry,
            config: self.config,
            profiles: self.profiles,
            global: self.rules,
            global_kotlin,
            resolve_imports: self.resolve_imports,
            sources: &self.sources,
            changes: if maven_full {
                None
            } else {
                self.changes.as_ref()
            },
            required: self.required,
            module_git_outcome,
        })
        .await;
        if !git_outcome.diagnostics.is_empty() {
            outcome.no_changes = false;
        }
        outcome.merge(git_outcome);
        if outcome.fallback_reason.is_none() {
            outcome.scope = if maven_full { Scope::Full } else { self.scope };
            outcome.fallback_reason = if maven_full {
                Some("Maven build checks scan the full reactor".to_owned())
            } else {
                self.fallback_reason.clone()
            };
        }
        self.apply_suppressions(&mut outcome);
        outcome
    }

    async fn check_sources_only(self, git_outcome: Outcome) -> Outcome {
        let rules = self.rules;
        if rules.is_empty() {
            return self.scoped_prep_error(miette!(
                "No rules apply; select a profile with active rules"
            ));
        }
        let mut files = if rules.iter().all(|rule| rule.id == FLYWAY_RULE) {
            Vec::new()
        } else {
            match self.sources.collect(self.target, |language| {
                rules.iter().any(|rule| rule.applies_to(language))
            }) {
                Ok(files) => files,
                Err(error) => {
                    let mut outcome = self.scoped_prep_error(error);
                    outcome.merge(git_outcome);
                    return outcome;
                }
            }
        };
        if let Some(changes) = &self.changes {
            files.retain(|file| changes.matches(file));
            if files.is_empty() && self.flyway_rule.is_none() {
                return Outcome {
                    scope: self.scope,
                    no_changes: true,
                    ..Outcome::default()
                };
            }
        }
        if files.is_empty() {
            if self.flyway_rule.is_some() {
                let mut outcome = git_outcome;
                outcome.scope = self.scope;
                outcome.fallback_reason = self.fallback_reason.clone();
                outcome.no_changes = self.changes.is_some()
                    && outcome.diagnostics.is_empty()
                    && outcome.failures.is_empty();
                self.apply_suppressions(&mut outcome);
                return outcome;
            }
            return self.scoped_prep_error(miette!("No source files apply to this check"));
        }
        let previous_count = files.len();
        files.retain(|file| !self.config.excludes_source(file));
        let mut outcome = run_source_engines(
            self.root,
            &files,
            self.registry,
            rules,
            self.config,
            None,
            self.required,
        )
        .await;
        outcome.merge(git_outcome);
        outcome.no_changes = previous_count > 0 && files.is_empty() && self.flyway_rule.is_none();
        outcome.scope = self.scope;
        outcome.fallback_reason = self.fallback_reason.clone();
        self.apply_suppressions(&mut outcome);
        outcome
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
    let loaded = match load_project(target, &selection, config, organization) {
        Ok(loaded) => loaded,
        Err(outcome) => return *outcome,
    };
    let Loaded {
        target,
        project,
        config,
        registry,
    } = &loaded;
    let Some(root) = project.parent() else {
        return config_error(miette!("Configuration must be a file inside a project"));
    };
    let profiles = selection.names;
    let resolved_rules = match registry.resolve(config, None, profiles) {
        Ok(rules) => rules,
        Err(error) => return config_error(error),
    };
    let rules = resolved_rules.iter().collect::<Vec<_>>();
    let flyway_rule = rules.iter().copied().find(|rule| rule.id == FLYWAY_RULE);
    let features = match scope_features(registry, config, profiles, &rules) {
        Ok(features) => features,
        Err(error) => return config_error(error),
    };
    let (changes, fallback_reason) = if organization.is_none()
        && let Some(request) = changed
    {
        changes_in_scope(root, project, config, request, &features).await
    } else {
        (None, None)
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
        target
    };
    let git_outcome = if let Some(rule) = flyway_rule {
        match flyway::check(root, target, changed.and_then(|request| request.base), rule).await {
            Ok(outcome) => outcome,
            Err(error) => {
                let mut outcome = failure(
                    FailureKind::PreparationError,
                    error.wrap_err("Flyway Git comparison failed"),
                );
                outcome.failures[0].set_rules([FLYWAY_RULE]);
                outcome
            }
        }
    } else {
        Outcome::default()
    };
    let tracked = if organization.is_some() {
        BTreeSet::new()
    } else {
        match changed::tracked_sources(root).await {
            Ok(tracked) => tracked,
            Err(error) => return failure(FailureKind::PreparationError, error),
        }
    };
    let uses_build = features.kotlin
        || module.is_some()
        || !config.modules.is_empty()
        || !config.module_dependencies.forbid.is_empty()
        || rules.iter().any(|rule| rule.engine == EngineName::Archunit);
    let uses_build =
        uses_build || resolve_imports && rules.iter().any(|rule| rule.uses_pmd_classpath());
    let prepared = Prepared {
        root,
        target,
        config,
        registry,
        profiles,
        rules: &rules,
        sources: SourceFiles {
            root,
            tracked,
            required: organization.is_some(),
        },
        module,
        changes,
        scope,
        fallback_reason,
        flyway_rule,
        required: organization.is_some(),
        resolve_imports,
    };
    if uses_build {
        prepared
            .check_with_build(git_outcome, features.kotlin)
            .await
    } else {
        prepared.check_sources_only(git_outcome).await
    }
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
    if test_rules.len() < rules.len() {
        outcome.skipped_test_files.extend(tests.iter().cloned());
    }
    if !main.is_empty() || tests.is_empty() || test_rules.is_empty() {
        outcome
            .merge(run_engines(root, &main, registry, rules, config, java_source, required).await);
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
