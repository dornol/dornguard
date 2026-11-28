use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use dornguard_core::EngineName;
use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};
use sha2::{Digest, Sha256};
use tokio::process::Command;

use crate::{
    ESLINT_INSTALL_VERSION, OSV_SCANNER_VERSION, PMD_VERSION, SEMGREP_VERSION, supported_version,
};

const MANAGED: &str = ".dornguard-managed";
const READY: &str = ".dornguard-ready";
const ESLINT_PACKAGE: &str = include_str!("../engines/eslint/package.json");
const ESLINT_LOCK: &str = include_str!("../engines/eslint/package-lock.json");
const PMD_CHECKSUM: &str = "d27177c185b837fa57f097e482d1253468aa035379c5eb2671fa0a279109b179";

/// Create the install root and take the exclusive install lock until the guard drops.
async fn lock_install_root() -> Result<(PathBuf, File)> {
    let root = install_root()
        .ok_or_else(|| miette!("Set DORNGUARD_ENGINE_DIR to a tool installation directory"))?;
    fs::create_dir_all(&root).into_diagnostic()?;
    let root = dunce::canonicalize(root).into_diagnostic()?;
    let guard = tokio::task::spawn_blocking({
        let root = root.clone();
        move || -> Result<File> {
            let guard = File::options()
                .create(true)
                .truncate(false)
                .write(true)
                .open(root.join(".install.lock"))
                .into_diagnostic()?;
            guard.lock().into_diagnostic()?;
            Ok(guard)
        }
    })
    .await
    .into_diagnostic()??;
    Ok((root, guard))
}

pub async fn install(engine: EngineName) -> Result<PathBuf> {
    let (root, _guard) = lock_install_root().await?;

    let destination = engine_directory(&root, engine);
    // A managed ESLint installed from an older lock (for example one without the
    // TypeScript parser) still runs, but installing again brings it up to date.
    let current = engine != EngineName::Eslint
        || fs::read_to_string(destination.join("package-lock.json"))
            .is_ok_and(|lock| lock == ESLINT_LOCK);
    if current && managed_engine(&root, engine).is_some() {
        return Ok(destination);
    }

    match engine {
        EngineName::Semgrep => install_semgrep(&root, &destination).await?,
        EngineName::Pmd => install_pmd(&root, &destination).await?,
        EngineName::Eslint => install_eslint(&destination).await?,
        _ => {
            bail!("The ArchUnit runner is distributed with DornGuard releases");
        }
    }
    Ok(destination)
}

pub(crate) fn engine_command(engine: EngineName) -> Result<Command> {
    let managed = install_root()
        .and_then(|root| managed_engine(&root, engine))
        .and_then(|path| dunce::canonicalize(path).ok());
    if cfg!(windows)
        && engine == EngineName::Pmd
        && let Some(script) = managed.clone().or_else(|| path_lookup("pmd.bat"))
        && let Some(root) = script.parent().and_then(Path::parent)
    {
        let mut command = Command::new(java_executable());
        command
            .arg("-cp")
            .arg(root.join("lib").join("*"))
            .arg("net.sourceforge.pmd.cli.PmdCli");
        return Ok(command);
    }
    let executable = managed
        .or_else(|| path_lookup(engine.executable()))
        .ok_or_else(|| match engine {
            EngineName::GoVet => miette!("Could not find go; install Go 1.22 or later and put it on PATH"),
            EngineName::Clippy => miette!(
                "Could not find cargo; install Rust with rustup and run 'rustup component add clippy'"
            ),
            _ => miette!(
                "Could not find {}; run dornguard engines install {}",
                engine.executable(),
                engine.executable()
            ),
        })?;
    Ok(Command::new(executable))
}

pub(crate) fn java_executable() -> PathBuf {
    let binary = if cfg!(windows) { "java.exe" } else { "java" };
    std::env::var_os("DORNGUARD_JAVA_HOME")
        .filter(|java| !java.is_empty())
        .or_else(|| std::env::var_os("JAVA_HOME"))
        .map_or_else(
            || PathBuf::from(binary),
            |java| PathBuf::from(java).join("bin").join(binary),
        )
}

