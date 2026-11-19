use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use dornguard_core::SourceLanguage;
use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};
use tokio::process::Command;

#[derive(Default)]
pub struct Changes {
    source_paths: BTreeSet<PathBuf>,
}

#[derive(Clone, Copy)]
pub struct Request<'a> {
    pub base: Option<&'a str>,
    pub baseline: Option<&'a Path>,
}

impl Changes {
    pub fn matches(&self, path: &Path) -> bool {
        self.source_paths.contains(path)
    }

    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        self.source_paths.iter().map(PathBuf::as_path)
    }
}

pub async fn discover(
    root: &Path,
    config: &Path,
    ruleset_dir: Option<&Path>,
    request: Request<'_>,
) -> Result<Changes> {
    let baseline = match request.baseline {
        Some(path) => Some(tracked_baseline(root, path).await?),
        None => None,
    };
    let commits = match request.base {
        Some(base) => Some(compare_commits(root, base).await?),
        None => None,
    };
    let head = commits.as_ref().map_or("HEAD", |(_, head)| head.as_str());
    let committed = async {
        if let Some((start, head)) = &commits {
            diff(root, &[start, head]).await
        } else {
            Ok(Vec::new())
        }
    };
    let staged_args = ["--cached", head];
    let staged = diff(root, &staged_args);
    let unstaged = diff(root, &[]);
    let new = git(
        root,
        &[
            "ls-files",
            "--others",
            "--exclude-standard",
            "-z",
            "--",
            ".",
        ],
    );
    let (committed, staged, unstaged, new) = tokio::try_join!(committed, staged, unstaged, new)?;

    let mut changes = Changes::default();
    for output in [&committed, &staged, &unstaged, &new] {
        let paths = std::str::from_utf8(output)
            .into_diagnostic()
            .wrap_err("Git reported a change with a non-UTF-8 path")?;
        for path in paths.split_terminator('\0').map(Path::new) {
            if policy_or_build_path(path)
                || config.strip_prefix(root).is_ok_and(|config| config == path)
                || ruleset_dir.is_some_and(|dir| path.starts_with(dir))
                || baseline.as_deref() == Some(path)
            {
                bail!("Policy or build input changed: {}", path.display());
            }
            if SourceLanguage::of(path).is_some() {
                changes.source_paths.insert(path.to_path_buf());
            }
        }
    }
    Ok(changes)
}

async fn compare_commits(root: &Path, reference: &str) -> Result<(String, String)> {
    let head = resolve_commit(root, "HEAD").await?;
    let base = resolve_commit(root, reference).await?;
    let output = git(root, &["merge-base", "--all", &base, &head])
        .await
        .wrap_err_with(|| format!("Could not find the merge base of HEAD and {reference}"))?;
    let start = object_id(&output).ok_or_else(|| {
        miette!("Could not determine a unique merge base of HEAD and {reference}")
    })?;
    Ok((start, head))
}

async fn resolve_commit(root: &Path, reference: &str) -> Result<String> {
    let expression = format!("{reference}^{{commit}}");
    let output = git(
        root,
        &["rev-parse", "--verify", "--end-of-options", &expression],
    )
    .await
    .wrap_err_with(|| format!("Could not resolve {reference} to a Git commit"))?;
    object_id(&output).ok_or_else(|| miette!("Git returned an invalid commit ID for {reference}"))
}

fn object_id(output: &[u8]) -> Option<String> {
    let object = std::str::from_utf8(output).ok()?.trim();
    (matches!(object.len(), 40 | 64) && object.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .then(|| object.to_owned())
}

async fn diff(root: &Path, comparison: &[&str]) -> Result<Vec<u8>> {
    let mut args = vec!["diff", "--name-only", "-z", "--no-renames", "--relative"];
    args.extend_from_slice(comparison);
    args.extend(["--", "."]);
    git(root, &args).await
}

async fn tracked_baseline(root: &Path, path: &Path) -> Result<PathBuf> {
    // Use the same current directory spelling as project discovery, including on Windows.
    let path_from_cwd = if path.is_absolute() {
        path.to_path_buf()
    } else {
        dunce::canonicalize(".")
            .into_diagnostic()
            .wrap_err("Could not resolve the current directory")?
            .join(path)
    };
    let absolute = std::path::absolute(path_from_cwd)
        .into_diagnostic()
        .wrap_err("Could not locate the selected baseline")?;
    let canonical = dunce::canonicalize(&absolute)
        .into_diagnostic()
        .wrap_err("Could not resolve the selected baseline")?;
    if canonical != absolute {
        bail!(
            "The selected baseline path resolves through an alias: {}",
            path.display()
        );
    }
    let relative = canonical
        .strip_prefix(root)
        .into_diagnostic()
        .wrap_err("The selected baseline is outside the project")?;
    let relative_text = relative
        .to_str()
        .ok_or_else(|| miette!("The selected baseline has a non-UTF-8 path"))?;
    #[cfg(windows)]
    let relative_text = &relative_text.replace('\\', "/");
    git(root, &["ls-files", "--error-unmatch", "--", relative_text])
        .await
        .wrap_err_with(|| {
            format!(
                "The selected baseline is not tracked by Git: {}",
                relative.display()
            )
        })?;
    Ok(relative.to_path_buf())
}

fn policy_or_build_path(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    matches!(
        name,
        ".dornguard.yml"
            | ".dornguard-baseline.json"
            | ".dornguard-ruleset.lock.json"
            | ".gitmodules"
            | ".semgrepignore"
            | ".gitignore"
            | "gradlew"
            | "gradlew.bat"
            | "gradle.properties"
            | "settings.gradle"
            | "settings.gradle.kts"
    ) || name.ends_with(".gradle")
        || name.ends_with(".gradle.kts")
        || path.components().any(|part| {
            matches!(
                part.as_os_str().to_str(),
                Some("gradle" | "buildSrc" | "build-logic")
            )
        })
        || path.starts_with("rules")
        || path.starts_with("profiles")
}

async fn git(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let mut command = Command::new("git");
    command
        .current_dir(root)
        .kill_on_drop(true)
        .stdin(Stdio::null())
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .arg("--literal-pathspecs")
        .args(args);
    let output = tokio::time::timeout(Duration::from_secs(10), command.output())
        .await
        .into_diagnostic()
        .wrap_err("Git change detection timed out")?
        .into_diagnostic()
        .wrap_err("Could not run Git")?;
    if !output.status.success() {
        bail!("Could not compare the Git working tree with HEAD");
    }
    Ok(output.stdout)
}
