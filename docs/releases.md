# Releases

See the [v0.0.7 release notes](releases/v0.0.7.md) for upgrade details.

## Unreleased

- `format.indentation` is off by default. Enable it with
  `rules: { format.indentation: { enabled: true } }`.
- The `format.*` rules skip test sources like other rules. Set
  `skip_java_tests: false` on a rule under `rules` to check them. The key is now
  accepted for every rule.
- `partial_parsing` and `partial_parsing_files` report valid files that a parser
  reads only partly as incomplete analysis instead of an engine error.
- Architecture findings count one cause each: a package edge for
  `architecture.package-cycle`, a class for forbidden dependencies, and a package
  for `java.jspecify-null-marked` when no `@NullMarked` scope exists.
  Fingerprints from earlier releases still match.
- `java.unnecessary-import` no longer reports an import whose type the file
  writes in full. `java.fully-qualified-type` asks for the simple name instead.
- `java.empty-catch` allows a block that has a comment.
- Bundled runner files are found when the executable is installed through a symlink.

The [release workflow](../.github/workflows/release.yml) builds
archives for Linux x86-64 (AMD64) and macOS ARM64. Each
archive includes the native CLI, the ArchUnit runner and the Maven model
extension, and is published with a SHA-256 file. Each job extracts its
archive and runs the bundled CLI against the Gradle multi-module example using the packaged runner.
The workflow checks that a release tag matches
`workspace.package.version` in Cargo.toml.

The archive can be extracted to a directory on PATH. On Linux x86-64 and Apple silicon Macs,
`scripts/install.sh vVERSION` installs a specific release to the user's
data directory and links the executable from the local bin directory.
This mode checks for `curl`, `unzip`, and `shasum` when downloading a release.

On x86-64 Ubuntu, including Ubuntu on WSL, add `--with-deps` to install missing
system packages through apt and install Semgrep and PMD as the current user.
The script reuses a supported Python with `venv` and `ensurepip` and an
existing JDK 21 or later. It ends by running `dornguard doctor`.

```sh
sh scripts/install.sh v0.0.7 --with-deps
export PATH="$HOME/.local/bin:$PATH"
```

For local installation from source, use
`cargo install --locked --path crates/dornguard-cli`. See
[engine setup](engines.md) for the corresponding runner build.

## Report format

DornGuard v0.0.3 through v0.0.7 emit JSON check report version 3. The v0.0.2
archive emits version 2. Project configuration remains version 1.

Version 3 uses one-based lines and UTF-16 columns in text, JSON, and SARIF.
JSON adds optional `end_line`, `end_column`, and `subject`. The end position
is exclusive. SARIF declares `columnKind: utf16CodeUnits` and records the
same ranges. ArchUnit can report a logical location with `module` and
`class_name` when the source is unresolved, or a path with a null line
when the debug mapping is ambiguous.

Check reports still expose `complete` and `exit_code`. Failures can include
`stage`, `build_tool`, `file_count`, `modules`, `rule_ids` and the
file's `path` and `error_type` when supplied by the engine. `files_requested`
counts unique files sent for analysis. `files_scanned` counts the files
that completed every requested analysis for the file. A failed invocation
can contain diagnostics from the
engines that produced results. Exit codes retain their documented meanings.

Version 0.0.4 adds `rule_counts` with error and warning counts by rule and
optional `analysis_notes` describing PMD classpath use. When organization
policy runs, reports include `project_scope` and `organization_scope`. The
combined `scope` is full. SARIF exposes the same scopes as `projectScope`
and `organizationScope`. ArchUnit
reports one finding per module, rule, origin class and forbidden target class.
Existing dependency fingerprints still match the grouped finding.
The v0.0.6 and v0.0.7 runners use protocol 5, which adds the `cycles` rule option for
`architecture.package-cycle`. The v0.0.4 and v0.0.5 runners use protocol 4.
Version 0.0.7 adds `confidence` to each diagnostic.
Install the matching CLI and runner together.
The v0.0.3 runner uses protocol 3.
A compiler-skipped input or a missing Java bytecode mapping is reported as
incomplete. Maven checks also reject class files older than the matched
source. Gradle checks use the compile tasks' up-to-date and cache state.

JSON findings can include an optional `edit` with UTF-8 byte offsets,
replacement text, and the input file's SHA-256 digest. When result filters are
used, JSON adds `report_filter` with the filters and omitted finding count.
Rule counts and exit codes cover the original check.

Version 0.0.5 adds `pmd_coverage.compiled_classpath` and
`pmd_coverage.source_only` to list the files PMD scanned in each mode.
Tolerated partial Thymeleaf parsing appears in `analysis_warnings` and sets
`complete` to false. When there are no blocking findings or other failures,
this produces `status: incomplete` with `exit_code: 0`.

For reproducible CI, pin the CLI archive,
the built-in Ruleset version or external Ruleset commit, and the engine versions.

## Release verification

1. Push the version bump to the default branch. Run `gh workflow run release.yml --ref main`
   and wait for both platform jobs to pass. Each job runs the integration suite
   with the pinned engines, then extracts and smoke-tests its release archive.
2. Create and push the version tag on that exact commit. The workflow checks
   the tag against Cargo.toml and reuses both archives from a successful
   manually triggered run on the default branch with the same commit SHA.
3. The publish job verifies both SHA-256 files and uploads the four assets.
   If a matching run or either package is missing or expired, the tag workflow
   builds and verifies both packages before publishing.