pub(crate) fn java_command(root: &Path) -> Command {
    let mut command = Command::new(java_executable());
    command
        .current_dir(root)
        .kill_on_drop(true)
        .env_remove("JAVA_TOOL_OPTIONS")
        .env_remove("_JAVA_OPTIONS")
        .env_remove("JDK_JAVA_OPTIONS");
    command
}

pub async fn inspect_java(root: &Path) -> Result<String> {
    let executable = java_executable();
    let output = tokio::time::timeout(
        Duration::from_secs(15),
        java_command(root)
            .args(["-XshowSettings:properties", "-version"])
            .output(),
    )
    .await
    .into_diagnostic()
    .wrap_err("Java version check timed out")?
    .into_diagnostic()
    .wrap_err_with(|| {
        format!(
            "Could not start {}; ArchUnit requires Java 21 or later; set DORNGUARD_JAVA_HOME to a Java 21+ installation",
            executable.display()
        )
    })?;
    if !output.status.success() {
        bail!(
            "Java version check failed for {} ({}); ArchUnit requires Java 21 or later; set DORNGUARD_JAVA_HOME to a Java 21+ installation",
            executable.display(),
            output.status
        );
    }
    let settings = String::from_utf8_lossy(&output.stderr);
    let property = |name| {
        settings.lines().find_map(|line| {
            let (key, value) = line.trim().split_once(" = ")?;
            (key == name).then_some(value)
        })
    };
    let version = property("java.version").unwrap_or("unknown");
    let executable = property("java.home")
        .map(|home| {
            Path::new(home)
                .join("bin")
                .join(if cfg!(windows) { "java.exe" } else { "java" })
        })
        .unwrap_or(executable);
    let major = version
        .split(['.', '-', '+'])
        .next()
        .and_then(|part| part.parse::<u32>().ok());
    if major.is_none_or(|major| major < 21) {
        bail!(
            "Java {version} at {}; ArchUnit requires Java 21 or later; set DORNGUARD_JAVA_HOME to a Java 21+ installation",
            executable.display()
        );
    }
    Ok(format!("Java {version} at {}", executable.display()))
}

fn path_lookup(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let names = if cfg!(windows) && !name.contains('.') {
        vec![format!("{name}.exe"), name.to_owned()]
    } else {
        vec![name.to_owned()]
    };
    for directory in std::env::split_paths(&path).filter(|path| path.is_absolute()) {
        for name in &names {
            let path = directory.join(name);
            if executable_file(&path) {
                return Some(path);
            }
        }
    }
    None
}

fn executable_file(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    metadata.is_file()
}

fn install_root() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("DORNGUARD_ENGINE_DIR") {
        return Some(PathBuf::from(dir));
    }
    #[cfg(windows)]
    return std::env::var_os("LOCALAPPDATA")
        .map(|home| PathBuf::from(home).join("DornGuard").join("engines"));
    #[cfg(not(windows))]
    std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share")))
        .map(|home| home.join("dornguard").join("engines"))
}

fn engine_directory(root: &Path, engine: EngineName) -> PathBuf {
    let version = if engine == EngineName::Eslint {
        ESLINT_INSTALL_VERSION
    } else {
        supported_version(engine)
    };
    root.join(format!("{}-{version}", engine.executable()))
}

/// Release asset and SHA-256 for OSV-Scanner on one platform, from the release's
/// published `osv-scanner_SHA256SUMS`.
fn osv_scanner_asset(os: &str, arch: &str) -> Option<(&'static str, &'static str)> {
    match (os, arch) {
        ("macos", "x86_64") => Some((
            "osv-scanner_darwin_amd64",
            "e82c5df7ab0a42dff29a1cd0660711447b56b4c0483ff66ad8634df10fd81a45",
        )),
        ("macos", "aarch64") => Some((
            "osv-scanner_darwin_arm64",
            "d4926ef8da134332543be276dff157e4c170f99ad91fc48e89448a8816bdf70b",
        )),
        ("linux", "x86_64") => Some((
            "osv-scanner_linux_amd64",
            "160c6253e2922e9e0a6a9771c5c4c40a4b3c6090b34a3cdd5e80c96cc2e4218b",
        )),
        ("linux", "aarch64") => Some((
            "osv-scanner_linux_arm64",
            "4061ef4c021ed3c47f203b818f337e351336cc5cd67c0274fc36bd1dfc728dd7",
        )),
        ("windows", "x86_64") => Some((
            "osv-scanner_windows_amd64.exe",
            "9964ed64ec9a387391eb4cc006ad5c531154821aef29b793e02c448de4686438",
        )),
        ("windows", "aarch64") => Some((
            "osv-scanner_windows_arm64.exe",
            "3e630381c1f1fd3c9c4d447e6ff2ef6c737083371722773db8337eead0f97962",
        )),
        _ => None,
    }
}

