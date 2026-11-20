# CI quality gate

Run `dornguard check` with the same configuration and engine versions as local
development. Check out the full project and use `--format sarif` to retain a
code-scanning artifact. The exit code gates CI even when a report is archived.

The [GitHub Actions workflow](../.github/workflows/ci.yml) uses read-only
repository permissions and clears checkout credentials. The
[Jenkins pipeline](../Jenkinsfile) expects a Linux worker with Rust, JDK 21,
Python 3.12, curl, unzip and shasum. Both verify downloads and build the
ArchUnit runner for the CLI.

Gradle evaluates the repository's scripts and plugins. Run pull request
checks on isolated workers without deployment credentials or secrets.

## Adopt the gate in an existing project

Record the existing findings once, using the same profiles and type resolution
as the CI check. Review and commit the resulting baseline with the policy.

```sh
dornguard baseline create
dornguard check --changed --baseline .dornguard-baseline.json --format agent
```

If CI uses `--resolve-types`, use it when creating the baseline and when
checking changes locally. Pin the CLI, engine and ruleset versions used by CI
and by agents.

For a pull request, use its actual base branch in the trusted CI job. Fetch the
base ref and use its baseline for the full check. The example guards the
configuration and baseline at the project root. Add the paths to any local
ruleset or custom configuration that your project uses.

```bash
set -eu
base_ref="${DORNGUARD_BASE_REF:-origin/main}"
base_commit=$(git rev-parse --verify --end-of-options "${base_ref}^{commit}")
git diff --exit-code "$base_commit" HEAD -- .dornguard.yml .dornguard-baseline.json

baseline=$(mktemp)
trap 'rm -f "$baseline"' EXIT
git show "$base_commit:.dornguard-baseline.json" > "$baseline"
dornguard check --baseline "$baseline" --format json
```

For organization-wide blocking rules, add
`--organization-policy /trusted/organization.yml` from a trusted location.
The baseline applies to the project check. See the
[organization policy settings](configuration.md#organization-policy).

## Focus on one kind of finding

The agent report lists a representative finding for each rule within a
severity before repeating locations for that severity. Combine
`--report-rule RULE_ID` and `--report-path PATH` to filter the findings you
read while keeping the same check scope. Repeat either option to match more
rules or paths. Global counts, analysis failures and exit codes still
describe the whole check.

```sh
dornguard check --changed --format json \
  --report-rule format.final-newline --report-path src
```

## Apply a suggested edit

A JSON finding for trailing whitespace or a missing final newline can include
an `edit`. Its `start_byte` and `end_byte` define a zero-based, half-open range
in the original UTF-8 file. Replace that range with `replacement`. The
diagnostic's line and column still use the report's usual coordinates.

Calculate SHA-256 over the file's exact bytes and compare it to the edit's
`source_sha256` before applying edits. If they differ, run the check again.
When applying several edits from the same snapshot, check for overlaps,
apply from highest to lowest byte offset, and rerun the original check.
