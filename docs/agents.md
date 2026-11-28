# Using DornGuard with coding agents

Copy [templates/AGENTS.md](templates/AGENTS.md) into the project's
`AGENTS.md`, or into `CLAUDE.md` for Claude Code. It is the complete workflow
an agent follows: when to run the check, what each exit code means, how to
read rule guidance, and when a finding may be suppressed.

The agent reads everything else from the CLI:

- `dornguard check --changed --format agent` shows up to 20 diagnostics, the
  most severe first, and ends with the `dornguard rules explain` command for
  the rules involved. `--format json` returns every diagnostic.
- `dornguard rules explain RULE...` prints what a rule reports and does not
  report, the fix, a bad and a good example, when suppression is legitimate
  and the exact suppression syntax. `--format json` returns the same fields.
- Findings with a `Review:` line come from `medium` confidence rules and need
  a judgment; the others need a code change.

DornGuard reports fixes as text and never edits files itself.
