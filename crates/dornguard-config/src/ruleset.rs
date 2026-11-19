use std::fs::{self, File};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output, Stdio};

use dornguard_core::fingerprint;
use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::BUILTIN_VERSION;

const MANIFEST: &str = "ruleset.yml";
const LOCK_FILE: &str = ".dornguard-ruleset.lock.json";
const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ruleset {
    #[serde(default)]
    pub source: PolicySource,
    pub version: Option<String>,
    pub path: Option<PathBuf>,
    repository: Option<String>,
    #[serde(rename = "ref")]
    reference: Option<String>,
}

#[derive(Default, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PolicySource {
    #[default]
    Builtin,
    Local,
    Git,
}

impl Ruleset {
    pub fn validate(&self) -> Result<()> {
        match self.source {
            PolicySource::Builtin => {
                if self.path.is_some() || self.repository.is_some() || self.reference.is_some() {
                    bail!("Built-in rulesets accept only a version");
                }
                if let Some(version) = &self.version
                    && version != BUILTIN_VERSION
                {
                    bail!(
                        "Unknown built-in ruleset version {version}; bundled version is {BUILTIN_VERSION}"
                    );
                }
            }
            PolicySource::Local => {
                if self.version.is_some() || self.repository.is_some() || self.reference.is_some() {
                    bail!("Local rulesets accept only a project-relative path");
                }
                checked_path(
                    self.path
                        .as_deref()
                        .ok_or_else(|| miette!("Missing local ruleset path"))?,
                )?;
            }
            PolicySource::Git => {
                if self.path.is_some()
                    || self.version.is_some()
                    || self.repository.is_none()
                    || self.reference.is_none()
                {
                    bail!("Git rulesets require repository and ref fields");
                }
            }
        }
        Ok(())
    }

    pub fn validate_trust(&self, policy: &Path, worktree: &Path) -> Result<()> {
        match self.source {
            PolicySource::Local => {
                let base = policy
                    .parent()
                    .ok_or_else(|| miette!("Missing organization policy directory"))?;
                let path = self
                    .path
                    .as_deref()
                    .ok_or_else(|| miette!("Missing organization ruleset path"))?;
                let location = dunce::canonicalize(base.join(path)).into_diagnostic()?;
                if location.starts_with(worktree) {
                    bail!("Organization ruleset files must be outside the project worktree");
                }
            }
            PolicySource::Git => {
                if !self
                    .repository
                    .as_deref()
                    .is_some_and(|repository| repository.starts_with("https://"))
                    || !self.reference.as_deref().is_some_and(full_hash)
                {
                    bail!("An organization Git ruleset requires HTTPS and a full commit ID");
                }
            }
            PolicySource::Builtin => {}
        }
        Ok(())
    }
}

pub enum Source {
    Local(PathBuf),
    Git { cache: PathBuf, commit: String },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    version: u32,
    #[serde(default)]
    pub profiles: Vec<String>,
    #[serde(default)]
    pub rules: Vec<String>,
}

impl Source {
    pub fn open(config_path: &Path, ruleset: &Ruleset) -> Result<Option<Self>> {
        let config_root = config_path
            .parent()
            .ok_or_else(|| miette!("Missing project directory"))?;
        match ruleset.source {
            PolicySource::Builtin => Ok(None),
            PolicySource::Local => {
                let path = ruleset
                    .path
                    .as_deref()
                    .ok_or_else(|| miette!("Missing local ruleset path"))?;
                let full = dunce::canonicalize(config_root.join(path))
                    .into_diagnostic()
                    .wrap_err("Could not open local ruleset directory")?;
                if !full.is_dir() || !full.starts_with(config_root) {
                    bail!("Local ruleset must stay inside the project");
                }
                Ok(Some(Self::Local(full)))
            }
            PolicySource::Git => Ok(Some(open_git(config_root, ruleset)?)),
        }
    }

    pub fn manifest(&self) -> Result<Manifest> {
        let manifest: Manifest = self.parse(MANIFEST)?;
        if manifest.version != 1 {
            bail!("Unsupported ruleset manifest version {}", manifest.version);
        }
        Ok(manifest)
    }

