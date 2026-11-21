use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use dornguard_config::{FLYWAY_RULE, ProjectConfig, RESOURCE_RULE, Registry, Rule};
use dornguard_core::{
    Diagnostic, EngineName, Failure, FailureKind, FailureStage, Module, Outcome, ProjectModel,
    SourceLanguage, TEMPLATE_DIR, fingerprint,
};
use dornguard_engine::{ArchModule, Engine, Pmd, Request, check_architecture};
use dornguard_gradle::CompileModules;
use miette::{Result, bail, miette};

use crate::{SourceFiles, changed::Changes, failure, run_source_engines};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BuildSystem {
    Gradle,
    Maven,
}

impl BuildSystem {
    pub fn detect(root: &Path) -> Self {
        let gradle_files = [
            "gradlew",
            "gradlew.bat",
            "settings.gradle",
            "settings.gradle.kts",
            "build.gradle",
            "build.gradle.kts",
        ];
        if root.join("pom.xml").is_file()
            && !gradle_files.iter().any(|file| root.join(file).is_file())
        {
            Self::Maven
        } else {
            Self::Gradle
        }
    }

    async fn collect(self, root: &Path, compile: CompileModules<'_>) -> Result<ProjectModel> {
        match self {
            Self::Gradle => dornguard_gradle::collect(root, compile).await,
            Self::Maven => {
                let compile = !matches!(compile, CompileModules::Selected([]));
                crate::maven::collect(root, compile).await
            }
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Gradle => "Gradle",
            Self::Maven => "Maven",
        }
    }
}

pub struct Check<'a> {
    pub build: BuildSystem,
    pub root: &'a Path,
    pub target: &'a Path,
    pub filter: Option<&'a str>,
    pub registry: &'a Registry,
    pub config: &'a ProjectConfig,
    pub profiles: &'a [String],
    pub global: &'a [&'a Rule],
    pub global_kotlin: bool,
    pub resolve_imports: bool,
    pub sources: &'a SourceFiles<'a>,
    pub changes: Option<&'a Changes>,
    pub required: bool,
    pub module_git_outcome: Outcome,
}

/// Rule lookups shared by every phase of a build-backed check.
struct Plan<'p> {
    build: BuildSystem,
    global: &'p [&'p Rule],
    overrides: BTreeMap<&'p str, Vec<&'p Rule>>,
    kotlin_modules: BTreeSet<&'p str>,
    global_kotlin: bool,
    resolve_imports: bool,
}

impl<'p> Plan<'p> {
    fn rules_for(&self, name: &str) -> &[&'p Rule] {
        self.overrides.get(name).map_or(self.global, Vec::as_slice)
    }

    fn rule_needs_compile(&self, rule: &Rule) -> bool {
        rule.engine == EngineName::Archunit || self.resolve_imports && rule.uses_pmd_classpath()
    }

    fn needs_compile(&self, rules: &[&Rule]) -> bool {
        rules.iter().any(|rule| self.rule_needs_compile(rule))
    }

    fn kotlin_for(&self, name: &str) -> bool {
        self.global_kotlin || self.kotlin_modules.contains(name)
    }

    fn format_kotlin(&self, name: &str) -> bool {
        !self.kotlin_for(name)
            && self
                .rules_for(name)
                .iter()
                .any(|rule| rule.is_format_rule())
    }

    fn kotlin_sources(&self, name: &str) -> bool {
        self.kotlin_for(name) || self.format_kotlin(name)
    }

    fn web_for(&self, name: &str, language: SourceLanguage) -> bool {
        self.rules_for(name)
            .iter()
            .any(|rule| rule.applies_to(language))
    }

    fn build_failure(
        &self,
        error: miette::Report,
        stage: FailureStage,
        modules: &[String],
    ) -> Outcome {
        let mut outcome = failure(FailureKind::PreparationError, error);
        let failure = &mut outcome.failures[0];
        failure.stage = Some(stage);
        failure.build_tool = Some(self.build.name());
        failure.modules = modules.to_vec();
        let affected_rule =
            |rule: &&Rule| stage != FailureStage::ProjectCompile || self.rule_needs_compile(rule);
        if modules.is_empty() {
            failure.set_rules(
                self.global
                    .iter()
                    .chain(self.overrides.values().flatten())
                    .copied()
                    .filter(affected_rule)
                    .map(|rule| rule.id.as_str()),
            );
        } else {
            failure.set_rules(
                modules
                    .iter()
                    .flat_map(|name| self.rules_for(name).iter())
                    .copied()
                    .filter(affected_rule)
                    .map(|rule| rule.id.as_str()),
            );
        }
        outcome
    }
}

