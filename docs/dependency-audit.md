# Maven dependency audit

Run the audit from the reactor directory containing the root `pom.xml`.
Install [OSV-Scanner v2](https://google.github.io/osv-scanner/installation/)
and put it on PATH, or run `dornguard engines install osv-scanner` to download
the pinned release (2.7.0) from GitHub into DornGuard's engine directory. The
download is verified against a SHA-256 recorded in DornGuard for each
supported platform (macOS, Linux and Windows on x86-64 and ARM64) and is
rejected on any mismatch. An OSV-Scanner on PATH takes precedence. DornGuard uses the Maven Wrapper when the directory
contains one and otherwise uses Maven on PATH.

```sh
dornguard audit
dornguard audit --format json
dornguard audit --maven-profile production --format json
```

The audit reads the dependency trees from Apache Maven Dependency Plugin
3.11.0. It checks the selected compile and runtime versions, including
transitive dependencies and versions inherited from the parent POM or
BOM. Test and provided dependencies and artifacts built by the same
reactor are outside the audit. Enable the same Maven profiles used by your
deployment to check the corresponding runtime dependencies.

DornGuard writes a temporary CycloneDX SBOM containing the resolved
dependency coordinates and asks OSV-Scanner to match them against the
online advisory database. The lookup sends Maven package names and
versions. Audit results identify the advisory, affected version, and
each reactor module whose dependency tree contains it. Source checks
use `dornguard check` independently.

Exit codes are `0` for no known findings, `1` for findings, and `2` for
an incomplete audit. A Maven error, scanner error, or missing result for
a resolved package makes the audit incomplete.