fn osv_scanner_directory(root: &Path) -> PathBuf {
    root.join(format!("osv-scanner-{OSV_SCANNER_VERSION}"))
}

fn managed_osv_scanner(root: &Path) -> Option<PathBuf> {
    let directory = osv_scanner_directory(root);
    let binary = directory.join(if cfg!(windows) {
        "osv-scanner.exe"
    } else {
        "osv-scanner"
    });
    (directory.join(READY).is_file() && binary.is_file()).then_some(binary)
}

/// The OSV-Scanner to run: one on `PATH`, else the managed install, else the bare
/// name so that a missing scanner reports a start failure.
pub fn osv_scanner_executable() -> PathBuf {
    let name = if cfg!(windows) {
        "osv-scanner.exe"
    } else {
        "osv-scanner"
    };
    if path_lookup(name).is_some() {
        return PathBuf::from(name);
    }
    install_root()
        .and_then(|root| managed_osv_scanner(&root))
        .and_then(|path| dunce::canonicalize(path).ok())
        .unwrap_or_else(|| PathBuf::from(name))
}

/// Download the pinned OSV-Scanner release and verify it against its recorded SHA-256.
pub async fn install_osv_scanner() -> Result<PathBuf> {
    let (root, _guard) = lock_install_root().await?;
    let destination = osv_scanner_directory(&root);
    if managed_osv_scanner(&root).is_some() {
        return Ok(destination);
    }
    let (asset, checksum) = osv_scanner_asset(std::env::consts::OS, std::env::consts::ARCH)
        .ok_or_else(|| miette!("No verified OSV-Scanner release is available on this platform"))?;
    if destination.exists() {
        if !destination.join(MANAGED).is_file() {
            bail!(
                "Refusing to overwrite an unmanaged engine directory at {}",
                destination.display()
            );
        }
        fs::remove_dir_all(&destination).into_diagnostic()?;
    }
    let temporary = tempfile::tempdir_in(&root).into_diagnostic()?;
    let download = temporary.path().join(asset);
    run(
        Command::new("curl")
            .args([
                "--fail",
                "--silent",
                "--show-error",
                "--location",
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                "--output",
            ])
            .arg(&download)
            .arg(format!(
                "https://github.com/google/osv-scanner/releases/download/v{OSV_SCANNER_VERSION}/{asset}"
            )),
        "download OSV-Scanner",
    )
    .await?;
    verify_checksum(&download, checksum).wrap_err("OSV-Scanner download verification failed")?;

    fs::create_dir(&destination).into_diagnostic()?;
    File::create(destination.join(MANAGED)).into_diagnostic()?;
    let binary = destination.join(if cfg!(windows) {
        "osv-scanner.exe"
    } else {
        "osv-scanner"
    });
    fs::copy(&download, &binary).into_diagnostic()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).into_diagnostic()?;
    }
    let output = tokio::time::timeout(
        Duration::from_secs(30),
        Command::new(&binary).arg("--version").output(),
    )
    .await
    .into_diagnostic()
    .wrap_err("OSV-Scanner version check timed out")?
    .into_diagnostic()
    .wrap_err("Could not run the downloaded OSV-Scanner")?;
    if !String::from_utf8_lossy(&output.stdout).contains(OSV_SCANNER_VERSION) {
        bail!("Downloaded OSV-Scanner is not version {OSV_SCANNER_VERSION}");
    }
    File::create(destination.join(READY)).into_diagnostic()?;
    Ok(destination)
}

