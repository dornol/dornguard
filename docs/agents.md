# Using DornGuard with coding agents

Copy the [AGENTS.md instructions](templates/AGENTS.md) into the project's
`AGENTS.md` for agents that read it. For Claude Code, put the same quality gate
instructions in the project's `CLAUDE.md`.

Use `dornguard check --changed --format agent` for a concise report that shows
the first 20 diagnostics. Use `--format json` for every result and
`dornguard rules explain RULE_ID` for the rule's guidance. Exit codes match
those used by the full CI check.
