## Quality gate

1. Run `dornguard check --changed --format agent` after editing Java code.
2. Fix blocking findings and repeat the check. Use `dornguard rules explain RULE_ID` for individual rules and `dornguard check --changed --format json` for the complete report.
3. Run the project's required tests and report any remaining warnings or incomplete checks.

CI runs the full `dornguard check`.
