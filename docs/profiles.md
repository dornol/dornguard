# Profiles and rulesets

Run `dornguard profiles list` to see available profiles and
`dornguard profiles show NAME` for their direct rules and parent profiles.
Use `dornguard rules explain ID` for the reason and guidance for a rule.

| Profile | Checks |
| --- | --- |
| `java-source` | Semgrep Java source rules |
| `java` | Semgrep and PMD Java source rules |
| `format` | Java, Kotlin, and JavaScript source formatting |
| `jspecify` | Requires all checked Java types to be in a JSpecify `@NullMarked` scope |
| `kotlin` | Semgrep output and coroutine checks for Gradle Kotlin/JVM production sources |
| `spring-boot` | Java and Spring injection, transaction, and singleton-state checks |
| `spring-rollback` | Opt-in review of checked exceptions under Spring’s default rollback policy |
| `thymeleaf` | Spring bean references in restricted template attributes |
| `thymeleaf-output-review` | Thymeleaf compatibility checks plus warnings for dynamic expressions rendered without HTML escaping |
| `html` | Executable inline scripts and common inline event handlers |
| `javascript` | ESLint checks for JavaScript evaluated from strings and async Promise executors |
| `jsp` | JSP scripting elements and JSTL output with escaping disabled |
| `security` | Java CORS patterns |
| `hexagonal` | Domain, application, and adapter dependencies |
| `layered` | Controller, service, and repository dependencies |
| `rest-api` | Spring and security rules |
| `scheduler` | Spring rules for scheduler modules |
| `java-library` | Java and framework independence rules |

The `spring.field-injection` rule skips Java test sources under
`src/test/java` in every module. Other rules still check those files when
they are included in the scan.

## Source formatting

Add `format` to your profiles or run `dornguard check --profile format`.
The profile works with `.java`, `.kt`, `.js`, `.mjs`, and `.cjs` sources and
uses the bundled parsers. It reports the following as errors.

| Rule | Default |
| --- | --- |
| `format.indentation` | Spaces only. Indent blocks by four spaces in Java and Kotlin and two in JavaScript. |
| `format.line-ending` | LF throughout the file. |
| `format.final-newline` | A newline at the end of every nonempty file. |
| `format.trailing-whitespace` | No spaces or tabs at the end of a line. |
| `format.blank-lines` | At most one consecutive blank line. |
| `format.control-spacing` | A single space before a control keyword's opening parenthesis, as in `if (ready)`. |

