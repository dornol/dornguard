//! Embeds every built-in profile, rule and engine configuration under `assets`.
//!
//! Adding a rule needs only its files under `assets`; the lists are generated.

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

fn main() -> io::Result<()> {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("set by Cargo"));
    let assets = manifest.join("assets");
    println!("cargo:rerun-if-changed=assets");

    let profiles = files(&assets.join("profiles"), false)?;
    let rules = files(&assets.join("rules"), false)?;
    let mut engine_configs = Vec::new();
    for engine in ["pmd", "semgrep"] {
        engine_configs.extend(files(&assets.join("rules").join(engine), true)?);
    }

    let mut out = String::new();
    write_list(&mut out, "BUILTIN_PROFILES", &profiles);
    write_list(&mut out, "BUILTIN_RULES", &rules);
    out.push_str("const BUILTIN_ENGINE_CONFIGS: &[(&str, &str)] = &[\n");
    for path in &engine_configs {
        let relative = relative(&assets, path);
        writeln!(
            out,
            "    ({relative:?}, include_str!({:?})),",
            path.display()
        )
        .expect("writing to a String cannot fail");
    }
    out.push_str("];\n");

    let target = PathBuf::from(std::env::var_os("OUT_DIR").expect("set by Cargo"));
    fs::write(target.join("builtin_assets.rs"), out)
}

/// Sorted asset files directly in `directory`, or below it when `recursive`.
fn files(directory: &Path, recursive: bool) -> io::Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            if recursive {
                found.extend(files(&path, true)?);
            }
        } else if matches!(
            path.extension().and_then(|extension| extension.to_str()),
            Some("yml" | "xml")
        ) {
            found.push(path);
        }
    }
    found.sort();
    Ok(found)
}

fn relative(assets: &Path, path: &Path) -> String {
    path.strip_prefix(assets)
        .expect("asset below the assets directory")
        .components()
        .map(|part| part.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn write_list(out: &mut String, name: &str, paths: &[PathBuf]) {
    writeln!(out, "const {name}: &[&str] = &[").expect("writing to a String cannot fail");
    for path in paths {
        writeln!(out, "    include_str!({:?}),", path.display())
            .expect("writing to a String cannot fail");
    }
    out.push_str("];\n");
}
