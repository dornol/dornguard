# DornGuard

DornGuard turns recurring code review findings into static checks for AI
coding agents. Agents can check their changes, fix the reported issues, and
rerun the checks before handing off. Running the same project policy in CI
keeps a consistent minimum standard for code quality, including for changes
that receive no manual review.

DornGuard applies quality, security, and architecture policies to Java,
Spring Boot, and Kotlin/JVM projects. It runs Semgrep, PMD, and ArchUnit
through one CLI with project and module profiles.

## Install or update on Ubuntu and WSL

Run these commands in Ubuntu, including Ubuntu on WSL. The same commands
install DornGuard for the first time or update an existing `install.sh`
installation. The installer verifies the archive checksum, links the new
version, and installs missing dependencies and engines with `--with-deps`.

```sh
curl -fsSLo /tmp/dornguard-install.sh https://raw.githubusercontent.com/dornol/dornguard/v0.0.3/scripts/install.sh &&
  sh /tmp/dornguard-install.sh v0.0.3 --with-deps &&
  "$HOME/.local/bin/dornguard" version
```

The version command should print `dornguard 0.0.3 (built-in ruleset 1.2.0)`.
If `.dornguard.yml` pins `ruleset.version` to `"1.1.0"`, update the pin to
`"1.2.0"`. Keep the top-level `version: 1`.

See the [release guide](docs/releases.md) for other platforms and the
[v0.0.3 release notes](docs/releases/v0.0.3.md) for upgrade details.

## Run the Java example

With Rust 1.89 or newer and Python 3.10–3.14 installed, run

```sh
cargo run --locked -- engines install semgrep
cargo run --locked -- check examples/java-single --format json
```

For a new Java project, run `dornguard init` from its root and install both
source engines with `dornguard engines install`. Use JDK 21 or later for the
ArchUnit runner and run `dornguard doctor` to see the Java executable it selects.
See [engine setup](docs/engines.md) for build and engine requirements and
[the multi-module example](examples/spring-boot-multimodule).

For Gradle Kotlin/JVM, enable `profiles: [kotlin]` in `.dornguard.yml`. With
JDK 21 and Semgrep installed, run
`cargo run --locked -- check examples/kotlin-single --format json`.
See the [Kotlin/JVM guide](docs/architecture.md#kotlinjvm) for mixed modules
and architecture checks.

For Java projects that use Maven, place `.dornguard.yml` beside the root
`pom.xml` and add `layered` or `hexagonal` to its profiles for bytecode checks.
See [Maven architecture checks](docs/architecture.md#maven).

## Common commands

```sh
dornguard check
dornguard check --changed --format agent
dornguard check --changed --base origin/main
dornguard check --module :api
dornguard check --format sarif
dornguard check --only-profile format
dornguard check --changed --format json --report-rule java.stdout-usage
dornguard rules explain java.stdout-usage
dornguard doctor
```

The check exit codes are 0 for success, 1 for a policy violation, 2 for
configuration errors, 3 for engine errors, and 4 for preparation errors.
Check reports include per-rule counts and identify incomplete analysis.
In a configured project, `dornguard doctor` checks the engines selected by its
profiles, including the ArchUnit runner protocol when architecture checks apply.
JSON includes the affected rules and available file or module information for
each failure. See the
[report format](docs/releases.md#report-format) for location and range fields.
See [CI quality gate](docs/ci.md) for using a baseline and applying suggested
edits.

Use `dornguard audit` for a Maven dependency vulnerability report.
See [Maven dependency audit](docs/dependency-audit.md) for setup and scope.

The `html` and `javascript` profiles check inline scripts and JavaScript
evaluation. Use `dornguard csp URL` to inspect an HTTP response.
See [HTML, JavaScript, and CSP](docs/web-checks.md).

## Verify

```sh
cargo test --workspace --locked
```

To run the real-engine integration tests, build the ArchUnit runner, set
`DORNGUARD_ARCHUNIT_RUNNER`, install the pinned source engines and make Maven 3
available on PATH. Then run `cargo test --workspace --locked -- --include-ignored`.

See [configuration](docs/configuration.md), [policy bundles](docs/profiles.md),
[CI integrations](docs/ci.md), [release policy](docs/releases.md) and
[architecture checks](docs/architecture.md).