type ResolvedRules<'c> = (BTreeMap<&'c str, Vec<Rule>>, BTreeSet<&'c str>);

/// Resolve each configured module's rules and find the modules that enable Kotlin.
fn resolve_module_rules<'c>(
    registry: &Registry,
    config: &'c ProjectConfig,
    profiles: &[String],
) -> std::result::Result<ResolvedRules<'c>, Box<Outcome>> {
    let config_error = |error| Box::new(failure(FailureKind::ConfigurationError, error));
    let mut overrides = BTreeMap::new();
    let mut kotlin_modules = BTreeSet::new();
    for name in config.modules.keys() {
        let rules = registry
            .resolve(config, Some(name), profiles)
            .map_err(config_error)?;
        let enabled = registry
            .kotlin_profile_enabled(config, Some(name), profiles)
            .map_err(config_error)?;
        if enabled || rules.iter().any(Rule::enables_kotlin) {
            kotlin_modules.insert(name.as_str());
        }
        overrides.insert(name.as_str(), rules);
    }
    Ok((overrides, kotlin_modules))
}

/// The project model and what the check must compile.
struct Acquired {
    model: ProjectModel,
    affected: Option<BTreeSet<String>>,
    compile_names: Vec<String>,
    compile_all: bool,
    compile_requested: bool,
    compile_failed: bool,
}

async fn acquire_model(
    plan: &Plan<'_>,
    root: &Path,
    filter: Option<&str>,
    changes: &mut Option<&Changes>,
) -> std::result::Result<(Acquired, Outcome), Outcome> {
    let build = plan.build;
    let (discovery, affected, fallback_reason) = if let Some(changes_found) = *changes {
        let model = match build.collect(root, CompileModules::Selected(&[])).await {
            Ok(model) => model,
            Err(error) => return Err(plan.build_failure(error, FailureStage::ProjectModel, &[])),
        };
        match affected_modules(
            root,
            &model.modules,
            changes_found,
            |name| plan.kotlin_sources(name),
            model
                .modules
                .iter()
                .any(|module| plan.kotlin_sources(&module.path)),
            |name| plan.format_kotlin(name),
            |name, language| plan.web_for(name, language),
        ) {
            Ok(affected) => (Some(model), Some(affected), None),
            Err(error) => {
                *changes = None;
                (Some(model), None, Some(error.to_string()))
            }
        }
    } else {
        (None, None, None)
    };
    let compile_names = if let Some(affected) = &affected {
        affected
            .iter()
            .filter(|name| plan.needs_compile(plan.rules_for(name)))
            .cloned()
            .collect()
    } else {
        match filter {
            Some(name) if plan.needs_compile(plan.rules_for(name)) => vec![name.to_owned()],
            Some(_) => Vec::new(),
            None => plan
                .overrides
                .iter()
                .filter(|(_, rules)| plan.needs_compile(rules))
                .map(|(&name, _)| name.to_owned())
                .collect(),
        }
    };
    let compile_all = affected.is_none() && filter.is_none() && plan.needs_compile(plan.global);
    let compile_requested = compile_all || !compile_names.is_empty();
    let mut compile_failed = false;
    let mut outcome = Outcome {
        fallback_reason,
        ..Outcome::default()
    };
    let model = match (compile_requested, discovery) {
        (false, Some(model)) => model,
        (_, discovered) => {
            let compile = if compile_all {
                CompileModules::AllJava
            } else {
                CompileModules::Selected(&compile_names)
            };
            match build.collect(root, compile).await {
                Ok(model) => model,
                Err(error) if compile_requested => {
                    compile_failed = true;
                    let requested = if compile_all {
                        &[]
                    } else {
                        compile_names.as_slice()
                    };
                    outcome.merge(plan.build_failure(
                        error,
                        FailureStage::ProjectCompile,
                        requested,
                    ));
                    if let Some(model) = discovered {
                        model
                    } else {
                        match build.collect(root, CompileModules::Selected(&[])).await {
                            Ok(model) => model,
                            Err(error) => {
                                outcome.merge(plan.build_failure(
                                    error,
                                    FailureStage::ProjectModel,
                                    requested,
                                ));
                                return Err(outcome);
                            }
                        }
                    }
                }
                Err(error) => {
                    let requested = filter.map(str::to_owned).into_iter().collect::<Vec<_>>();
                    outcome.merge(plan.build_failure(
                        error,
                        FailureStage::ProjectModel,
                        &requested,
                    ));
                    return Err(outcome);
                }
            }
        }
    };
    Ok((
        Acquired {
            model,
            affected,
            compile_names,
            compile_all,
            compile_requested,
            compile_failed,
        },
        outcome,
    ))
}

