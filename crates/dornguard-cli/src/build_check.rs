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
    let preparation_error = |error| failure(FailureKind::PreparationError, error);
    if !target.starts_with(root) {
        return preparation_error(miette!(
            "The check target must be inside the configuration directory"
        ));
    }
    let mut overrides = BTreeMap::new();
    let mut kotlin_modules = BTreeSet::new();
    for name in config.modules.keys() {
        match registry.resolve(config, Some(name), profiles) {
            Ok(rules) => {
                match registry.kotlin_profile_enabled(config, Some(name), profiles) {
                    Ok(enabled) => {
                        if enabled || rules.iter().any(Rule::enables_kotlin) {
                            kotlin_modules.insert(name.as_str());
                        }
                    }
                    Err(error) => return config_error(error),
                }
                overrides.insert(name.as_str(), rules);
            }
            Err(error) => return config_error(error),
        }
    }

    if build == BuildSystem::Maven {
        if global_kotlin || !kotlin_modules.is_empty() {
            return config_error(miette!("Kotlin build integration requires Gradle"));
        }
        if !config.module_dependencies.forbid.is_empty() {
            return config_error(miette!("module_dependencies.forbid requires Gradle"));
        }
    }
    let overrides = overrides
        .iter()
        .map(|(&name, rules)| (name, rules.iter().collect::<Vec<_>>()))
        .collect::<BTreeMap<_, _>>();
    let rules_for = |name: &str| overrides.get(name).map_or(global, Vec::as_slice);
    let rule_needs_compile = |rule: &Rule| {
        rule.engine == EngineName::Archunit || resolve_imports && rule.uses_pmd_classpath()
    };
    let build_failure = |error, stage, modules: &[String]| {
        let mut outcome = preparation_error(error);
        let failure = &mut outcome.failures[0];
        failure.stage = Some(stage);
        failure.build_tool = Some(build.name());
        failure.modules = modules.to_vec();
        let affected_rule =
            |rule: &&Rule| stage != FailureStage::ProjectCompile || rule_needs_compile(rule);
        if modules.is_empty() {
            failure.set_rules(
                global
                    .iter()
                    .chain(overrides.values().flatten())
                    .copied()
                    .filter(affected_rule)
                    .map(|rule| rule.id.as_str()),
            );
        } else {
            failure.set_rules(
                modules
                    .iter()
                    .flat_map(|name| rules_for(name).iter())
                    .copied()
                    .filter(affected_rule)
                    .map(|rule| rule.id.as_str()),
            );
        }
        outcome
    };
    let kotlin_for = |name: &str| global_kotlin || kotlin_modules.contains(name);
    let format_kotlin =
        |name: &str| !kotlin_for(name) && rules_for(name).iter().any(|rule| rule.is_format_rule());
    let kotlin_sources = |name: &str| kotlin_for(name) || format_kotlin(name);
    let web_for = |name: &str, language: SourceLanguage| {
        rules_for(name).iter().any(|rule| rule.applies_to(language))
    };
    let needs_compile = |rules: &[&Rule]| rules.iter().copied().any(rule_needs_compile);
    let (discovery, affected, fallback_reason) = if let Some(changes_found) = changes {
        let model = match build.collect(root, CompileModules::Selected(&[])).await {
            Ok(model) => model,
            Err(error) => return build_failure(error, FailureStage::ProjectModel, &[]),
        };
        match affected_modules(
            root,
            &model.modules,
            changes_found,
            kotlin_sources,
            model
                .modules
                .iter()
                .any(|module| kotlin_sources(&module.path)),
            format_kotlin,
            web_for,
        ) {
            Ok(affected) => (Some(model), Some(affected), None),
            Err(error) => {
                changes = None;
                (Some(model), None, Some(error.to_string()))
            }
        }
    } else {
        (None, None, None)
    };
    let compile_names = if let Some(affected) = &affected {
        affected
            .iter()
            .filter(|name| needs_compile(rules_for(name)))
            .cloned()
            .collect()
    } else {
        match filter {
            Some(name) if needs_compile(rules_for(name)) => vec![name.to_owned()],
            Some(_) => Vec::new(),
            None => overrides
                .iter()
                .filter(|(_, rules)| needs_compile(rules))
                .map(|(&name, _)| name.to_owned())
                .collect(),
        }
    };
    let compile_all = affected.is_none() && filter.is_none() && needs_compile(global);
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
                    outcome.merge(build_failure(
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
                                outcome.merge(build_failure(
                                    error,
                                    FailureStage::ProjectModel,
                                    requested,
                                ));
                                return outcome;
                            }
                        }
                    }
                }
                Err(error) => {
                    let requested = filter.map(str::to_owned).into_iter().collect::<Vec<_>>();
                    outcome.merge(build_failure(error, FailureStage::ProjectModel, &requested));
                    return outcome;
                }
            }
        }
    };
    let maven_compiled = build == BuildSystem::Maven && compile_requested && !compile_failed;

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
    for name in overrides
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
                && affected
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
                    || kotlin_sources(&module.path)
                        && source_dirs(module, SourceLanguage::Kotlin, format_kotlin(&module.path))
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
    let mut source_groups = BTreeMap::new();
    let mut pmd_modules = Vec::new();
    let mut excluded_source = false;
    let mut architecture = Vec::new();
    for module in selected {
        let rules = rules_for(&module.path);
        let kotlin = kotlin_for(&module.path);
        let jvm = |language| {
            language == SourceLanguage::Java || (language == SourceLanguage::Kotlin && kotlin)
        };
        let format_kotlin = format_kotlin(&module.path);
        let source_jvm =
            |language| jvm(language) || (language == SourceLanguage::Kotlin && format_kotlin);
        let mut files = match module_files(
            build_name,
            sources,
            target,
            module,
            format_kotlin,
            source_jvm,
        ) {
            Ok(files) => files,
            Err(error) => return preparation_error(error),
        };
        let arch_rules = rules
            .iter()
            .copied()
            .filter(|rule| rule.engine == EngineName::Archunit)
            .collect::<Vec<_>>();
        if !arch_rules.is_empty()
            && (affected.is_some()
                || files
                    .iter()
                    .any(|path| SourceLanguage::of(path).is_some_and(jvm)))
        {
            let mut all_files = match module_files(build_name, sources, root, module, false, jvm) {
                Ok(files) => files,
                Err(error) => return preparation_error(error),
            };
            // package-info.java need not emit bytecode. Keep it in source checks.
            let has_arch_sources = !all_files.is_empty();
            let has_class_sources = all_files.iter().any(|file| {
                file.file_name()
                    .is_some_and(|name| name != "package-info.java")
            });
            all_files.retain(|file| {
                file.file_name()
                    .is_none_or(|name| name != "package-info.java")
                    || has_class_sources && module.compile_sources.contains(&root.join(file))
            });
            excluded_source |= has_arch_sources && all_files.is_empty();
            if !all_files.is_empty() {
                if compile_failed && needs_compile(rules) {
                    outcome.record_coverage(&all_files);
                } else if let Some(file) =
                    all_files
                        .iter()
                        .find(|file| match SourceLanguage::of(file) {
                            Some(SourceLanguage::Java) => {
                                !module.compile_sources.contains(&root.join(file))
                            }
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
                    architecture.push(ArchModule {
                        module,
                        rules: arch_rules,
                        files: all_files,
                        kotlin,
                        // Gradle validates input content for up-to-date and cache hits.
                        check_class_timestamps: build == BuildSystem::Maven,
                    });
                }
            }
        }
        match module_files(build_name, sources, target, module, false, |language| {
            !matches!(language, SourceLanguage::Java | SourceLanguage::Kotlin)
                && web_for(&module.path, language)
        }) {
            Ok(web_files) => files.extend(web_files),
            Err(error) => return preparation_error(error),
        }
        if let Some(changes) = changes {
            files.retain(|file| changes.matches(file));
        }
        let mut source_rules = rules
            .iter()
            .copied()
            .filter(|rule| {
                matches!(
                    rule.engine,
                    EngineName::Semgrep
                        | EngineName::Pmd
                        | EngineName::Eslint
                        | EngineName::Builtin
                )
            })
            .collect::<Vec<_>>();
        excluded_source |= files.iter().any(|file| config.excludes_source(file));
        files.retain(|file| {
            !config.excludes_source(file)
                && SourceLanguage::of(file).is_some_and(|language| {
                    source_rules.iter().any(|rule| rule.applies_to(language))
                })
        });
        let test_rules = source_rules
            .iter()
            .copied()
            .filter(|rule| rule.applies_to_java_tests())
            .collect::<Vec<_>>();
        if !test_rules.is_empty() || source_rules.iter().any(|rule| rule.skips_tests()) {
            let tests = match module_test_sources(
                build_name,
                sources,
                target,
                module,
                config,
                changes,
                &files,
                &mut excluded_source,
            ) {
                Ok(tests) => tests,
                Err(error) => return preparation_error(error),
            };
            if source_rules.iter().any(|rule| rule.skips_tests()) {
                outcome.skipped_test_files.extend(tests.iter().cloned());
            }
            if !test_rules.is_empty() {
                add_source_group(
                    &mut source_groups,
                    test_rules,
                    tests,
                    module.java_test_source.or(module.java_source),
                );
            }
        }
        let uses_classpath = source_rules.iter().any(|rule| rule.uses_pmd_classpath());
        let classpath = (!compile_failed
            && uses_classpath
            && (maven_compiled || compile_all || compile_names.contains(&module.path)))
        .then(|| compiled_classpath(module))
        .flatten();
        if resolve_imports && classpath.is_none() && uses_classpath {
            let java_files = files
                .iter()
                .filter(|path| SourceLanguage::of(path) == Some(SourceLanguage::Java));
            let count = java_files.clone().count();
            if count > 0 {
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
                pmd_modules.push((module, java_files, pmd_rules, classpath, java_target));
            }
        }
        add_source_group(&mut source_groups, source_rules, files, module.java_source);
    }
    let include_unmapped = |language| {
        // Kotlin checks depend on the source sets registered by the JVM plugin.
        (language != SourceLanguage::Kotlin || !global_kotlin && kotlin_modules.is_empty())
            && global.iter().any(|rule| rule.applies_to(language))
    };
    if filter.is_none() && SOURCE_LANGUAGES.into_iter().any(include_unmapped) {
        let roots = SOURCE_LANGUAGES
            .into_iter()
            .filter(|&language| include_unmapped(language))
            .map(|language| {
                let paths = model
                    .modules
                    .iter()
                    .flat_map(|module| {
                        source_dirs(module, language, format_kotlin(&module.path))
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
        let files = match sources.collect(target, include_unmapped) {
            Ok(files) => files,
            Err(error) => return preparation_error(error),
        };
        let files = files
            .into_iter()
            .filter(|path| {
                changes.is_none_or(|changes| changes.matches(path))
                    && SourceLanguage::of(path)
                        .and_then(|language| roots.get(&language))
                        .is_none_or(|directories| {
                            let full_path = root.join(path);
                            !directories
                                .iter()
                                .any(|directory| full_path.starts_with(directory))
                        })
            })
            .filter(|path| {
                let excluded = config.excludes_source(path);
                excluded_source |= excluded;
                !excluded
            });
        add_source_group(&mut source_groups, global.to_vec(), files, None);
    }

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
        outcome.merge(preparation_error(miette!(
            "No {build_name} module targets apply to this check"
        )));
        return outcome;
    }
    if !dependency_checks.is_empty() {
        let rule = match registry.rule("architecture.module-dependency") {
            Ok(rule) => rule,
            Err(error) => return config_error(error),
        };
        outcome.diagnostics.extend(module_dependency_diagnostics(
            root,
            &model,
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
