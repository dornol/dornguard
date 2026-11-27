//! Which source files of a build module each language checks.

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use dornguard_config::ProjectConfig;
use dornguard_core::{Module, SourceLanguage, TEMPLATE_DIR};
use miette::{Result, bail};

use crate::{SourceFiles, changed::Changes};

pub(super) const SOURCE_LANGUAGES: [SourceLanguage; 7] = [
    SourceLanguage::Java,
    SourceLanguage::Kotlin,
    SourceLanguage::Html,
    SourceLanguage::Template,
    SourceLanguage::Javascript,
    SourceLanguage::Typescript,
    SourceLanguage::Jsp,
];

pub(super) fn module_files(
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
pub(super) fn module_test_sources(
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

pub(super) fn collect_source_dirs(
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

pub(super) fn source_dirs(
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
        // Go and Rust sources are checked in source-only projects, not in JVM build modules.
        SourceLanguage::Go | SourceLanguage::Rust => Cow::Owned(Vec::new()),
        SourceLanguage::Html | SourceLanguage::Javascript | SourceLanguage::Typescript => {
            let mut dirs = vec![
                "src/main/resources/static",
                "src/main/resources/public",
                "src/main/webapp",
            ];
            if language != SourceLanguage::Html {
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
