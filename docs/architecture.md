# Architecture checks

The Gradle integration runs the project's Wrapper with a DornGuard init
script for Java and opted-in Kotlin/JVM `main` source sets. It collects project paths, source
directories, compiler outputs, classpaths and direct project dependencies.
When architecture rules apply, Gradle compiles the selected modules.

The ArchUnit runner receives a versioned JSON request with the classes to
inspect and the classpath to use for resolution. A dependency can refer to a
class from another module or library. The originating class is evaluated
in the module that owns it.

Use `--module :api` to check a single Gradle module. Use `--changed` to
compare the index and working tree with `HEAD`, including untracked,
non-ignored source files in the selected languages. Source engines check changed files. ArchUnit checks
affected modules and their reverse dependents. A change to build or policy
inputs or an unmapped path in a selected language triggers a full check.

For a pull request, use `dornguard check --changed --base origin/main`. It
adds the committed changes from the unique merge base to `HEAD` to the
index, working tree and untracked changes. Fetch the base ref and enough
history to resolve its merge base. DornGuard reports the reason and runs a
full check if the base ref or a unique merge base is unavailable. CI uses a
full check as its quality gate.

## Kotlin/JVM

Apply the Kotlin JVM Gradle plugin and enable the `kotlin` profile at the
project or module level. Kotlin-only modules can use `profiles: [kotlin]`.
Mixed modules can use `profiles: [java, kotlin]`. Add `hexagonal` or
`layered` to analyze JVM dependencies with ArchUnit.

```sh
dornguard check examples/kotlin-single
dornguard check examples/kotlin-multimodule --format sarif
```

The [Kotlin-only example](../examples/kotlin-single) uses Semgrep.
The [mixed example](../examples/kotlin-multimodule) adds PMD for Java and
ArchUnit for JVM dependencies. Both examples pin Kotlin Gradle plugin
2.4.21, Gradle 9.7 and JDK 21.
[Kotlin Gradle compatibility](https://kotlinlang.org/docs/gradle-configure-project.html)

DornGuard uses the registered `main` source directories, including custom
roots and directories shared by Java and Kotlin. PMD receives Java files
only. Android, Kotlin Multiplatform, scripts and test source sets are outside
the current scope.

ArchUnit can report a file location even when a Kotlin file's directory
and package differ. It reports a logical class location with its Gradle
module when the source file cannot be selected uniquely. A physical file
location may omit a line for Kotlin classes with source debug mappings.

`--changed` and `--changed --base` include Kotlin files in modules that
enable Kotlin. Deletions and renames can trigger ArchUnit for the old
module and modules that depend on it. An unmapped Kotlin change triggers
a full scan when Kotlin is enabled.