/// Everything a module needs to be turned into checks.
struct ModuleRun<'r, 'p> {
    plan: &'r Plan<'p>,
    root: &'r Path,
    target: &'r Path,
    sources: &'r SourceFiles<'r>,
    config: &'r ProjectConfig,
    changes: Option<&'r Changes>,
    acquired: &'r Acquired,
}

/// Checks queued while walking the selected modules.
#[derive(Default)]
struct Queued<'x> {
    source_groups: SourceGroups<'x>,
    pmd_modules: Vec<PmdModule<'x>>,
    architecture: Vec<ArchModule<'x>>,
    excluded_source: bool,
}

fn queue_architecture<'x>(
    run: &ModuleRun<'_, 'x>,
    module: &'x Module,
    arch_rules: Vec<&'x Rule>,
    files: &[PathBuf],
    queued: &mut Queued<'x>,
    outcome: &mut Outcome,
) -> Result<()> {
    let plan = run.plan;
    let kotlin = plan.kotlin_for(&module.path);
    let jvm = |language| {
        language == SourceLanguage::Java || (language == SourceLanguage::Kotlin && kotlin)
    };
    let build_name = plan.build.name();
    if arch_rules.is_empty()
        || !(run.acquired.affected.is_some()
            || files
                .iter()
                .any(|path| SourceLanguage::of(path).is_some_and(jvm)))
    {
        return Ok(());
    }
    let mut all_files = module_files(build_name, run.sources, run.root, module, false, jvm)?;
    // package-info.java need not emit bytecode. Keep it in source checks.
    let has_arch_sources = !all_files.is_empty();
    let has_class_sources = all_files.iter().any(|file| {
        file.file_name()
            .is_some_and(|name| name != "package-info.java")
    });
    all_files.retain(|file| {
        file.file_name()
            .is_none_or(|name| name != "package-info.java")
            || has_class_sources && module.compile_sources.contains(&run.root.join(file))
    });
    queued.excluded_source |= has_arch_sources && all_files.is_empty();
    if all_files.is_empty() {
        return Ok(());
    }
    if run.acquired.compile_failed && plan.needs_compile(plan.rules_for(&module.path)) {
        outcome.record_coverage(&all_files);
    } else if let Some(file) = all_files
        .iter()
        .find(|file| match SourceLanguage::of(file) {
            Some(SourceLanguage::Java) => !module.compile_sources.contains(&run.root.join(file)),
            Some(SourceLanguage::Kotlin) => !module.kotlin_compiled,
            _ => false,
        })
    {
        let mut failure = Failure::new(
            FailureKind::EngineError,
            "The build did not compile this source. Check the compiler's skip and exclude settings.",
        );
        failure.engine = Some(EngineName::Archunit);
        failure.stage = Some(FailureStage::Engine);
        failure.path = Some(file.to_string_lossy().replace('\\', "/"));
        failure.modules.push(module.path.clone());
        failure.file_count = Some(all_files.len());
        failure.set_rules(arch_rules.iter().map(|rule| rule.id.as_str()));
        outcome.failures.push(failure);
        outcome.incomplete_files.extend(all_files.iter().cloned());
        outcome.record_coverage(&all_files);
    } else {
        queued.architecture.push(ArchModule {
            module,
            rules: arch_rules,
            files: all_files,
            kotlin,
            // Gradle validates input content for up-to-date and cache hits.
            check_class_timestamps: plan.build == BuildSystem::Maven,
        });
    }
    Ok(())
}

/// Report Java files that `--resolve-types` cannot check without a compiled classpath.
fn report_missing_classpath(
    module: &Module,
    source_rules: &[&Rule],
    files: &[PathBuf],
    compile_failed: bool,
    outcome: &mut Outcome,
) {
    let java_files = files
        .iter()
        .filter(|path| SourceLanguage::of(path) == Some(SourceLanguage::Java));
    let count = java_files.clone().count();
    if count == 0 {
        return;
    }
    if !compile_failed {
        let mut failure = Failure::new(
            FailureKind::EngineError,
            "The compiled classpath required by --resolve-types is unavailable.",
        );
        failure.engine = Some(EngineName::Pmd);
        failure.stage = Some(FailureStage::Engine);
        failure.modules.push(module.path.clone());
        failure.file_count = Some(count);
        failure.set_rules(
            source_rules
                .iter()
                .filter(|rule| rule.uses_pmd_classpath())
                .map(|rule| rule.id.as_str()),
        );
        outcome.failures.push(failure);
    }
    outcome.record_coverage(java_files);
}

