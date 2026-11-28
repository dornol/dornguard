## DornGuard (unreleased)

<!-- Draft for the next release. The "Prepare release" commit renames this file
to vX.Y.Z.md, fills in the version, the ruleset version and the links, and
starts a new draft. Add an entry here with every user-visible change. -->

Release archives are available for Linux x86-64 (AMD64), Linux ARM64 and macOS
ARM64. Linux ARM64 is new; `install.sh` selects it on `aarch64` machines.

- Go and Rust checks. The `go` profile reports `go vet` findings as `go.vet`.
  The `rust` profile reports `cargo clippy` lints as `rust.clippy` and compiler
  lints as `rust.rustc-lint`. Both use the project's own toolchain and lint
  settings, and `dornguard init` selects them for a Go module or Cargo package.
  See [Go and Rust](../go-rust.md).
- TypeScript. The `javascript` profile checks `.ts`, `.mts`, `.cts` and `.tsx`
  files through `@typescript-eslint/parser`, without type information.
  Declaration files are skipped.
- New default rules:
  - `java-source` and `java`: `java.date-pattern-typo` (error, `YYYY` or `DD` in a
    date pattern), `java.string-reference-equality`
    (error), `java.log-format-arguments`, `java.throw-from-finally`,
    `java.double-checked-locking` and `java.lock-without-finally` (warnings,
    the last with medium confidence).
  - `spring-boot`: `spring.static-member-injection` (error) and
    `spring.request-mapping-without-method` (warning).
  - `security`: `security.jackson-default-typing` and `security.xxe`
    (warnings, medium confidence).
  - `kotlin`: `kotlin.date-pattern-typo`, `kotlin.field-injection` and
    `kotlin.static-member-injection` (errors, the last for `@Value` in a
    companion object or an `object`), `kotlin.private-transactional` and
    `kotlin.jpa-data-class-entity` (warnings).
  - `javascript`: `no-unsafe-finally`, `no-self-compare`,
    `no-constant-binary-expression`, `no-unsafe-negation`,
    `no-unsafe-optional-chaining`, `use-isnan`, `valid-typeof` and
    `no-script-url` (warnings).
- More Java, Spring and Kotlin rules from recurring review findings:
  - `java-source`: `java.wildcard-import` and `java.optional-misuse` (medium
    confidence).
  - `java`: `java.unused-private-field`, `java.unused-private-method`,
    `java.unused-local-variable` and `java.return-null-collection`.
  - `spring-boot`: `spring.redundant-autowired-constructor`,
    `spring.jpa-entity-request-body`, `spring.jpa-lombok-entity`,
    `spring.jpa-eager-fetch`, `spring.transactional-in-controller`,
    `spring.http-client-without-timeout` and `spring.open-in-view-enabled`
    (the last two with medium confidence).
  - `kotlin`: `kotlin.unmanaged-coroutine-scope` (medium confidence), and
    `kotlin.missing-spring-plugin` and `kotlin.missing-jpa-plugin` (errors) for
    Gradle modules without the Kotlin Spring or JPA compiler plugin.
  - `layered`: `architecture.service-independence` (error) keeps services from
    depending on controllers.
  - New opt-in profiles: `java-design` (`java.cognitive-complexity`,
    `java.excessive-parameters`, `java.god-class`, `java.loose-coupling`),
    `kotlin-review` (`kotlin.not-null-assertion`) and `typescript-review`
    (`typescript.no-floating-promises`, which uses TypeScript type
    information from the project's `tsconfig.json` and skips, with a note,
    TypeScript files that no `tsconfig.json` project includes).
- `dornguard engines install eslint` installs a pinned ESLint, TypeScript parser
  and TypeScript from a lockfile with integrity hashes, without install scripts.
  `dornguard engines install osv-scanner` downloads a pinned, checksum-verified
  OSV-Scanner for `dornguard audit`.
- `DORNGUARD_JAVA_HOME` selects the JVM for the ArchUnit runner while Maven and
  Gradle keep using `JAVA_HOME`.
- A managed Semgrep whose Python interpreter is gone, for example after a
  Homebrew Python upgrade, counts as not installed, so `dornguard engines
  install` rebuilds it.
- A Maven compile failure on JDK 23 or later that loses Lombok-generated members
  explains the annotation processor change and the fixes.
- The Thymeleaf literal-markup hint finds the right column on lines with
  multibyte text.
- The Gradle project model works with Gradle 8.10 and earlier, where
  module dependencies failed with "Could not get unknown property 'path'".
- `java.preserve-stack-trace` no longer reports throwing the exception a caught
  wrapper carries, such as `throw e.original()` or
  `throw (RuntimeException) e.getCause()`.
- `java.resource-not-closed` no longer reports a resource that a Spring `@Bean`
  method, or a private helper only `@Bean` methods call, hands to the object it
  builds, such as a connection manager passed to an HTTP client builder.
- When the Java target is unknown, as in a source-only check, a
  `java.resource-not-closed` finding for the JDK `HttpClient` or an
  `ExecutorService` comes with a note to set `java_target`; on a target below
  Java 21 or 19 these types are not `AutoCloseable`.
- Rule guidance for coding agents. `dornguard rules explain RULE...` prints
  what each rule reports and does not report, the fix, a bad and a good
  example, when suppression is legitimate and the exact suppression syntax;
  `--format json` returns the same fields. The agent report ends with the
  `rules explain` command for the rules it shows. The
  [AGENTS.md template](../templates/AGENTS.md) is now a complete,
  language-independent workflow, and the generated [rule catalog](../rules.md)
  replaces the rule tables in the profile documentation. Custom rules can
  carry the same `guidance` block.

### Upgrading from v0.0.8

New default rules can add findings, and the error-severity rules among them
(`java.date-pattern-typo`, `java.string-reference-equality`,
`spring.static-member-injection`, `kotlin.date-pattern-typo`,
`kotlin.field-injection`, `kotlin.static-member-injection`,
`kotlin.missing-spring-plugin`,
`kotlin.missing-jpa-plugin` and, with `layered`,
`architecture.service-independence`) can fail a check that passed before. Run
`dornguard check` after upgrading, then fix the findings, suppress them with a
reason, or record them with `dornguard baseline create --force`.

An earlier managed ESLint has no TypeScript parser or typescript-eslint
plugin. Run `dornguard engines install eslint` again before checking
TypeScript sources or using `typescript-review`.
