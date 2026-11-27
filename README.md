# DornGuard

DornGuard turns recurring code review findings into static checks for AI
coding agents. Agents can check their changes, fix the reported issues, and
rerun the checks before handing off. Running the same project policy in CI
keeps a consistent minimum standard for code quality, including for changes
that receive no manual review.

DornGuard applies quality, security, and architecture policies to Java,
Spring Boot, and Kotlin/JVM projects. It runs Semgrep, PMD, and ArchUnit
through one CLI with project and module profiles. The same CLI checks HTML,
JSP and Thymeleaf templates, JavaScript and TypeScript with ESLint, Go with
`go vet`, and Rust with `cargo clippy`, and audits Maven dependencies with
OSV-Scanner. See the [rule catalog](docs/rules.md) for every built-in rule.

## Install or update on Ubuntu and WSL

DornGuard v0.0.8 publishes Linux x86-64 (AMD64) and macOS ARM64 archives.
Run these commands in x86-64 Ubuntu, including Ubuntu on WSL. They
install DornGuard for the first time or update an existing `install.sh`
installation. The installer verifies the archive checksum, links the new
version, and installs missing dependencies and engines with `--with-deps`.

```sh
curl -fsSLo /tmp/dornguard-install.sh https://raw.githubusercontent.com/dornol/dornguard/v0.0.8/scripts/install.sh &&
  sh /tmp/dornguard-install.sh v0.0.8 --with-deps &&
  "$HOME/.local/bin/dornguard" version
```

The version command should print `dornguard 0.0.8 (built-in ruleset 1.6.0)`.
Use `"1.6.0"` if `.dornguard.yml` pins the built-in `ruleset.version`.
Keep the top-level `version: 1`.

## Install or update on macOS

The macOS archive is built for Apple silicon (ARM64). `--with-deps` is
Ubuntu-only, so install the prerequisites first, then the release, then the
engines. You need Python 3.10–3.14 and JDK 21 or later (for example from
Homebrew: `brew install python@3.13 openjdk@21`).

```sh
curl -fsSLo /tmp/dornguard-install.sh https://raw.githubusercontent.com/dornol/dornguard/v0.0.8/scripts/install.sh &&
  sh /tmp/dornguard-install.sh v0.0.8
export PATH="$HOME/.local/bin:$PATH"
export JAVA_HOME="$(/usr/libexec/java_home -v 21)"
dornguard engines install
dornguard doctor
```

Add the `PATH` and `JAVA_HOME` exports to your shell profile. Without
`--with-deps` the installer only downloads and links the release, so run
`dornguard engines install` yourself to install Semgrep and PMD.

See the [release guide](docs/releases.md) for details and the
[v0.0.8 release notes](docs/releases/v0.0.8.md) for upgrade details.

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
Projects can allow partial Thymeleaf parsing with exit code 0 and an incomplete
report. See [Thymeleaf parsing settings](docs/web-checks.md#partial-thymeleaf-parsing).
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

The `go` and `rust` profiles run `go vet` and `cargo clippy` from the
project's own toolchain and report their findings like any other rule.
See [Go and Rust](docs/go-rust.md).

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