fn queue_module<'x>(
    run: &ModuleRun<'_, 'x>,
    module: &'x Module,
    queued: &mut Queued<'x>,
    outcome: &mut Outcome,
) -> Result<()> {
    let plan = run.plan;
    let config = run.config;
    let acquired = run.acquired;
    let build_name = plan.build.name();
    let rules = plan.rules_for(&module.path);
    let kotlin = plan.kotlin_for(&module.path);
    let jvm = |language| {
        language == SourceLanguage::Java || (language == SourceLanguage::Kotlin && kotlin)
    };
    let format_kotlin = plan.format_kotlin(&module.path);
    let source_jvm =
        |language| jvm(language) || (language == SourceLanguage::Kotlin && format_kotlin);
    let mut files = module_files(
        build_name,
        run.sources,
        run.target,
        module,
        format_kotlin,
        source_jvm,
    )?;
    let arch_rules = rules
        .iter()
        .copied()
        .filter(|rule| rule.engine == EngineName::Archunit)
        .collect::<Vec<_>>();
    queue_architecture(run, module, arch_rules, &files, queued, outcome)?;
    files.extend(module_files(
        build_name,
        run.sources,
        run.target,
        module,
        false,
        |language| {
            !matches!(language, SourceLanguage::Java | SourceLanguage::Kotlin)
                && plan.web_for(&module.path, language)
        },
    )?);
    if let Some(changes) = run.changes {
        files.retain(|file| changes.matches(file));
    }
    let mut source_rules = rules
        .iter()
        .copied()
        .filter(|rule| {
            matches!(
                rule.engine,
                EngineName::Semgrep | EngineName::Pmd | EngineName::Eslint | EngineName::Builtin
            )
        })
        .collect::<Vec<_>>();
    queued.excluded_source |= files.iter().any(|file| config.excludes_source(file));
    files.retain(|file| {
        !config.excludes_source(file)
            && SourceLanguage::of(file)
                .is_some_and(|language| source_rules.iter().any(|rule| rule.applies_to(language)))
    });
    let test_rules = source_rules
        .iter()
        .copied()
        .filter(|rule| rule.applies_to_java_tests())
        .collect::<Vec<_>>();
    let skips_tests = source_rules.iter().any(|rule| rule.skips_tests());
    if !test_rules.is_empty() || skips_tests {
        let tests = module_test_sources(
            build_name,
            run.sources,
            run.target,
            module,
            config,
            run.changes,
            &files,
            &mut queued.excluded_source,
        )?;
        if skips_tests {
            outcome.skipped_test_files.extend(tests.iter().cloned());
        }
        if !test_rules.is_empty() {
            add_source_group(
                &mut queued.source_groups,
                test_rules,
                tests,
                module.java_test_source.or(module.java_source),
            );
        }
    }
    let uses_classpath = source_rules.iter().any(|rule| rule.uses_pmd_classpath());
    let maven_compiled =
        plan.build == BuildSystem::Maven && acquired.compile_requested && !acquired.compile_failed;
    let classpath = (!acquired.compile_failed
        && uses_classpath
        && (maven_compiled
            || acquired.compile_all
            || acquired.compile_names.contains(&module.path)))
    .then(|| compiled_classpath(module))
    .flatten();
    if plan.resolve_imports && classpath.is_none() && uses_classpath {
        report_missing_classpath(
            module,
            &source_rules,
            &files,
            acquired.compile_failed,
            outcome,
        );
    }
    let java_target = config.java_target.or(module.java_target);
    let needs_target = config.java_target.is_none()
        && module.java_target.is_some_and(|target| target < 21)
        && source_rules.iter().any(|rule| rule.id == RESOURCE_RULE);
    if classpath.is_some() || needs_target {
        let java_files = files
            .iter()
            .filter(|path| SourceLanguage::of(path) == Some(SourceLanguage::Java))
            .cloned()
            .collect::<Vec<_>>();
        if !java_files.is_empty() {
            let pmd_rules = source_rules
                .extract_if(.., |rule| rule.engine == EngineName::Pmd)
                .collect::<Vec<_>>();
            queued
                .pmd_modules
                .push((module, java_files, pmd_rules, classpath, java_target));
        }
    }
    add_source_group(
        &mut queued.source_groups,
        source_rules,
        files,
        module.java_source,
    );
    Ok(())
}

