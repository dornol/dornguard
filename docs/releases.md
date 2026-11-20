# Releases

See the [v0.0.3 release notes](releases/v0.0.3.md) for upgrade details.

The [release workflow](../.github/workflows/release.yml) builds archives for
Linux x86-64 and ARM64, macOS ARM64, and Windows x86-64. Each
archive includes the native CLI, the ArchUnit runner and the Maven model
extension, and is published with a SHA-256 file. Each job extracts its
archive and runs the bundled CLI against the Gradle multi-module example using the packaged runner.
The workflow checks that a release tag matches
`workspace.package.version` in Cargo.toml.

The archive can be extracted to a directory on PATH. On Linux and Apple silicon Macs,
`scripts/install.sh vVERSION` installs a specific release to the user's
data directory and links the executable from the local bin directory.
This mode checks for `curl`, `unzip`, and `shasum` when downloading a release.

On Ubuntu, including Ubuntu on WSL, add `--with-deps` to install missing
system packages through apt and install Semgrep and PMD as the current user.
The script reuses a supported Python with `venv` and `ensurepip` and an
existing JDK 21 or later. It ends by running `dornguard doctor`.

```sh
sh scripts/install.sh v0.0.3 --with-deps
export PATH="$HOME/.local/bin:$PATH"
```

On Windows, compare the archive digest with its accompanying SHA-256 file
using `Get-FileHash -Algorithm SHA256`, then extract it to a directory on PATH.

For local installation from source, use
`cargo install --locked --path crates/dornguard-cli`. See
[engine setup](engines.md) for the corresponding runner build.

## Report format

DornGuard v0.0.3 emits JSON check report version 3. The v0.0.2 archive emits
version 2. Project configuration remains version 1.

Version 3 uses one-based lines and UTF-16 columns in text, JSON, and SARIF.
JSON adds optional `end_line`, `end_column`, and `subject`. The end position
is exclusive. SARIF declares `columnKind: utf16CodeUnits` and records the
same ranges. ArchUnit can report a logical location with `module` and
`class_name` when the source is unresolved, or a path with a null line
when the debug mapping is ambiguous. The runner uses protocol 3.

Check reports still expose `complete` and `exit_code`. Failures can include
`stage`, `build_tool`, `file_count`, `modules`, `rule_ids` and the
file's `path` and `error_type` when supplied by the engine. `files_requested`
counts unique files sent for analysis. `files_scanned` counts the files
that completed every requested analysis for the file. A failed invocation
can contain diagnostics from the
engines that produced results. Exit codes retain their documented meanings.

The development version adds `rule_counts` with error and warning counts by
rule, and optional `analysis_notes` describing PMD classpath use. ArchUnit
reports one finding per module, rule, origin class and forbidden target class.
Existing dependency fingerprints still match the grouped finding.
The current runner uses protocol 4 and must be installed with the matching CLI.
A compiler-skipped input or a missing Java bytecode mapping is reported as
incomplete. Maven checks also reject class files older than the matched
source. Gradle checks use the compile tasks' up-to-date and cache state.

For reproducible CI, pin the CLI archive,
the built-in Ruleset version or external Ruleset commit, and the engine versions.

## Release verification

Run the release workflow on the candidate commit to verify the four platform
packages. It runs the pinned engines and the integration suite, then extracts
and smoke-tests each archive. Use the Windows workflow result to verify native
Windows engine and Gradle execution.
