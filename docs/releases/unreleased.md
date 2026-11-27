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
  - `kotlin`: `kotlin.date-pattern-typo` and `kotlin.field-injection` (errors),
    `kotlin.private-transactional` and `kotlin.jpa-data-class-entity`
    (warnings).
  - `javascript`: `no-unsafe-finally`, `no-self-compare`,
    `no-constant-binary-expression`, `no-unsafe-negation`,
    `no-unsafe-optional-chaining`, `use-isnan`, `valid-typeof` and
    `no-script-url` (warnings).
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
- The [rule catalog](../rules.md) lists every built-in rule with its engine,
  severity, confidence and profiles.

### Upgrading from v0.0.8

New default rules can add findings, and the error-severity rules among them
(`java.date-pattern-typo`, `java.string-reference-equality`,
`spring.static-member-injection`, `kotlin.date-pattern-typo` and
`kotlin.field-injection`) can fail a check that passed before. Run
`dornguard check` after upgrading, then fix the findings, suppress them with a
reason, or record them with `dornguard baseline create --force`.

An earlier managed ESLint has no TypeScript parser. Run `dornguard engines
install eslint` again before checking TypeScript sources.