/// Queue sources that sit outside every module's source directories.
fn queue_unmapped_sources<'x>(run: &ModuleRun<'_, 'x>, queued: &mut Queued<'x>) -> Result<()> {
    let plan = run.plan;
    let model = &run.acquired.model;
    let include_unmapped = |language| {
        // Kotlin checks depend on the source sets registered by the JVM plugin.
        (language != SourceLanguage::Kotlin
            || !plan.global_kotlin && plan.kotlin_modules.is_empty())
            && plan.global.iter().any(|rule| rule.applies_to(language))
    };
    if !SOURCE_LANGUAGES.into_iter().any(include_unmapped) {
        return Ok(());
    }
    let roots = SOURCE_LANGUAGES
        .into_iter()
        .filter(|&language| include_unmapped(language))
        .map(|language| {
            let paths = model
                .modules
                .iter()
                .flat_map(|module| {
                    source_dirs(module, language, plan.format_kotlin(&module.path))
                        .into_owned()
                        .into_iter()
                        .chain(
                            module
                                .test_source_dirs
                                .iter()
                                .filter(|_| language == SourceLanguage::Java)
                                .cloned(),
                        )
                })
                .collect::<BTreeSet<_>>();
            (language, paths)
        })
        .collect::<BTreeMap<_, _>>();
    let files = run.sources.collect(run.target, include_unmapped)?;
    let excluded_source = &mut queued.excluded_source;
    let files = files
        .into_iter()
        .filter(|path| {
            run.changes.is_none_or(|changes| changes.matches(path))
                && SourceLanguage::of(path)
                    .and_then(|language| roots.get(&language))
                    .is_none_or(|directories| {
                        let full_path = run.root.join(path);
                        !directories
                            .iter()
                            .any(|directory| full_path.starts_with(directory))
                    })
        })
        .filter(|path| {
            let excluded = run.config.excludes_source(path);
            *excluded_source |= excluded;
            !excluded
        });
    add_source_group(&mut queued.source_groups, plan.global.to_vec(), files, None);
    Ok(())
}

