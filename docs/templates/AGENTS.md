## Quality gate (DornGuard)

Run DornGuard after every change to source code, templates or build files, and
before you report the work as done. CI runs the full `dornguard check` with the
same policy.

1. Run `dornguard check --changed --format agent`.
2. Read the exit code:
   - `0`: no blocking findings. Warnings may remain; go to step 5.
   - `1`: findings to fix. Go to step 3.
   - `2`: the configuration is invalid. Report the message; do not edit
     `.dornguard.yml` to get past it unless the task is about that file.
   - `3` or `4`: an engine or the build failed, so the analysis is incomplete.
     Fix a compile error you caused and rerun; otherwise report the failure.
     Never report an incomplete check as passing.
3. The last line of the report names the rules involved. Run
   `dornguard rules explain RULE...` for them (`--format json` for data). It
   says what the rule reports and does not report, the fix, a bad and good
   example, and when suppression is allowed.
4. Fix each finding by editing the code; DornGuard applies no edits itself.
   - A finding without a `Review:` line is a real problem: change the code.
   - A finding with a `Review:` line (`confidence: medium`) comes from a rule
     that infers intent. Confirm it against the rule's "Reports" and
     "Suppression" text. If it is real, fix it. If the code is correct, add
     the suppression that `rules explain` shows, with a reason a reviewer can
     check, such as
     `// dornguard-ignore java.optional-misuse -- Jackson requires the Optional field`.
   - Do not suppress a finding without a `Review:` line, and do not disable
     rules, lower severities, add `ignore` entries or regenerate the baseline
     to make a check pass, unless the user asks for it.
5. Rerun step 1 until it exits `0`, then run the project's tests.
6. In your report, list the warnings that remain, every suppression you added
   with its reason, and any check that was incomplete.

`dornguard check --changed --format json` returns every finding when the agent
report says more were omitted.
