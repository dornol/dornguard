# Engine installation

DornGuard supports Semgrep 1.182.0, PMD 7.29.0 and an ArchUnit runner built
for Java 21. Java source analysis uses Java 21 syntax in PMD. Gradle project
integration is tested with Gradle 9.9.1 and JDK 21. Java Maven integration
is tested with Maven 3.9.9 and JDK 21.

```sh
dornguard engines install
dornguard doctor
```

To install an individual engine, add `semgrep` or `pmd` to the install
command. Semgrep requires Python 3.10 through 3.14. Set `DORNGUARD_PYTHON`
to select an executable. PMD requires JDK 21 and uses curl and unzip
(`tar.exe` on Windows) for managed installation.

For the optional `javascript` profile, install ESLint 9 or 10 in the
project or make it available on PATH. See [web checks](web-checks.md) for
the ESLint rules and the independent CSP command.

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