pub async fn check(check: Check<'_>) -> Outcome {
    let Check {
        build,
        root,
        target,
        filter,
        registry,
        config,
        profiles,
        global,
        global_kotlin,
        resolve_imports,
        sources,
        mut changes,
        required,
        mut module_git_outcome,
    } = check;
    let config_error = |error| failure(FailureKind::ConfigurationError, error);
    if !target.starts_with(root) {
        return failure(
            FailureKind::PreparationError,
            miette!("The check target must be inside the configuration directory"),
        );
    }
    let (resolved, kotlin_modules) = match resolve_module_rules(registry, config, profiles) {
        Ok(resolved) => resolved,
        Err(outcome) => return *outcome,
    };
    if build == BuildSystem::Maven {
        if global_kotlin || !kotlin_modules.is_empty() {
            return config_error(miette!("Kotlin build integration requires Gradle"));
        }
        if !config.module_dependencies.forbid.is_empty() {
            return config_error(miette!("module_dependencies.forbid requires Gradle"));
        }
    }
    let plan = Plan {
        build,
        global,
        overrides: resolved
            .iter()
            .map(|(&name, rules)| (name, rules.iter().collect::<Vec<_>>()))
            .collect(),
        kotlin_modules,
        global_kotlin,
        resolve_imports,
    };
    let (acquired, mut outcome) = match acquire_model(&plan, root, filter, &mut changes).await {
        Ok(acquired) => acquired,
        Err(outcome) => return outcome,
    };
    let model = &acquired.model;
    let build_name = build.name();
    let known = model
        .modules
        .iter()
        .map(|module| module.path.as_str())
        .collect::<BTreeSet<_>>();
    let dependency_paths = config
        .module_dependencies
        .forbid
        .iter()
        .flat_map(|dependency| [dependency.from.as_str(), dependency.to.as_str()]);
    for name in plan
        .overrides
        .keys()
        .copied()
        .chain(filter)
        .chain(dependency_paths)
    {
        if !known.contains(name) {
            return config_error(miette!("Unknown {build_name} module: {name}"));
        }
    }

    let selected = model
        .modules
        .iter()
        .filter(|module| {
            filter.is_none_or(|filter| filter == module.path)
                && acquired
                    .affected
                    .as_ref()
                    .is_none_or(|affected| affected.contains(&module.path))
                && (target == root
                    || target.starts_with(&module.project_dir)
                    || module.project_dir.starts_with(target)
                    || module
                        .source_dirs
                        .iter()
                        .chain(&module.test_source_dirs)
                        .any(|dir| target.starts_with(dir) || dir.starts_with(target))
                    || plan.kotlin_sources(&module.path)
                        && source_dirs(
                            module,
                            SourceLanguage::Kotlin,
                            plan.format_kotlin(&module.path),
                        )
                        .iter()
                        .any(|dir| target.starts_with(dir) || dir.starts_with(target)))
        })
        .collect::<Vec<_>>();
    let selected_names = selected
        .iter()
        .map(|module| module.path.as_str())
        .collect::<BTreeSet<_>>();
    if filter.is_some() {
        let selected_path = |path: &Path| {
            let absolute = root.join(path);
            model
                .modules
                .iter()
                .filter(|module| absolute.starts_with(&module.project_dir))
                .max_by_key(|module| module.project_dir.components().count())
                .is_some_and(|module| selected_names.contains(module.path.as_str()))
        };
        module_git_outcome.diagnostics.retain(|item| {
            item.path
                .as_deref()
                .is_some_and(|path| selected_path(Path::new(path)))
        });
        module_git_outcome
            .scanned_files
            .retain(|path| selected_path(path));
        outcome.merge(module_git_outcome);
    }
    let run = ModuleRun {
        plan: &plan,
        root,
        target,
        sources,
        config,
        changes,
        acquired: &acquired,
    };
    let mut queued = Queued::default();
    for module in selected {
        if let Err(error) = queue_module(&run, module, &mut queued, &mut outcome) {
            return failure(FailureKind::PreparationError, error);
        }
    }
    if filter.is_none()
        && let Err(error) = queue_unmapped_sources(&run, &mut queued)
    {
        return failure(FailureKind::PreparationError, error);
    }
    let Queued {
        source_groups,
        pmd_modules,
        architecture,
        excluded_source,
    } = queued;

    let dependency_checks = config
        .module_dependencies
        .forbid
        .iter()
        .filter(|dependency| selected_names.contains(dependency.from.as_str()))
        .collect::<Vec<_>>();
    if source_groups.is_empty()
        && pmd_modules.is_empty()
        && architecture.is_empty()
        && dependency_checks.is_empty()
    {
        if !outcome.failures.is_empty() {
            return outcome;
        }
        if changes.is_some() || excluded_source {
            outcome.no_changes = true;
            return outcome;
        }
        if global.iter().any(|rule| rule.id == FLYWAY_RULE) {
            return outcome;
        }
        outcome.merge(failure(
            FailureKind::PreparationError,
            miette!("No {build_name} module targets apply to this check"),
        ));
        return outcome;
    }
    if !dependency_checks.is_empty() {
        let rule = match registry.rule("architecture.module-dependency") {
            Ok(rule) => rule,
            Err(error) => return config_error(error),
        };
        outcome.diagnostics.extend(module_dependency_diagnostics(
            root,
            model,
            rule,
            &dependency_checks,
        ));
    }

    let source_checks = run_source_groups(root, registry, config, required, source_groups);
    let pmd_checks = run_pmd_modules(root, registry, config, required, pmd_modules);
    let (source_result, architecture_result, pmd_result) = tokio::join!(
        source_checks,
        check_architecture(root, &architecture),
        pmd_checks
    );
    outcome.merge(source_result);
    outcome.merge(architecture_result);
    outcome.merge(pmd_result);
    outcome
}

fn module_dependency_diagnostics(
    root: &Path,
    model: &ProjectModel,
    rule: &Rule,
    dependency_checks: &[&dornguard_config::Dependency],
) -> Vec<Diagnostic> {
    let build_file = if root.join("build.gradle.kts").is_file() {
        "build.gradle.kts"
    } else {
        "build.gradle"
    };
    dependency_checks
        .iter()
        .filter(|dependency| {
            model.modules.iter().any(|module| {
                module.path == dependency.from && module.dependencies.contains(&dependency.to)
            })
        })
        .map(|dependency| Diagnostic {
            path: Some(build_file.into()),
            module: None,
            class_name: None,
            line: None,
            column: None,
            end_line: None,
            end_column: None,
            subject: None,
            rule_id: rule.id.clone(),
            engine: EngineName::Gradle,
            severity: rule.severity,
            confidence: rule.confidence,
            message: format!(
                "{} {} → {}.",
                rule.description, dependency.from, dependency.to
            ),
            fix: rule.fix.clone(),
            edit: None,
            legacy_fingerprints: Vec::new(),
            fingerprint: Some(fingerprint(&format!(
                "{}:{}",
                dependency.from, dependency.to
            ))),
        })
        .collect()
}