    pub fn parse<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        let content = self.read_file(path)?;
        serde_saphyr::from_str(&content)
            .into_diagnostic()
            .wrap_err_with(|| format!("Invalid ruleset YAML in {path}"))
    }

    pub fn read_file(&self, name: &str) -> Result<String> {
        let path = checked_path(Path::new(name))?;
        let bytes = match self {
            Self::Local(root) => {
                let location = dunce::canonicalize(root.join(path))
                    .into_diagnostic()
                    .wrap_err_with(|| format!("Could not find ruleset file {name}"))?;
                if !location.starts_with(root) || !location.is_file() {
                    bail!("Ruleset file must stay inside the policy directory: {name}");
                }
                match fs::metadata(&location) {
                    Ok(metadata) if metadata.len() <= MAX_FILE_BYTES as u64 => {}
                    Ok(_) => bail!("Ruleset file is too large: {name}"),
                    Err(error) => return Err(error).into_diagnostic(),
                }
                fs::read(location).into_diagnostic()?
            }
            Self::Git { cache, commit } => {
                let item =
                    checked_git(cache, &["ls-tree", "-z", "--full-tree", commit, "--", name])?;
                if !item.starts_with(b"100644 blob ") && !item.starts_with(b"100755 blob ") {
                    bail!("Expected a regular ruleset file in Git: {name}");
                }
                let object = format!("{commit}:{name}");
                let size = checked_git(cache, &["cat-file", "-s", &object])?;
                let size: usize = std::str::from_utf8(&size)
                    .into_diagnostic()?
                    .trim()
                    .parse()
                    .into_diagnostic()?;
                if size > MAX_FILE_BYTES {
                    bail!("Ruleset file is too large: {name}");
                }
                checked_git(cache, &["cat-file", "blob", &object])?
            }
        };
        if bytes.len() > MAX_FILE_BYTES {
            bail!("Ruleset file is too large: {name}");
        }
        String::from_utf8(bytes)
            .into_diagnostic()
            .wrap_err_with(|| format!("Invalid UTF-8 in {name}"))
    }
}

fn checked_path(path: &Path) -> Result<&Path> {
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        bail!(
            "Ruleset paths must be relative, without parent traversal: {}",
            path.display()
        );
    }
    Ok(path)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct GitLock {
    version: u8,
    repository: String,
    reference: String,
    commit: String,
}

fn open_git(config_root: &Path, ruleset: &Ruleset) -> Result<Source> {
    let repository = ruleset
        .repository
        .as_deref()
        .ok_or_else(|| miette!("Missing Git ruleset repository"))?;
    let reference = ruleset
        .reference
        .as_deref()
        .ok_or_else(|| miette!("Missing Git ruleset ref"))?;
    let https = repository
        .strip_prefix("https://")
        .and_then(|url| url.split('/').next())
        .is_some_and(|host| !host.is_empty() && !host.contains('@'));
    if !(https || Path::new(repository).is_absolute()) || repository.chars().any(char::is_control) {
        bail!(
            "Git rulesets require an HTTPS repository URL or an absolute local Git repository path"
        );
    }

    let cache_root = cache_root()?.join("rulesets");
    fs::create_dir_all(&cache_root).into_diagnostic()?;
    let cache_id = fingerprint(repository);
    let cache = cache_root.join(format!("{cache_id}.git"));
    let guard = File::options()
        .create(true)
        .truncate(false)
        .write(true)
        .open(cache_root.join(format!("{cache_id}.lock")))
        .into_diagnostic()?;
    guard.lock().into_diagnostic()?;
    if !cache.join("HEAD").is_file() {
        fs::create_dir_all(&cache).into_diagnostic()?;
        checked_git(&cache, &["init", "--quiet", "--bare"])?;
    }

    let lock_path = config_root.join(LOCK_FILE);
    let commit = if full_hash(reference) {
        ensure_commit(&cache, repository, reference)?;
        reference.to_owned()
    } else {
        let tag = if reference.starts_with("refs/tags/") {
            reference.to_owned()
        } else {
            format!("refs/tags/{reference}")
        };
        checked_git(&cache, &["check-ref-format", &tag]).wrap_err("Invalid Git ruleset tag")?;
        match read_lock(&lock_path)? {
            Some(lock) => {
                if lock.version != 1
                    || lock.repository != repository
                    || lock.reference != reference
                    || !full_hash(&lock.commit)
                {
                    bail!("Ruleset lock does not match the configured repository and tag");
                }
                ensure_commit(&cache, repository, &lock.commit)?;
                lock.commit
            }
            None => {
                checked_git(
                    &cache,
                    &[
                        "fetch",
                        "--quiet",
                        "--no-tags",
                        "--depth=1",
                        "--",
                        repository,
                        &tag,
                    ],
                )
                .wrap_err("Could not fetch the pinned Git ruleset tag")?;
                let commit =
                    checked_git(&cache, &["rev-parse", "--verify", "FETCH_HEAD^{commit}"])?;
                let commit = std::str::from_utf8(&commit)
                    .into_diagnostic()?
                    .trim()
                    .to_owned();
                if !full_hash(&commit) {
                    bail!("Git did not resolve the ruleset tag to a full commit ID");
                }
                write_lock(
                    &lock_path,
                    &GitLock {
                        version: 1,
                        repository: repository.to_owned(),
                        reference: reference.to_owned(),
                        commit: commit.clone(),
                    },
                )?;
                commit
            }
        }
    };
    checked_git(
        &cache,
        &[
            "update-ref",
            &format!("refs/dornguard/pins/{commit}"),
            &commit,
        ],
    )?;
    Ok(Source::Git { cache, commit })
}