/// The directory to resolve `@typescript-eslint/parser` from, in the order DornGuard
/// picks ESLint: the project, the installation of the ESLint on `PATH` (including a
/// global npm install), then the managed ESLint.
pub(crate) fn typescript_parser_base(project: &Path) -> Option<PathBuf> {
    let has_parser = |dir: &Path| {
        dir.join("node_modules/@typescript-eslint/parser/package.json")
            .is_file()
    };
    if has_parser(project) {
        return Some(project.to_path_buf());
    }
    let executable = if cfg!(windows) {
        "eslint.cmd"
    } else {
        "eslint"
    };
    // `.bin/eslint` links into `node_modules/eslint`, so the parser installed
    // beside that ESLint is a sibling of an ancestor directory.
    if let Some(eslint) = path_lookup(executable).and_then(|path| dunce::canonicalize(path).ok())
        && let Some(base) = eslint.ancestors().skip(1).find(|dir| has_parser(dir))
    {
        return Some(base.to_path_buf());
    }
    let managed = engine_directory(&install_root()?, EngineName::Eslint);
    has_parser(&managed)
        .then(|| dunce::canonicalize(&managed).ok())
        .flatten()
}

/// The managed ESLint, used only when neither the project nor `PATH` provides one.
pub(crate) fn managed_eslint_fallback(executable: &str) -> Option<PathBuf> {
    if path_lookup(executable).is_some() {
        return None;
    }
    let root = install_root()?;
    managed_engine(&root, EngineName::Eslint).and_then(|path| dunce::canonicalize(path).ok())
}

fn managed_engine(root: &Path, engine: EngineName) -> Option<PathBuf> {
    let directory = engine_directory(root, engine);
    let path = match engine {
        EngineName::Semgrep if cfg!(windows) => directory.join("Scripts/semgrep.exe"),
        EngineName::Semgrep => directory.join("bin/semgrep"),
        EngineName::Pmd if cfg!(windows) => directory.join("bin/pmd.bat"),
        EngineName::Pmd => directory.join("bin/pmd"),
        EngineName::Eslint if cfg!(windows) => directory.join("node_modules/.bin/eslint.cmd"),
        EngineName::Eslint => directory.join("node_modules/.bin/eslint"),
        _ => return None,
    };
    if engine == EngineName::Eslint && !directory.join("node_modules/eslint/package.json").is_file()
    {
        return None;
    }
    // A managed Semgrep is a venv. If the interpreter it was built from is gone
    // (for example after a Homebrew Python upgrade), its `python` link dangles
    // and the engine cannot start, so it must not count as installed.
    let interpreter_works = engine != EngineName::Semgrep
        || directory
            .join(if cfg!(windows) {
                "Scripts/python.exe"
            } else {
                "bin/python"
            })
            .is_file();
    (directory.join(READY).is_file() && path.is_file() && interpreter_works).then_some(path)
}

async fn install_semgrep(root: &Path, destination: &Path) -> Result<()> {
    let checksum = semgrep_checksum()
        .ok_or_else(|| miette!("No verified Semgrep wheel is available on this platform"))?;
    let python = find_python().await?;
    if destination.exists() {
        if !destination.join(MANAGED).is_file() {
            bail!(
                "Refusing to overwrite an unmanaged engine directory at {}",
                destination.display()
            );
        }
        fs::remove_dir_all(destination).into_diagnostic()?;
    }
    fs::create_dir(destination).into_diagnostic()?;
    File::create(destination.join(MANAGED)).into_diagnostic()?;
    run(
        Command::new(&python)
            .args(["-I", "-m", "venv"])
            .arg(destination),
        "prepare Python environment",
    )
    .await?;

    let venv_python = destination.join(if cfg!(windows) {
        "Scripts/python.exe"
    } else {
        "bin/python"
    });
    let temporary = tempfile::tempdir_in(root).into_diagnostic()?;
    let requirements = temporary.path().join("semgrep.txt");
    fs::write(
        &requirements,
        format!("semgrep=={SEMGREP_VERSION} --hash=sha256:{checksum}\n"),
    )
    .into_diagnostic()?;

    run(
        Command::new(&venv_python)
            .args([
                "-I",
                "-m",
                "pip",
                "--isolated",
                "download",
                "--no-deps",
                "--only-binary=:all:",
                "--require-hashes",
                "--dest",
            ])
            .arg(temporary.path())
            .arg("-r")
            .arg(requirements),
        "download and verify Semgrep",
    )
    .await?;
    let wheel = fs::read_dir(temporary.path())
        .into_diagnostic()?
        .filter_map(std::result::Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|extension| extension == "whl"))
        .ok_or_else(|| miette!("No verified Semgrep wheel was downloaded"))?;
    run(
        Command::new(&venv_python)
            .args([
                "-I",
                "-m",
                "pip",
                "--isolated",
                "install",
                "--prefer-binary",
            ])
            .arg(wheel),
        "install Semgrep",
    )
    .await?;
    File::create(destination.join(READY)).into_diagnostic()?;
    Ok(())
}

