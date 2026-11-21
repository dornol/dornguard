# Configuration

DornGuard searches from the target directory to its ancestors for
`.dornguard.yml`. Use `--config PATH` to choose a specific file. See
the [configuration schema](../schemas/config.schema.json) and the [Gradle
example](../examples/spring-boot-multimodule/.dornguard.yml) or the
[Maven module example](../schemas/examples/maven-layered.yml).

```yaml
version: 1
profiles: [java, security]
rules:
  java.stdout-usage: { severity: warning }
modules:
  ":api":
    profiles: [hexagonal, rest-api]
    rules:
      java.stdout-usage: { severity: error }
      architecture.domain-independence:
        options:
          from: [domain]
          forbidden: [adapter, infrastructure]
module_dependencies:
  forbid:
    - { from: ":api", to: ":admin" }
```

Project and module profiles are combined with profiles from `--profile`.
Use `dornguard check --only-profile format` to run formatting by itself.
The command still uses the project's format settings, rule overrides and
source exclusions. Repeat `--only-profile` to select multiple profiles.
Use the same option with `baseline create` to record that profile selection.
The module's rule fields override project fields, followed by profile defaults
and built-in values. Conflicting profile defaults require an explicit override.
Use `enabled: false` to disable an activated rule in a project or module. For
ArchUnit, `options` replaces the rule's package selectors. A single name
matches a package segment and a dotted name matches a package prefix.

## Java source, resource and exception checks

```yaml
version: 1
profiles: [java]
java_target: 17
allow_logged_exceptions: true
```

PMD uses the Java source version reported by the Maven or Gradle compiler
configuration, including separate settings for main and test sources. Java
21 is the fallback when the version is unknown, including source-only checks.
Set `java_source` to choose another Java feature version (3 through 27).
For example, `java_source: 25` selects Java 25 syntax in a source-only scan.

In build-backed checks, DornGuard reads the Java target from Maven's main
compiler settings or Gradle's main Java compile task. A configured release
takes precedence over a bytecode target. Set `java_target` for source-only
checks or to override the discovered target. With a known target below 21,
`java.resource-not-closed` excludes the JDK's `java.net.http.HttpClient`.
With an unknown target, the rule uses PMD's runtime analysis. JDBC resources
and project types named `HttpClient` remain subject to the rule.
With a known target below 19, the rule also excludes JDK `ExecutorService`.
For other targets, it recognizes `shutdown()` or `shutdownNow()` at the start
of the immediately following `finally` block when the executor is unchanged.
Returning the resource itself transfers ownership to the caller or to Spring
for an ordinary bean factory that returns the resource. For a project wrapper
or other ownership-taking API, list its fully qualified signature.

```yaml
java_resource_owners:
  - "example.JdbcHandle#new(java.sql.Connection)"
```

For a factory that lends a resource that Spring or another component will
close, register the factory under `java_managed_resource_factories`. Use this
only for methods whose callers never own the returned resource.

```yaml
java_managed_resource_factories:
  - "example.AppResources#connection()"
```

An ownership-taking call must take the resource as its sole argument and immediately follow
the local declaration. It may be a standalone method call or the expression
returned by the method. The rule still reports conditional transfers and
other paths where it cannot establish ownership. Configured signatures must
resolve in PMD. The report indicates when analysis uses the compiled project
classpath. Use `check --resolve-types` for compiled PMD type resolution on
main Java sources. `--resolve-imports` is an equivalent option.
An `instanceof` resource binding is treated as closed when the matching
branch begins with a direct call to that binding's `close()` method.

`allow_logged_exceptions` defaults to `false`. Set it to `true` to allow a
direct call to a resolved `java.util.logging.Logger` or `org.slf4j.Logger`
throwable overload before a direct `throw` in the same `catch`
body. The log call must pass the original, unmodified catch parameter.
Conditional logging and calls that only pass its message still produce a
finding. Use a per-finding suppression to document deliberate cause redaction.
Stack trace findings point to the `throw` statement when it can be located.

## Formatting

Add `format` to the existing profiles to require consistent formatting. The
settings below show the defaults. Include only the format fields you want to
override. Use `crlf` to require Windows line endings and set `max_blank_lines` to zero to
disallow blank lines.

```yaml
version: 1
profiles: [spring-boot, format]
format:
  java_indent: 4
  kotlin_indent: 4
  javascript_indent: 2
  line_ending: lf
  max_blank_lines: 1
```

