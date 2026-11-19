# Releases

The [release workflow](../.github/workflows/release.yml) builds archives for
Linux x86-64 and ARM64, macOS ARM64 and Intel, and Windows x86-64. Each
archive includes the native CLI, the matching ArchUnit runner, and an
archive SHA-256 file. Each job extracts its archive and runs the bundled
CLI against the Gradle multi-module example using the packaged runner.
The workflow checks that a release tag matches
`workspace.package.version` in Cargo.toml.

The archive can be extracted to a directory on PATH. On Linux and macOS,
`scripts/install.sh vVERSION` installs a specific release to the user's
data directory and links the executable from the local bin directory.
On Windows, compare the archive digest with its accompanying SHA-256 file
using `Get-FileHash -Algorithm SHA256`, then extract it to a directory on PATH.

For local installation from source, use
`cargo install --locked --path crates/dornguard-cli`. See
[engine setup](engines.md) for the corresponding runner build.

Project configuration remains version 1. JSON reports use version 2.
Existing file diagnostics keep their relative path and position fields.
ArchUnit adds `module` and `class_name`, and `path` and `line` can be
`null` when the source is unresolved. An identified source can also have
a null line when its debug mapping is ambiguous. The matching ArchUnit
runner uses protocol 3. JSON consumers should accept report version 2
and handle logical-only findings. Exit codes retain their documented meanings.
For reproducible CI, pin the CLI archive,
the built-in Ruleset version or external Ruleset commit, and the engine versions.

## Verification status

The full integration suite passed with the pinned engines on macOS ARM64,
on Intel macOS binaries under Rosetta, and in Linux ARM64 and x86-64
containers. Windows x86-64 passed Rust's cross-target check. Native Windows
engine and Gradle execution remains a release-matrix check. A Windows
executable built with MinGW also returned the expected CLI version and
JSON failure codes under Wine, including for a project path with spaces.
The Rosetta environment stopped the Windows Semgrep and Java processes.
