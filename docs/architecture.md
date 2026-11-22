# Architecture checks

## How findings are counted

A diagnostic stands for one cause, not for each class dependency.

- `architecture.package-cycle` reports one diagnostic per package edge. The
  message gives the number of class dependencies and up to three examples.
- Forbidden-dependency rules, such as `architecture.library-framework-independence`,
  report one diagnostic per class that has a forbidden dependency. The message
  gives the number of forbidden types and up to three examples.

The location is the earliest one that has a file and a line. A fingerprint
identifies the package edge or the class, so a baseline or an `ignore` entry
with a fingerprint covers every dependency in that group, including ones added
later. A baseline or `ignore` entry for the group matches by rule and fingerprint, whichever file the group is reported in, and entries recorded for the individual dependencies before grouping still match.

## Gradle

The Gradle integration runs the project's Wrapper with a DornGuard init
script for Java and opted-in Kotlin/JVM `main` source sets. It collects project paths, source
directories, compiler outputs, classpaths and direct project dependencies.
When architecture rules apply, Gradle compiles the selected modules.
ArchUnit inspects main classes. Ordinary Java source rules also check the
Java directories in the Gradle `test` source set.

The ArchUnit runner receives a versioned JSON request with the classes to
inspect and the classpath to use for resolution. A dependency can refer to a
class from another module or library. The originating class is evaluated
in the module that owns it.

Use `--module :api` to check a single Gradle module. Use `--changed` to
compare the index and working tree with `HEAD`, including untracked,
non-ignored untracked source files in the selected languages. Tracked files
remain eligible even when they match a Git ignore pattern. Source engines check changed files. ArchUnit checks
affected modules and their reverse dependents. A change to build or policy
inputs or an unmapped path in a selected language triggers a full check.

For a pull request, use `dornguard check --changed --base origin/main`. It
adds the committed changes from the unique merge base to `HEAD` to the
index, working tree and untracked changes. Fetch the base ref and enough
history to resolve its merge base. DornGuard reports the reason and runs a
full check if the base ref or a unique merge base is unavailable. CI uses a
full check as its quality gate.

## Maven

For a Java Maven project, place `.dornguard.yml` beside the root `pom.xml`
and enable an architecture profile such as `layered` or `hexagonal`.
DornGuard uses `mvnw` (`mvnw.cmd` on Windows) when available and otherwise runs
Maven 3 from PATH. If both Gradle and Maven build files are present at the
root, DornGuard selects Gradle.

DornGuard runs Maven's `compile` phase for architecture checks and collects
the production source roots, class output directory and compile classpath
of each reactor module. The build uses the project's regular Maven profiles
and settings. Source rules check the reported Java production and test
source roots, including custom test directories. Build-aware checks also apply
project-wide source rules to eligible files outside module source roots, such
as Java scripts under `docs/scripts`. These files use source-only analysis.
Use `--module` to select a module's sources. Maven integration has been tested
with Maven 3.9.9 and JDK 21.

Java test sources use source-only analysis for import checks. Rules with
Java test exceptions, such as `spring.field-injection`, recognize the test
directories reported by the build.

Maven module keys in reports, `modules` settings and `--module` use
`groupId:artifactId`. A module check limits the report to that module
while the full reactor compiles. With `--changed`, Maven build checks fall
back to a full scan. Kotlin/JVM and `module_dependencies.forbid` use Gradle
integration.

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
