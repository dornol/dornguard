# Engine installation

DornGuard supports Semgrep 1.182.0, PMD 7.29.0 and an ArchUnit runner built
for Java 21. PMD uses the Java syntax level from the project build model or
`java_source` in the configuration, with Java 21 as the fallback. Gradle project
integration is tested with Gradle 9.9.1 and JDK 21. Java Maven integration
is tested with Maven 3.9.9 and JDK 21.

The ArchUnit runner needs Java 21 or later. DornGuard selects
`DORNGUARD_JAVA_HOME/bin/java` when `DORNGUARD_JAVA_HOME` is set, then
`JAVA_HOME/bin/java`, and otherwise uses `java` from `PATH`. Maven and Gradle
keep using `JAVA_HOME`, so a project that builds on an older JDK can still run
ArchUnit on a newer one. `dornguard doctor` reports this runtime separately from its source
engine checks. An unavailable optional ArchUnit runtime does not change the
doctor exit code for the source engines.

Maven and Gradle start through their usual launchers and follow the project's
build settings, including compiler toolchains. The compiler target may be
older than the engine runtime. A failed project model or compile operation
is a preparation error (exit 4). A failed analysis engine is an engine error
(exit 3). For an incomplete check, the text and agent formats show the
failure before collected diagnostics.
After a failed compile, DornGuard uses the discovery model to run available
source checks without the compile classpath. The report remains incomplete.

```sh
dornguard engines install
dornguard doctor
```

To install an individual engine, add `semgrep`, `pmd` or `eslint` to the
install command. `osv-scanner`, which `dornguard audit` uses, installs the same
way and downloads with curl (see [dependency audit](dependency-audit.md)). Semgrep requires Python 3.10 through 3.14. Set `DORNGUARD_PYTHON`
to select an executable. PMD requires JDK 21 and uses curl and unzip
(`tar.exe` on Windows) for managed installation.

For the optional `javascript` profile, install ESLint 9 or 10 in the
project or make it available on PATH, or run `dornguard engines install
eslint` to install ESLint 10.14.0 into DornGuard's engine directory. That
install needs Node.js 20.19, 22.13 or 24 or later and npm. It runs `npm ci
--ignore-scripts` against a lockfile bundled with DornGuard, so every package is
checked against its recorded integrity hash and no install script runs. A plain
`dornguard engines install` does not install ESLint, so Java-only projects
need no Node.js. A project or PATH installation takes precedence over the
managed one. See [web checks](web-checks.md) for the ESLint rules and the
independent CSP command.

TypeScript sources also need `@typescript-eslint/parser`. The managed install
includes `@typescript-eslint/parser` 8.72.2 and TypeScript 6.0.3. Otherwise
DornGuard uses the parser installed in the project, or the one installed beside
the ESLint on PATH (for example by a global `npm install -g`). If an
earlier DornGuard installed the managed ESLint, run `dornguard engines install
eslint` again to add the parser. Without a parser, a check that includes
TypeScript files reports an engine error.

The `jspecify` profile uses a Java parser bundled with DornGuard and runs
without a separate engine installation.

Managed installations use `DORNGUARD_ENGINE_DIR` when set and otherwise
use the operating system's local application data directory. The CLI looks
there first and then on PATH for the supported version.

The pinned Semgrep release supplies a wheel for Linux x86-64 and ARM64,
macOS ARM64 and Intel, and Windows x86-64. Semgrep 1.182 is the last upstream
release with Intel Mac wheels.

Release archives bundle the matching ArchUnit runner JAR. When building
DornGuard from source, point `DORNGUARD_ARCHUNIT_RUNNER` to a local JAR.
The same build creates the companion `maven-model.jar` alongside the runner.

```sh
examples/spring-boot-multimodule/gradlew -p runners/archunit-runner jar
export DORNGUARD_ARCHUNIT_RUNNER="$PWD/runners/archunit-runner/build/libs/archunit-runner.jar"
```
