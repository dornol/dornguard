## Quality gate

1. Run `dornguard check --changed --format agent` after editing Java code.
2. Fix blocking findings and repeat the check. Use `dornguard rules explain RULE_ID` for individual rules and `dornguard check --changed --format json` for the complete report.
3. A finding with a `Review:` line (`confidence: medium` in JSON) comes from a rule that infers intent from code shape. Confirm that it is a real problem and fix it. If the code is correct, suppress it with a `// dornguard-ignore RULE_ID -- reason` comment that explains why. Do not suppress a finding without a `Review:` line to make a check pass; fix the code.
4. Run the project's required tests and report any remaining warnings, incomplete checks and suppressions you added.

CI runs the full `dornguard check`.