/// Whether a Node.js version satisfies ESLint's `^20.19.0 || ^22.13.0 || >=24`.
fn node_supported(version: &str) -> bool {
    let mut parts = version
        .trim()
        .trim_start_matches('v')
        .split('.')
        .map(|part| part.parse::<u32>().ok());
    let (Some(Some(major)), Some(Some(minor))) = (parts.next(), parts.next()) else {
        return false;
    };
    match major {
        20 => minor >= 19,
        22 => minor >= 13,
        major => major >= 24,
    }
}

async fn install_eslint(destination: &Path) -> Result<()> {
    let node = path_lookup(if cfg!(windows) { "node.exe" } else { "node" })
        .ok_or_else(|| miette!("Install Node.js 20.19, 22.13 or 24 or later to install ESLint"))?;
    let npm = path_lookup(if cfg!(windows) { "npm.cmd" } else { "npm" })
        .ok_or_else(|| miette!("Install npm (it ships with Node.js) to install ESLint"))?;
    let output = tokio::time::timeout(
        Duration::from_secs(15),
        Command::new(&node).arg("--version").output(),
    )
    .await
    .into_diagnostic()
    .wrap_err("Node.js version check timed out")?
    .into_diagnostic()
    .wrap_err("Could not run node")?;
    let node_version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if !node_supported(&node_version) {
        bail!(
            "Node.js {node_version} cannot run ESLint {ESLINT_INSTALL_VERSION}; \
             install Node.js 20.19, 22.13 or 24 or later"
        );
    }
    if destination.exists() {
        if !destination.join(MANAGED).is_file() {
            bail!(
                "Refusing to overwrite an unmanaged engine directory at {}",
                destination.display()
            );
        }
        fs::remove_dir_all(destination).into_diagnostic()?;
    }
    fs::create_dir(destination).into_diagnostic()?;
    File::create(destination.join(MANAGED)).into_diagnostic()?;
    fs::write(destination.join("package.json"), ESLINT_PACKAGE).into_diagnostic()?;
    fs::write(destination.join("package-lock.json"), ESLINT_LOCK).into_diagnostic()?;
    // `npm ci` installs exactly the locked, integrity-checked tree and fails on any
    // mismatch. Install scripts never run.
    run(
        Command::new(&npm)
            .args(["ci", "--ignore-scripts", "--no-audit", "--no-fund"])
            .current_dir(destination),
        "install ESLint with npm",
    )
    .await?;
    let installed = destination.join("node_modules/eslint/package.json");
    let manifest = fs::read_to_string(&installed)
        .into_diagnostic()
        .wrap_err("ESLint was not installed")?;
    let manifest: serde_json::Value = serde_json::from_str(&manifest).into_diagnostic()?;
    if manifest.get("version").and_then(serde_json::Value::as_str) != Some(ESLINT_INSTALL_VERSION) {
        bail!("Installed ESLint is not version {ESLINT_INSTALL_VERSION}");
    }
    if !destination
        .join("node_modules/@typescript-eslint/parser/package.json")
        .is_file()
    {
        bail!("The TypeScript parser for ESLint was not installed");
    }
    if !destination
        .join("node_modules/@typescript-eslint/eslint-plugin/package.json")
        .is_file()
    {
        bail!("The typescript-eslint plugin for ESLint was not installed");
    }
    File::create(destination.join(READY)).into_diagnostic()?;
    Ok(())
}