async fn run_source_groups(
    root: &Path,
    registry: &Registry,
    config: &ProjectConfig,
    required: bool,
    source_groups: SourceGroups<'_>,
) -> Outcome {
    let mut outcome = Outcome::default();
    for ((_, java_source), (rules, files)) in source_groups {
        outcome.merge(
            run_source_engines(
                root,
                &files.into_iter().collect::<Vec<_>>(),
                registry,
                &rules,
                config,
                java_source,
                required,
            )
            .await,
        );
    }
    outcome
}

type PmdModule<'a> = (
    &'a Module,
    Vec<PathBuf>,
    Vec<&'a Rule>,
    Option<Vec<PathBuf>>,
    Option<u16>,
);

async fn run_pmd_modules(
    root: &Path,
    registry: &Registry,
    config: &ProjectConfig,
    required: bool,
    pmd_modules: Vec<PmdModule<'_>>,
) -> Outcome {
    let mut outcome = Outcome::default();
    for (module, files, rules, classpath, java_target) in pmd_modules {
        let mut result = Pmd {
            classpath: classpath.as_deref(),
            compile_sources: Some(&module.compile_sources),
            java_target,
            java_source: config.java_source.or(module.java_source),
            allow_logged_exceptions: config.allow_logged_exceptions,
            resource_owners: &config.java_resource_owners,
            managed_resource_factories: &config.java_managed_resource_factories,
        }
        .check(Request {
            root,
            files: &files,
            rules: &rules,
            registry,
            required,
        })
        .await;
        for failure in &mut result.failures {
            failure.modules.push(module.path.clone());
        }
        outcome.merge(result);
    }
    outcome
}

