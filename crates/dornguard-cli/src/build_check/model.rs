//! Loading the build model and choosing which modules to compile.

use std::collections::BTreeSet;
use std::path::Path;

use dornguard_core::{FailureStage, Module, Outcome, ProjectModel, SourceLanguage};
use dornguard_gradle::CompileModules;
use miette::{Result, bail};

use super::Plan;
use super::sources::source_dirs;
use crate::changed::Changes;

/// The project model and what the check must compile.
pub(super) struct Acquired {
    pub(super) model: ProjectModel,
    pub(super) affected: Option<BTreeSet<String>>,
    pub(super) compile_names: Vec<String>,
    pub(super) compile_all: bool,
    pub(super) compile_requested: bool,
    pub(super) compile_failed: bool,
}

pub(super) async fn acquire_model(
    plan: &Plan<'_>,
    root: &Path,
    filter: Option<&str>,
    changes: &mut Option<&Changes>,
) -> std::result::Result<(Acquired, Outcome), Box<Outcome>> {
    let build = plan.build;
    let (discovery, affected, fallback_reason) = if let Some(changes_found) = *changes {
        let model = match build.collect(root, CompileModules::Selected(&[])).await {
            Ok(model) => model,
            Err(error) => {
                return Err(Box::new(plan.build_failure(
                    error,
                    FailureStage::ProjectModel,
                    &[],
                )));
            }
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
                                return Err(Box::new(outcome));
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
                    return Err(Box::new(outcome));
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
