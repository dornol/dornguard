# Go and Rust checks

The `go` and `rust` profiles run each language's own analyzer and report its
findings in DornGuard's text, agent, JSON and SARIF formats. DornGuard adds
no rules of its own for these languages. The findings, their suppressions and
the project's lint settings come from the toolchain.

```yaml
version: 1
profiles: [go]        # or [rust], or both beside JVM profiles
```

The rules are `go.vet`, `rust.clippy` (Clippy lints) and `rust.rustc-lint`
(compiler lints). Each diagnostic names the analyzer or lint in `subject`, and
the tool's own suggestion is the fix text. See `dornguard rules explain go.vet
rust.clippy rust.rustc-lint` for what each reports and how to suppress it.

`dornguard init` writes these profiles for a directory with `go.mod` or
`Cargo.toml` and no Maven or Gradle build.

## Requirements

- Go 1.22 or later on `PATH` for the `go` profile.
- Rust with the Clippy component (`rustup component add clippy`) for the
  `rust` profile.

`dornguard doctor` checks both. `dornguard engines install` does not install
them. The Go and Cargo environment (`GOFLAGS`, `CARGO_HOME`,
`CARGO_TARGET_DIR` and so on) is passed through unchanged.

## How the tools run

- **Go**: DornGuard finds the nearest `go.mod` above each checked `.go` file
  and runs `go vet -json` in that module on the packages of those files. Files
  under `vendor` and `testdata` directories are not checked.
- **Rust**: DornGuard finds the Cargo workspace of each checked `.rs` file
  and runs `cargo clippy --workspace --all-targets --message-format=json` in
  it, so tests, examples and benches are linted too. Files under `target`
  directories are not checked. Lint levels come from the project: `[lints]` in
  `Cargo.toml`, `clippy.toml` and `#![allow]`/`#![deny]` attributes.

A lint the project sets to `deny` is reported with `error` severity and fails
the check whatever the rule's default. To fail on every Clippy warning, raise
the rule instead:

```yaml
rules:
  rust.clippy: { severity: error }
```

To report Clippy lints without compiler warnings, disable `rust.rustc-lint`:

```yaml
rules:
  rust.rustc-lint: { enabled: false }
```

A compile error is an engine error (exit 3). The report lists the compiler
errors and stays incomplete, and the lints of the code that built are still
reported.

`--changed` checks only the changed `.go` and `.rs` files. The tools still
compile what those files depend on, and findings in unchanged files are left
out. A change to `go.mod`, `go.work`, `Cargo.toml`, `clippy.toml` or
`.clippy.toml` can change the findings everywhere, so it makes the check
scan every file.

When a check uses the Maven or Gradle build model (Kotlin sources, `modules`,
`--module` or architecture rules), DornGuard checks the JVM and web sources of
the build modules and leaves Go and Rust sources out.
