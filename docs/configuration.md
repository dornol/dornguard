# Configuration

DornGuard searches from the target directory to its ancestors for
`.dornguard.yml`. Use `--config PATH` to choose a specific file. See
the [configuration schema](../schemas/config.schema.json) and the [Gradle
example](../examples/spring-boot-multimodule/.dornguard.yml).

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
The module's rule fields override project fields, followed by profile defaults
and built-in values. Conflicting profile defaults require an explicit override.
Use `enabled: false` to disable an activated rule in a project or module. For
ArchUnit, `options` replaces the rule's package selectors. A single name
matches a package segment and a dotted name matches a package prefix.

## Java resource and exception checks

```yaml
version: 1
profiles: [java]
java_target: 17
allow_logged_exceptions: true
```

In build-backed checks, DornGuard reads the Java target from Maven's main
compiler settings or Gradle's main Java compile task. A configured release
takes precedence over a bytecode target. Set `java_target` for source-only
checks or to override the discovered target. With a known target below 21,
`java.resource-not-closed` excludes the JDK's `java.net.http.HttpClient`.
With an unknown target, the rule uses PMD's runtime analysis. JDBC resources
and project types named `HttpClient` remain subject to the rule.
An `instanceof` resource binding is treated as closed when the matching
branch begins with a direct call to that binding's `close()` method.

`allow_logged_exceptions` defaults to `false`. Set it to `true` to allow a
direct call to a resolved `java.util.logging.Logger` or `org.slf4j.Logger`
throwable overload immediately before a direct `throw` in the same `catch`
body. The log call must pass the original, unmodified catch parameter.
Conditional logging and calls that only pass its message still produce a
finding. Use a per-finding suppression for other intentional exceptions.

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
[formatting rules](profiles.md#source-formatting).

## Intentional exceptions

To exempt a Java source finding at a specific statement, put a standalone
comment immediately before the line the diagnostic reports. The comment
must name the rule and include a reason.

```java
// dornguard-ignore java.stdout-usage -- The calling script reads standard output.
System.out.println(result);
```

Each comment exempts one finding for the named rule on the next line.
Move the comment with the code.

To keep an exception in `.dornguard.yml`, copy the diagnostic's `path`,
`rule_id`, and `fingerprint` from `dornguard check --format json`. Use
the `rule` key for the rule ID and give a reason.

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
includes both exception forms in its suppressed count. Baseline creation
records findings that remain after these exceptions have been applied.

## Baseline

Run `dornguard baseline create` from the project root, review the result, then
run `dornguard check --baseline .dornguard-baseline.json`. Baselines match
the rule, relative file path and finding fingerprint. Findings with a
logical location use the Gradle module and JVM class instead of a file path.
The CLI reads version 1 file baselines and writes version 2. Running
`baseline create --force` replaces the file after the scan completes.
With `--changed`, DornGuard runs a full scan if the selected baseline changed
or Git cannot verify it as a tracked project file.

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
