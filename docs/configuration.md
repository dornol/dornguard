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
The checkout's Baseline, rule overrides, ignore list and CLI scope are
applied to the project scan. Gradle still uses the project's build model,
so protect updates to the build configuration and use isolated CI workers.
