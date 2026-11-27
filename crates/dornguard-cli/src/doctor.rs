use crate::build_check;
use crate::check::discover_config;
use std::collections::BTreeSet;
use std::path::Path;

use dornguard_config::Registry;
use dornguard_core::EngineName;
use dornguard_engine::{inspect, inspect_archunit, maven_model_path, supported_version};
use miette::{IntoDiagnostic, Result};

pub(crate) async fn doctor(profiles: &[String], config: Option<&Path>) -> Result<u8> {
    let cwd = dunce::canonicalize(".").into_diagnostic()?;
    let config = discover_config(&cwd, config)?;
    let root = config.as_deref().and_then(Path::parent).unwrap_or(&cwd);
    let mut engines = BTreeSet::new();
    let mut needs_build_model = false;
    if let Some(path) = &config {
        let config = Registry::load(path)?;
        let registry = Registry::for_config(path, &config)?;
        for module in
            std::iter::once(None).chain(config.modules.keys().map(|name| Some(name.as_str())))
        {
            engines.extend(
                registry
                    .resolve(&config, module, profiles)?
                    .into_iter()
                    .map(|rule| rule.engine),
            );
        }
        needs_build_model = !config.modules.is_empty() || engines.contains(&EngineName::Archunit);
    } else if profiles.is_empty() {
        engines.extend([EngineName::Semgrep, EngineName::Pmd]);
        if std::env::var_os("DORNGUARD_ARCHUNIT_RUNNER").is_some() {
            engines.insert(EngineName::Archunit);
        }
    } else {
        let config = Registry::parse("version: 1\n")?;
        engines.extend(
            Registry::builtin()?
                .resolve(&config, None, profiles)?
                .into_iter()
                .map(|rule| rule.engine),
        );
        needs_build_model = engines.contains(&EngineName::Archunit);
    }

    let mut exit_code = 0;
    for engine in engines.iter().copied().filter(|engine| {
        matches!(
            engine,
            EngineName::Semgrep
                | EngineName::Pmd
                | EngineName::Eslint
                | EngineName::GoVet
                | EngineName::Clippy
        )
    }) {
        match inspect(engine, root).await {
            Ok(()) => println!("{} {} OK", engine.executable(), supported_version(engine)),
            Err(error) => {
                eprintln!("{}: {error:#}", engine.executable());
                exit_code = 3;
            }
        }
    }
    if engines.contains(&EngineName::Archunit) {
        match inspect_archunit(root).await {
            Ok(details) => println!("ArchUnit runner OK ({details})"),
            Err(error) => {
                eprintln!("ArchUnit runner: {error:#}");
                exit_code = 3;
            }
        }
    } else if config.is_none() && profiles.is_empty() {
        // Profiles that need the runner are chosen later, so report the bundled
        // components now without failing the default checks.
        match inspect_archunit(root).await {
            Ok(details) => println!("ArchUnit runner OK ({details})"),
            Err(error) => eprintln!("Optional ArchUnit runner: {error:#}"),
        }
        match maven_model_path() {
            Ok(path) => println!("Maven model extension OK ({})", path.display()),
            Err(error) => eprintln!("Optional Maven model extension: {error:#}"),
        }
    }
    if needs_build_model
        && build_check::BuildSystem::detect(root) == build_check::BuildSystem::Maven
    {
        match maven_model_path() {
            Ok(path) => println!("Maven model extension OK ({})", path.display()),
            Err(error) => {
                eprintln!("{error:#}");
                exit_code = 3;
            }
        }
    }
    if engines.iter().all(|engine| *engine == EngineName::Builtin) && !needs_build_model {
        println!("No external engines required by the active rules.");
    }
    Ok(exit_code)
}
