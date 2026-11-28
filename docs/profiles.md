# Profiles and rulesets

A profile is a named set of rules. List profiles in `.dornguard.yml`, add one
for a single run with `--profile NAME`, or run only some with
`--only-profile NAME`. `dornguard profiles show NAME` lists a profile's rules.
What each rule reports, with an example and when it may be suppressed, is in
`dornguard rules explain RULE...` and the generated [rule catalog](rules.md).

| Profile | Use it for |
| --- | --- |
| `java-source` | Java with Semgrep only |
| `java` | Java with Semgrep and PMD (extends `java-source`); the `init` default |
| `spring-boot` | Spring Boot applications (extends `java`) |
| `rest-api` | Spring API modules (`spring-boot` and `security`) |
| `scheduler` | Spring scheduler modules |
| `security` | Web security checks for Java |
| `kotlin` | Gradle Kotlin/JVM modules |
| `java-library` | Java libraries that must not use Spring |
| `layered`, `hexagonal`, `package-cycles` | Architecture checks with ArchUnit; need a Maven or Gradle build |
| `format` | Source formatting for Java, Kotlin and JavaScript |
| `jspecify` | Requiring JSpecify `@NullMarked` coverage |
| `html`, `javascript`, `jsp`, `thymeleaf` | Web sources; see [web checks](web-checks.md) |
| `go`, `rust` | Go with `go vet`, Rust with `cargo clippy`; see [Go and Rust](go-rust.md) |
| `java-review`, `java-design`, `kotlin-review`, `spring-rollback`, `jpa-web`, `thymeleaf-output-review`, `flyway` | Opt-in reviews; start them as warnings and measure the findings first |
| `typescript-review` | Opt-in TypeScript checks with type information; needs a `tsconfig.json` and the project's installed dependencies |

Rules skip test sources, any file below a `src/test` directory, by default.
`java.assert-throws-nested-call` still checks them. Set
`rules: { RULE: { skip_java_tests: false } }` to check test sources with
another rule.

## Rule confidence

Severity decides whether a finding fails the check. Confidence says how
reliably it is a real problem, and every diagnostic carries it.

- `high`: the code is wrong by policy or by construction. Fix it.
- `medium`: the rule infers intent from code shape, so correct code can match.
  Confirm the finding, then fix it or suppress it with a reason. Text and
  agent reports add a `Review:` line to these findings.

A custom rule defaults to `high`; set `confidence: medium` in its rule file to
make it a review rule.

## Source formatting

`format.indentation` is off by default, because a formatter such as Spotless,
google-java-format, ktlint or Prettier lays out blocks better than a checker.
Enable it with `rules: { format.indentation: { enabled: true } }`. See
[formatting configuration](configuration.md#formatting) for widths, line
endings, overrides and excluded files.

## Profile notes

- **jspecify**: add `org.jspecify:jspecify` to the build and a
  `package-info.java` with `@NullMarked` per package. A change to a
  `package-info.java` or `module-info.java` makes `--changed` scan every file.
- **java**: unused wildcard imports are found only when PMD gets the module's
  compiled classpath. DornGuard reuses it when another check compiles the
  module; `dornguard check --resolve-imports` compiles for this rule alone.
  With Maven, that build check covers the whole reactor even with `--changed`.
- **jpa-web**: with `--changed`, a changed Java file that mentions
  `persistence` or can no longer be read makes the check scan every file,
  because an entity change can affect an unchanged controller.
- **flyway**: enable it at project scope and run
  `dornguard check --changed --base origin/main` in a pull request. Without
  `--base` the comparison uses `HEAD`. Git history is required.
- **thymeleaf**: scans `.html` and `.htm` files under
  `src/main/resources/templates`, for Thymeleaf 3.1.4 and later.

## Reuse a policy bundle

A policy bundle has a `ruleset.yml` manifest. The manifest names files
relative to the bundle directory.

```yaml
version: 1
profiles: [profiles/team-java.yml]
rules: [rules/team.log-stderr.yml]
```

Profiles declare `name`, `description`, `extends`, `rules`, and optional
`rule_defaults` using the fields in the configuration schema. Custom rule
metadata binds one Semgrep pattern rule to its engine file or provides
package selectors for ArchUnit, and can carry the same `guidance` block as
the built-in rules so that `rules explain` shows it. Rule IDs, engine rule IDs
and profile names must be unique. Declare the rule's source languages in its
Semgrep YAML. A rule targeting Kotlin activates Gradle Kotlin/JVM source
discovery for its policy scope.

Set `ruleset: { source: local, path: .dornguard-policy }` to load a bundle
inside the project, or use a versioned Git repository.

```yaml
ruleset:
  source: git
  repository: https://github.com/example/team-policy
  ref: v1.2.0
```

Review and commit `.dornguard-ruleset.lock.json` after resolving a tag.
Subsequent checks use the recorded commit and reuse the local Git object
cache. A full commit SHA can also be configured directly. Use
`DORNGUARD_CACHE_DIR` to move the cache.
