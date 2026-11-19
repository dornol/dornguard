# DornGuard

DornGuard applies quality, security, and architecture policies to Java,
Spring Boot, and Kotlin/JVM projects through one CLI. It runs Semgrep, PMD, and ArchUnit with
project-specific and Gradle module-specific profiles.

## Run the Java example

With Rust 1.89 or newer and Python 3.10–3.14 installed, run

```sh
cargo run --locked -- engines install semgrep
cargo run --locked -- check examples/java-single --format json
```

For a new Java project, run `dornguard init` from its root and install both
source engines with `dornguard engines install`. Install JDK 21 to use PMD
and Gradle. See [engine setup](docs/engines.md) for the Gradle runner and
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
dornguard rules explain java.stdout-usage
dornguard doctor
```

The check exit codes are 0 for success, 1 for a policy violation, 2 for
configuration errors, 3 for engine errors, and 4 for preparation errors.
JSON reports identify the scan scope and whether the checks completed.

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