fn ensure_commit(cache: &Path, repository: &str, commit: &str) -> Result<()> {
    let object = format!("{commit}^{{commit}}");
    if !git(cache, &["cat-file", "-e", &object])?.status.success() {
        checked_git(
            cache,
            &[
                "fetch",
                "--quiet",
                "--no-tags",
                "--depth=1",
                "--",
                repository,
                commit,
            ],
        )
        .wrap_err("Could not fetch the pinned Git ruleset commit")?;
        checked_git(cache, &["cat-file", "-e", &object])?;
    }
    Ok(())
}

fn full_hash(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn read_lock(path: &Path) -> Result<Option<GitLock>> {
    match fs::read(path) {
        Ok(contents) => serde_json::from_slice(&contents)
            .map(Some)
            .into_diagnostic()
            .wrap_err("Invalid ruleset lock file"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error)
            .into_diagnostic()
            .wrap_err("Could not read ruleset lock file"),
    }
}

fn write_lock(path: &Path, lock: &GitLock) -> Result<()> {
    let mut file = tempfile::NamedTempFile::new_in(
        path.parent()
            .ok_or_else(|| miette!("Missing lock directory"))?,
    )
    .into_diagnostic()?;
    serde_json::to_writer_pretty(&mut file, lock).into_diagnostic()?;
    writeln!(file).into_diagnostic()?;
    file.as_file().sync_all().into_diagnostic()?;
    file.persist_noclobber(path)
        .into_diagnostic()
        .wrap_err("Could not save ruleset lock")?;
    Ok(())
}

fn cache_root() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("DORNGUARD_CACHE_DIR") {
        return Ok(PathBuf::from(path));
    }
    if let Some(path) = std::env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(path).join("dornguard"));
    }
    #[cfg(windows)]
    let location = std::env::var_os("LOCALAPPDATA")
        .map(|home| PathBuf::from(home).join("DornGuard").join("cache"));
    #[cfg(not(windows))]
    let location =
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".cache").join("dornguard"));
    location.ok_or_else(|| miette!("Set DORNGUARD_CACHE_DIR to cache the Git ruleset"))
}

fn git(cache: &Path, args: &[&str]) -> Result<Output> {
    Command::new("git")
        .current_dir(cache)
        .stdin(Stdio::null())
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .args([
            "--literal-pathspecs",
            "-c",
            "protocol.ext.allow=never",
            "-c",
            "http.lowSpeedLimit=1024",
            "-c",
            "http.lowSpeedTime=20",
        ])
        .args(args)
        .output()
        .into_diagnostic()
        .wrap_err("Could not execute Git for the ruleset")
}

fn checked_git(cache: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = git(cache, args)?;
    if !output.status.success() {
        bail!(
            "Git ruleset operation {} failed ({})",
            args[0],
            output.status
        );
    }
    Ok(output.stdout)
}