async fn install_pmd(root: &Path, destination: &Path) -> Result<()> {
    if destination.exists() {
        bail!(
            "Refusing to overwrite an existing PMD directory at {}",
            destination.display()
        );
    }
    let temporary = tempfile::tempdir_in(root).into_diagnostic()?;
    let zip = temporary.path().join("pmd.zip");
    let url = format!(
        "https://github.com/pmd/pmd/releases/download/pmd_releases%2F{PMD_VERSION}/pmd-dist-{PMD_VERSION}-bin.zip"
    );
    run(
        Command::new("curl")
            .args([
                "--fail",
                "--silent",
                "--show-error",
                "--location",
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                "--output",
            ])
            .arg(&zip)
            .arg(url),
        "download PMD",
    )
    .await?;
    verify_checksum(&zip, PMD_CHECKSUM).wrap_err("PMD archive verification failed")?;
    if cfg!(windows) {
        run(
            Command::new("tar.exe")
                .arg("-xf")
                .arg(&zip)
                .arg("-C")
                .arg(temporary.path()),
            "extract PMD",
        )
        .await?;
    } else {
        run(
            Command::new("unzip")
                .arg("-q")
                .arg(&zip)
                .arg("-d")
                .arg(temporary.path()),
            "extract PMD",
        )
        .await?;
    }
    let extracted = temporary.path().join(format!("pmd-bin-{PMD_VERSION}"));
    File::create(extracted.join(READY)).into_diagnostic()?;
    fs::rename(extracted, destination)
        .into_diagnostic()
        .wrap_err("Could not publish the verified PMD installation")
}

async fn find_python() -> Result<PathBuf> {
    let candidates = if let Some(python) = std::env::var_os("DORNGUARD_PYTHON") {
        vec![PathBuf::from(python)]
    } else {
        [
            "python3.14",
            "python3.13",
            "python3.12",
            "python3.11",
            "python3.10",
            "python3",
            "python",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect()
    };
    for python in candidates {
        let output = tokio::time::timeout(
            Duration::from_secs(5),
            Command::new(&python)
                .kill_on_drop(true)
                .args([
                    "-I",
                    "-c",
                    "import sys; sys.exit(not ((3, 10) <= sys.version_info[:2] <= (3, 14)))",
                ])
                .output(),
        )
        .await;
        if matches!(output, Ok(Ok(output)) if output.status.success()) {
            return Ok(python);
        }
    }
    bail!("Install Python 3.10–3.14 or set DORNGUARD_PYTHON to its executable");
}

fn semgrep_checksum() -> Option<&'static str> {
    match (
        std::env::consts::OS,
        std::env::consts::ARCH,
        cfg!(target_env = "musl"),
    ) {
        ("linux", "x86_64", false) => {
            Some("19a82868830389c039bb71bfc0aeebb28385e5b65740120710eff050324ad14f")
        }
        ("linux", "aarch64", false) => {
            Some("e20485948f87a7a8e5ef099bb0ef80beae8c5f53a19d55de94e89c516eb9f731")
        }
        ("linux", "x86_64", true) => {
            Some("6263233c3f2e7b05c2d89b1de7408e91c0238014ab67bb68746c0c452d33b828")
        }
        ("linux", "aarch64", true) => {
            Some("02cbad24b4409b5b85df11aea123f6cf74232858db570b1d28dce0e6109004a0")
        }
        ("macos", "aarch64", _) => {
            Some("675e69cbc71827a36eb490745d3d56c846c356b70072933c5513587a684767b0")
        }
        ("macos", "x86_64", _) => {
            Some("56f609a26dfba5e16428485f120f671e216ae3d3329697e8cf0075fb8a17c10a")
        }
        ("windows", "x86_64", _) => {
            Some("d1cc11b4662175d62c0296c6e4944483104a9ede26ae1a3c1723abab234815ce")
        }
        _ => None,
    }
}

pub(crate) fn verify_checksum(path: &Path, expected: &str) -> Result<()> {
    let mut file = File::open(path).into_diagnostic()?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let bytes = file.read(&mut buffer).into_diagnostic()?;
        if bytes == 0 {
            break;
        }
        digest.update(&buffer[..bytes]);
    }
    if format!("{:x}", digest.finalize()) != expected {
        bail!("Artifact failed SHA-256 verification");
    }
    Ok(())
}

