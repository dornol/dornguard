use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use dornguard_core::EngineName;
use miette::{IntoDiagnostic, Result, WrapErr, bail, miette};
use sha2::{Digest, Sha256};
use tokio::process::Command;

use crate::{PMD_VERSION, SEMGREP_VERSION, supported_version};

const MANAGED: &str = ".dornguard-managed";
const READY: &str = ".dornguard-ready";
const PMD_CHECKSUM: &str = "d27177c185b837fa57f097e482d1253468aa035379c5eb2671fa0a279109b179";

pub async fn install(engine: EngineName) -> Result<PathBuf> {
    let root = install_root()
        .ok_or_else(|| miette!("Set DORNGUARD_ENGINE_DIR to a tool installation directory"))?;
    fs::create_dir_all(&root).into_diagnostic()?;
    let root = dunce::canonicalize(root).into_diagnostic()?;
    let _guard = tokio::task::spawn_blocking({
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

    let destination = engine_directory(&root, engine);
    if managed_engine(&root, engine).is_some() {
        return Ok(destination);
    }

    match engine {
        EngineName::Semgrep => install_semgrep(&root, &destination).await?,
        EngineName::Pmd => install_pmd(&root, &destination).await?,
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
        .ok_or_else(|| {
            miette!(
                "Could not find {}; run dornguard engines install {}",
                engine.executable(),
                engine.executable()
            )
        })?;
    Ok(Command::new(executable))
}

pub(crate) fn java_executable() -> PathBuf {
    let binary = if cfg!(windows) { "java.exe" } else { "java" };
    std::env::var_os("JAVA_HOME").map_or_else(
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
            "Could not start {}; ArchUnit requires Java 21 or later",
            executable.display()
        )
    })?;
    if !output.status.success() {
        bail!(
            "Java version check failed for {} ({}); ArchUnit requires Java 21 or later",
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
            "Java {version} at {}; ArchUnit requires Java 21 or later",
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
    root.join(format!(
        "{}-{}",
        engine.executable(),
        supported_version(engine)
    ))
}

fn managed_engine(root: &Path, engine: EngineName) -> Option<PathBuf> {
    let directory = engine_directory(root, engine);
    let path = match engine {
        EngineName::Semgrep if cfg!(windows) => directory.join("Scripts/semgrep.exe"),
        EngineName::Semgrep => directory.join("bin/semgrep"),
        EngineName::Pmd if cfg!(windows) => directory.join("bin/pmd.bat"),
        EngineName::Pmd => directory.join("bin/pmd"),
        _ => return None,
    };
    (directory.join(READY).is_file() && path.is_file()).then_some(path)
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
