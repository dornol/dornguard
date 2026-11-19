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