async fn run(command: &mut Command, step: &str) -> Result<()> {
    command.stdin(Stdio::null()).kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(300), command.output())
        .await
        .into_diagnostic()
        .wrap_err_with(|| format!("Timed out attempting to {step}"))?
        .into_diagnostic()
        .wrap_err_with(|| format!("Could not {step}"))?;
    if !output.status.success() {
        bail!("Could not {step} ({})", output.status);
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn fake_semgrep(root: &Path) -> std::io::Result<PathBuf> {
        let directory = engine_directory(root, EngineName::Semgrep);
        fs::create_dir_all(directory.join("bin"))?;
        File::create(directory.join(READY))?;
        fs::write(directory.join("bin/semgrep"), "")?;
        Ok(directory)
    }

    #[test]
    fn semgrep_with_dangling_interpreter_is_not_installed() -> std::io::Result<()> {
        let root = tempfile::tempdir()?;
        let directory = fake_semgrep(root.path())?;
        std::os::unix::fs::symlink(
            root.path().join("missing-python"),
            directory.join("bin/python"),
        )?;
        assert!(managed_engine(root.path(), EngineName::Semgrep).is_none());
        Ok(())
    }

    #[test]
    fn semgrep_with_working_interpreter_is_installed() -> std::io::Result<()> {
        let root = tempfile::tempdir()?;
        let directory = fake_semgrep(root.path())?;
        fs::write(directory.join("bin/python"), "")?;
        assert!(managed_engine(root.path(), EngineName::Semgrep).is_some());
        Ok(())
    }
}

#[cfg(test)]
mod eslint_tests {
    use super::*;

    #[test]
    fn node_versions_follow_the_eslint_engine_range() {
        for supported in [
            "v20.19.0", "v20.20.1", "v22.13.0", "v22.20.0", "v24.0.0", "v26.1.0",
        ] {
            assert!(node_supported(supported), "{supported}");
        }
        for unsupported in [
            "v18.20.0", "v20.18.9", "v21.7.0", "v22.12.0", "v23.5.0", "", "node",
        ] {
            assert!(!node_supported(unsupported), "{unsupported}");
        }
    }

    #[test]
    fn pinned_lockfile_matches_the_install_version() {
        let package: serde_json::Value = serde_json::from_str(ESLINT_PACKAGE).unwrap_or_default();
        assert_eq!(
            package["dependencies"]["eslint"].as_str(),
            Some(ESLINT_INSTALL_VERSION)
        );
        let lock: serde_json::Value = serde_json::from_str(ESLINT_LOCK).unwrap_or_default();
        assert_eq!(
            lock["packages"]["node_modules/eslint"]["version"].as_str(),
            Some(ESLINT_INSTALL_VERSION)
        );
    }
}

#[cfg(test)]
mod osv_tests {
    use super::*;

    #[test]
    fn every_supported_platform_has_a_sha256() {
        for (os, arch) in [
            ("macos", "x86_64"),
            ("macos", "aarch64"),
            ("linux", "x86_64"),
            ("linux", "aarch64"),
            ("windows", "x86_64"),
            ("windows", "aarch64"),
        ] {
            let (asset, checksum) = osv_scanner_asset(os, arch).unwrap_or(("", ""));
            assert!(asset.starts_with("osv-scanner_"), "{os} {arch}");
            assert_eq!(checksum.len(), 64, "{os} {arch}");
            assert!(checksum.bytes().all(|byte| byte.is_ascii_hexdigit()));
        }
        assert!(osv_scanner_asset("freebsd", "x86_64").is_none());
    }

    #[test]
    fn a_download_with_the_wrong_checksum_is_rejected() -> std::io::Result<()> {
        let directory = tempfile::tempdir()?;
        let file = directory.path().join("osv-scanner");
        fs::write(&file, b"not the released binary")?;
        let (_, checksum) = osv_scanner_asset("linux", "x86_64").unwrap_or(("", ""));
        assert!(verify_checksum(&file, checksum).is_err());
        Ok(())
    }
}