Indentation width is checked at block boundaries and for direct block
members. Continuation lines may align with surrounding expressions.
Whitespace inside string literals, text blocks, and template literals is
preserved by the spacing, blank-line, and indentation rules.
See [formatting configuration](configuration.md#formatting) for overrides.

## JSpecify coverage

Add `jspecify` alongside the project's existing profiles to enable
`java.jspecify-null-marked` as an error.

```yaml
version: 1
profiles: [spring-boot, jspecify]
```

Add `org.jspecify:jspecify` to the Java project's build dependencies and mark
each package through its own `package-info.java`:

```java
@NullMarked
package com.example.service;

import org.jspecify.annotations.NullMarked;
```

The rule checks named classes, interfaces, records, enums, and annotation types,
including nested and local declarations. It accepts `@NullMarked` on the type,
an enclosing declaration, the same package via a sibling `package-info.java`,
or a Java module via `module-info.java` at the package hierarchy's source root.
Each subpackage has its own scope. A type in a `@NullUnmarked` scope is
reported unless a more specific declaration marks it again. Changes to either
descriptor trigger a full scan in changed mode to catch affected types.

DornGuard checks annotation coverage from source. Use JSpecify's `@Nullable`
where a type permits null and a JSpecify-aware compiler checker for nullness
analysis. See the [JSpecify user guide](https://jspecify.dev/docs/user-guide/).

## Java, Kotlin and Spring warnings

These rules are warnings by default. Use a rule's `severity` override to
make it blocking.

| Rule | Reports |
| --- | --- |
| `java.fully-qualified-type` and `kotlin.fully-qualified-type` | Package-qualified names shaped like types in Java or Kotlin code |
| `java.unnecessary-import` | Unused, duplicate, or implicit imports reported by PMD |
| `java.preserve-stack-trace` | Rethrown exceptions that lose their original cause, as reported by PMD |
| `java.resource-not-closed` | Potential leaks of Java resources such as file streams and JDBC connections, as reported by PMD |
| `java.bigdecimal-from-floating-point-literal` | Decimal literals passed to the `BigDecimal` constructor, as reported by PMD |
| `java.return-from-finally` | A return inside a `finally` block that can override an earlier return or suppress an exception, as reported by PMD |
| `java.broken-null-check` | A null check that fails to guard a subsequent dereference, as reported by PMD |
| `java.equals-hashcode-contract` | Overriding only one of `equals` and `hashCode`, as reported by PMD |
| `java.ignored-interrupt` | An `InterruptedException` catch with a single logging call, optionally followed by a bare return |
| `kotlin.run-blocking-in-suspend` | `kotlinx.coroutines.runBlocking` inside a Kotlin suspend function |
| `kotlin.thread-sleep-in-suspend` | `Thread.sleep` inside a Kotlin suspend function |
| `kotlin.swallowed-cancellation` | An explicit coroutine cancellation catch that is empty, returns without a value, or only logs |
| `kotlin.run-catching-cancellation` | A known suspending coroutine operation or a direct call to a sibling no-argument `suspend` function inside `runCatching` |
| `kotlin.global-scope` | `GlobalScope.launch` or `GlobalScope.async` |
| `spring.singleton-request-state` | A Java Spring MVC singleton controller handler that stores a parameter directly in a `this` field |
| `spring.shared-simple-date-format` | A field of type `SimpleDateFormat` or one initialized with it in a singleton Spring component |
| `spring.private-transactional` | A private Java method with Spring's `@Transactional` |
| `spring.transactional-self-invocation` | An implicit or explicit `this` call from a Java method to a no-argument Spring `@Transactional` method declared in the same class |
| `spring.async-self-invocation` | An implicit or explicit `this` call from a Java method to a no-argument Spring `@Async` method declared in the same class |

The Spring self-invocation checks apply to proxy-based transactions and async
execution. They cover no-argument targets so calls with arguments cannot be
mistaken for the annotated overload.

The shared formatter check recognizes Java Spring stereotype annotations
with default singleton scope or an explicit singleton declaration.
Method-local formatters and request-, session-, application-, and
prototype-scoped beans are outside its scope. PMD may keep imports whose
usage it cannot resolve without a complete classpath.

Unused wildcard imports, including static ones, are reported when DornGuard
can pass the module's compiled classes and dependencies to PMD and the file
is included in the main compiler inputs reported by the build integration.
Files excluded by the compiler use the source-only fallback, including
files that still have class output from an earlier build.
DornGuard reuses this classpath when another check already compiles the
module. To compile Maven or Gradle sources specifically for this rule, run
`dornguard check --resolve-imports`.

The option uses Maven's main compile phase or Gradle's main classes task.
With Maven, build checks examine the full reactor even when `--changed` is
set. Gradle supports the usual module selection. Files checked without a
compiled classpath, including Java test sources,
still report unnecessary explicit imports and duplicate wildcard imports.

The qualified-type checks use Java and Kotlin naming conventions to identify
candidates in code. Java analysis skips qualified expressions rooted in a
locally declared value that is visible at that position. Import and package
declarations, comments, and string literals are skipped. Qualifiers that
distinguish types with the same name may be intentional.

The `runCatching` check recognizes selected `kotlinx.coroutines` calls such as
`delay` and `withContext`. It also finds direct calls to sibling no-argument
`suspend` functions with block bodies, and skips a direct
`runCatching { ... }.getOrThrow()` chain.

### Thymeleaf output review

Enable `thymeleaf-output-review` to check dynamic expressions in `th:utext`,
`data-th-utext`, and unescaped `[(...)]` in normal HTML template text. Review
each warning to confirm whether the HTML should be escaped or has already been
sanitized.

### Rollback review

Keep the existing Spring profile and add `spring-rollback` to review checked
exceptions in projects using Spring’s default rollback behavior.

```yaml
version: 1
profiles: [spring-boot, spring-rollback]
```

`spring.checked-exception-rollback` reports a direct throw of a new
`IOException` or `SQLException` from a Java method with a bare or empty
Spring `@Transactional` annotation. It checks top-level statements and
throws in simple top-level `if` branches. Review the transaction’s actual
rollback policy and whether the exception should undo the transaction.

For setup and runtime CSP checks, see [HTML, JavaScript, and CSP](web-checks.md).

## Thymeleaf compatibility

Add `thymeleaf` to `profiles` to scan `.html` and `.htm` files under
`src/main/resources/templates` in the project or its build modules.
`thymeleaf.restricted-bean-access` flags Spring bean references in direct
expressions in `th:href`, `th:src`, `th:utext`, `th:on*`, `th:insert`, and
`th:replace`, including their `data-th-*` forms. It also checks simple
dynamic URL bases and fragment selectors. These restricted expressions
can fail at render time on Thymeleaf 3.1.4 and later. Compute the value in
the controller and expose it as a model attribute.

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
metadata can bind one Semgrep pattern rule to its engine file or provide
package selectors for ArchUnit. Rule IDs, engine rule IDs and profile names
must be unique.
Declare the rule's source languages in its Semgrep YAML. A rule targeting
Kotlin activates Gradle Kotlin/JVM source discovery for its policy scope.

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