Use the existing `rules` section to change the severity or disable individual
[formatting rules](profiles.md#source-formatting). Override selected settings
by file. The format profile skips `*.min.js`, `*.min.mjs`, and `*.min.cjs`
by default. Set `format.include_minified: true` to format-check them. Use
`format.exclude` to skip only formatting for more paths.

```yaml
format:
  exclude: ["src/main/resources/static/vendor/**"]
  overrides:
    - files: ["src/main/resources/static/legacy/**/*.js"]
      indent_style: tabs
      control_spacing: none
```

The default `indent_style` is `spaces`. With `tabs`, each block level uses
one tab. `control_spacing` accepts `space` for `if (...)` and `none` for
`if(...)`. Both options can also be set at the top level of `format`.

Use forward slashes. A single `*` stays within one path segment and `**` can
span directories. The last matching override wins for each field it sets.
Top-level `exclude` applies to source checks. Architecture and migration checks still
use their build and Git scopes, respectively. A tracked source file remains
eligible even if it matches `.gitignore`.

Malformed Java, Kotlin, or JavaScript can leave syntax-dependent formatting
rules incomplete. The report lists the affected rules and retains line-ending
and final-newline checks.

## Intentional exceptions

Use `ignore` to suppress results for a file or directory. A trailing slash
matches all files under a directory, including nested directories. A glob
such as `legacy/**/*.java` also works. Include `rule` to suppress only
findings for that rule. Use a quoted pattern such as `"format.*"` to
suppress a rule family and keep other findings visible. Omitting `rule`
suppresses findings from all rules at that path. Each entry requires a reason.
Fingerprint entries use an exact rule ID.

```yaml
ignore:
  - path: src/main/resources/static/vendor/
    reason: "Third-party files maintained upstream."
  - path: src/main/resources/static/legacy/**
    rule: "format.*"
    reason: "Preserve formatting until these files are migrated."
  - path: docs/scripts/JasyptTool.java
    rule: java.stdout-usage
    reason: "The calling script reads standard output."
```

`ignore` applies to findings with a file path. DornGuard still analyzes the
files and reports analysis failures. Use `exclude` when source checks
should skip the files altogether. Organization policy checks run
independently of local ignores.

To exempt a Java source finding at a specific statement, put a standalone
comment immediately before the line the diagnostic reports. The comment
must name the rule and include a reason.

```java
// dornguard-ignore java.stdout-usage -- The calling script reads standard output.
System.out.println(result);
```

Each comment exempts one finding for the named rule on the next line.
Move the comment with the code. HTML and Thymeleaf accept the same directive
in a standalone HTML comment.

```html
<!-- dornguard-ignore thymeleaf.unescaped-output -- Reviewed server-side sanitization -->
<div th:utext="${reviewedHtml}"></div>
```

The directive suppresses findings. A file the engine partially parsed still
appears in the failures list with an incomplete result.

For Thymeleaf, explicitly register a bean method that your application uses
to sanitize HTML. This allows a whole-value call in `th:utext` or a single
`th:with` binding used by `th:utext` in the same lexical scope.

```yaml
thymeleaf_sanitizers: ["@htmlSanitizer.clean"]
```

```html
<section th:with="safe=${@htmlSanitizer.clean(body)}">
  <div th:utext="${safe}"></div>
</section>
```

Calls with simple variable or property arguments are supported. Concatenation,
shadowed variables and unsupported scope changes still produce findings.
The compatibility rules for restricted bean access still run.

To suppress one specific occurrence in `.dornguard.yml`, copy the diagnostic's `path`,
`rule_id`, and `fingerprint` from `dornguard check --format json`. Use
the `rule` key for the rule ID and give a reason. Fingerprint entries match
exact file paths.

```yaml
ignore:
  - path: docs/scripts/JasyptTool.java
    rule: java.stdout-usage
    fingerprint: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    reason: "The calling script reads standard output."
```

Use a project-relative path with forward slashes. Replace the example
fingerprint with the reported value. Each entry exempts one matching
finding when the code moves within the file. For identical code in the
same file, use a comment to exempt a specific occurrence. The report
includes all `ignore` entries and source directives in its suppressed count.
A complete full scan also lists suppressions that matched no finding, such as an
`ignore` entry that names a rule or a `dornguard-ignore` comment, so you can
remove them when the code no longer needs them. Path-only entries, which are
broad exclusions, are not listed. Baseline creation
records findings that remain after these exceptions have been applied.

Suppressions for rules that are not active in a run, such as with
`--only-profile`, are not reported as obsolete.

## Incomplete analysis of valid code

When Semgrep's parser stops partway through a valid file, the check ends with
an engine error. Report such a file as incomplete analysis instead, with a
reason that explains why the parser is wrong.

```yaml
partial_parsing_files:
  - path: src/main/java/example/ColumnStyleConfig.java
    reason: Semgrep cannot parse a type-use annotation on varargs.
```

Use `partial_parsing: warning` to apply this to every file. Prefer the file
list, because it keeps other parse failures visible. The report still has
`complete: false` and lists the file in `analysis_warnings`. The status is
`incomplete` with exit code 0 when there are no blocking findings or other
failures. Baseline creation requires complete analysis. Organization policy runs ignore
these settings. `thymeleaf_partial_parsing` remains available for templates.

## Baseline

Run `dornguard baseline create` from the project root, review the result, then
run `dornguard check --baseline .dornguard-baseline.json`. Baselines match
the rule, relative file path and finding fingerprint. Findings with a
logical location use the Gradle module and JVM class instead of a file path.
The CLI reads version 1 file baselines and writes version 2. Running
`baseline create --force` replaces the file after the scan completes.
Use `baseline create --resolve-imports` when your check uses
`--resolve-imports` so both commands use the compiled classpath.
New PMD fingerprints use the displayed finding range. DornGuard also
matches earlier PMD fingerprints for existing baseline and ignore entries.
With `--changed`, DornGuard runs a full scan if the selected baseline changed
or Git cannot verify it as a tracked project file.

A baseline hides existing findings, so two options keep the debt shrinking:

- `--strict-changed` (with `--changed`) reports baselined findings in the
  files that the run selected. Whoever edits a file also clears its legacy
  findings; untouched files stay hidden.
- `--ratchet` fails a complete full scan when the baseline records findings
  that no longer occur. Without it, the report only notes the count. Run
  `dornguard baseline create --force` and commit the smaller file, so the
  baseline can only shrink.

## Organization policy

From a trusted CI job, run
`dornguard check --organization-policy /trusted/organization.yml`.
Use the same configuration schema for the organization file. Supply it and
its local ruleset from outside the checkout. An organization Git ruleset
requires HTTPS and a full commit ID.

Organization rules run over the full project with blocking severity.
Baselines, source comments, YAML exceptions, rule overrides, and CLI scope
from the checkout apply only to the project scan. Gradle still uses the project's build model,
so protect updates to the build configuration and use isolated CI workers.