type SourceGroups<'a> = BTreeMap<
    (Vec<(&'a str, dornguard_core::Severity)>, Option<u16>),
    (Vec<&'a Rule>, BTreeSet<PathBuf>),
>;

fn add_source_group<'a>(
    groups: &mut SourceGroups<'a>,
    rules: Vec<&'a Rule>,
    files: impl IntoIterator<Item = PathBuf>,
    java_source: Option<u16>,
) {
    let mut files = files.into_iter().peekable();
    if rules.is_empty() || files.peek().is_none() {
        return;
    }
    let ids = rules
        .iter()
        .map(|rule| (rule.id.as_str(), rule.severity))
        .collect();
    let java_source =
        java_source.filter(|_| rules.iter().any(|rule| rule.engine == EngineName::Pmd));
    groups
        .entry((ids, java_source))
        .or_insert_with(|| (rules, BTreeSet::new()))
        .1
        .extend(files);
}

fn compiled_classpath(module: &Module) -> Option<Vec<PathBuf>> {
    if module.compile_sources.is_empty() {
        return None;
    }
    let output = module.class_dirs.iter().filter(|path| path.is_dir());
    let mut classpath = output.cloned().collect::<Vec<_>>();
    if classpath.is_empty()
        || module
            .classpath
            .iter()
            .any(|entry| !(entry.is_file() || entry.is_dir()))
    {
        return None;
    }
    let mut seen = classpath.iter().cloned().collect::<BTreeSet<_>>();
    classpath.extend(
        module
            .classpath
            .iter()
            .filter(|path| seen.insert((*path).clone()))
            .cloned(),
    );
    Some(classpath)
}

fn affected_modules(
    root: &Path,
    modules: &[Module],
    changes: &Changes,
    kotlin_for: impl Fn(&str) -> bool,
    any_kotlin: bool,
    format_kotlin: impl Fn(&str) -> bool,
    web_for: impl Fn(&str, SourceLanguage) -> bool,
) -> Result<BTreeSet<String>> {
    let mut affected = BTreeSet::new();
    for path in changes.paths() {
        let Some(language) = SourceLanguage::of(path) else {
            continue;
        };
        if language == SourceLanguage::Kotlin && !any_kotlin {
            continue;
        }
        let full = root.join(path);
        let mut matched = false;
        for module in modules.iter().filter(|module| {
            source_dirs(module, language, format_kotlin(&module.path))
                .iter()
                .chain(
                    module
                        .test_source_dirs
                        .iter()
                        .filter(|_| language == SourceLanguage::Java),
                )
                .any(|dir| full.starts_with(dir))
        }) {
            matched = true;
            let enabled = match language {
                SourceLanguage::Java => true,
                SourceLanguage::Kotlin => kotlin_for(&module.path),
                other => web_for(&module.path, other),
            };
            if enabled {
                affected.insert(module.path.clone());
            }
        }
        if !matched && matches!(language, SourceLanguage::Java | SourceLanguage::Kotlin) {
            bail!(
                "Changed source path does not map to a Gradle module: {}",
                path.display()
            );
        }
    }
    let mut pending = affected.iter().cloned().collect::<Vec<_>>();
    while let Some(changed) = pending.pop() {
        for module in modules
            .iter()
            .filter(|module| module.dependencies.contains(&changed))
        {
            if affected.insert(module.path.clone()) {
                pending.push(module.path.clone());
            }
        }
    }
    Ok(affected)
}

const SOURCE_LANGUAGES: [SourceLanguage; 6] = [
    SourceLanguage::Java,
    SourceLanguage::Kotlin,
    SourceLanguage::Html,
    SourceLanguage::Template,
    SourceLanguage::Javascript,
    SourceLanguage::Jsp,
];

fn module_files(
    build_name: &str,
    sources: &SourceFiles<'_>,
    target: &Path,
    module: &Module,
    format_kotlin: bool,
    accepts: impl Fn(SourceLanguage) -> bool,
) -> Result<Vec<PathBuf>> {
    let mut files = BTreeSet::new();
    for language in SOURCE_LANGUAGES
        .into_iter()
        .filter(|&language| accepts(language))
    {
        let dirs = source_dirs(module, language, format_kotlin);
        collect_source_dirs(build_name, sources, target, language, &dirs, &mut files)?;
    }
    Ok(files.into_iter().collect())
}

#[allow(clippy::too_many_arguments)]
fn module_test_sources(
    build_name: &str,
    sources: &SourceFiles<'_>,
    target: &Path,
    module: &Module,
    config: &ProjectConfig,
    changes: Option<&Changes>,
    main_files: &[PathBuf],
    excluded_source: &mut bool,
) -> Result<BTreeSet<PathBuf>> {
    let mut tests = BTreeSet::new();
    collect_source_dirs(
        build_name,
        sources,
        target,
        SourceLanguage::Java,
        &module.test_source_dirs,
        &mut tests,
    )?;
    *excluded_source |= tests.iter().any(|file| config.excludes_source(file));
    tests.retain(|file| {
        !config.excludes_source(file) && changes.is_none_or(|changes| changes.matches(file))
    });
    for file in main_files {
        tests.remove(file);
    }
    Ok(tests)
}

fn collect_source_dirs(
    build_name: &str,
    sources: &SourceFiles<'_>,
    target: &Path,
    language: SourceLanguage,
    dirs: &[PathBuf],
    files: &mut BTreeSet<PathBuf>,
) -> Result<()> {
    for dir in dirs {
        if !dir.starts_with(sources.root) {
            bail!(
                "{build_name} source directory is outside the project: {}",
                dir.display()
            );
        }
        if !dir.is_dir() {
            continue;
        }
        if target.starts_with(dir) {
            files.extend(sources.collect(target, |file| file == language)?);
        } else if dir.starts_with(target) {
            files.extend(sources.collect(dir, |file| file == language)?);
        }
    }
    Ok(())
}

fn source_dirs(
    module: &Module,
    language: SourceLanguage,
    format_kotlin: bool,
) -> Cow<'_, [PathBuf]> {
    match language {
        SourceLanguage::Java => Cow::Borrowed(&module.source_dirs),
        SourceLanguage::Kotlin if format_kotlin => Cow::Owned(
            module
                .kotlin_source_dirs
                .iter()
                .chain(&module.source_dirs)
                .cloned()
                .chain([module.project_dir.join("src/main/kotlin")])
                .collect(),
        ),
        SourceLanguage::Kotlin => Cow::Borrowed(&module.kotlin_source_dirs),
        SourceLanguage::Template => Cow::Owned(vec![module.project_dir.join(TEMPLATE_DIR)]),
        SourceLanguage::Jsp => Cow::Owned(vec![module.project_dir.join("src/main/webapp")]),
        SourceLanguage::Html | SourceLanguage::Javascript => {
            let mut dirs = vec![
                "src/main/resources/static",
                "src/main/resources/public",
                "src/main/webapp",
            ];
            if language == SourceLanguage::Javascript {
                dirs.extend([TEMPLATE_DIR, "src/main/js", "src/main/frontend"]);
            }
            Cow::Owned(
                dirs.into_iter()
                    .map(|dir| module.project_dir.join(dir))
                    .collect(),
            )
        }
    }
}
