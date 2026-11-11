# Arm E: Claude Code's Bash re-checks, offline replay (tuned, not held out)

No agent was run. The data are the 78 Claude Code D transcripts of `heldout17-20261111/` and
`heldout18-20261111/`. Those transcripts motivated arm E, so this is a tuned measurement.

## Why arm E

In amendment 18 the Grep hook answered 17 Greps. After every one of them Claude ran the same
search again as a Bash `grep -rn`, which the Grep hook does not see. Of the Bash commands in
these D runs that run `grep` or `rg`:

- 33 came after a hook answer and are plain symbol searches;
- 13 came after a hook answer and rebuild enclosing functions by hand (`for` loops, `awk`,
  `grep -n "^class"`);
- 57 came in runs where the hook had not answered yet (49 of them rebuild enclosing
  functions).

Arm E is arm D with the same hook also matched on Bash.

- **What the hook answers:** a single search, meaning an optional leading `cd`, then
  `grep -r…` or `rg …`, then only `grep -v` filters with a fixed string. It answers that search
  with the Grep hook's rules: the caller packet first, then the call-site lines.
- **What it lets run:** everything else.
- **Size check:** fixed-string filters are applied to the size estimate, so an answer is never
  longer than what the command would print.
- **Other arms:** A to D are unchanged.

## Replay

`scripts/hook_replay.py <bin> <corpus> <results> '<set>/claude/transcripts/*__D__*.jsonl'
Grep,Bash`, with the arm-E hook and each run replayed in one hook session.

| 194 recorded searches in the 78 D runs | Grep only (D's hook) | Grep and Bash (E's hook) |
|---|---|---|
| Greps answered (packet / call-site lines) | 47 (36 / 11) | 47 (36 / 11) |
| Bash searches answered | – | 7 of 76 |
| Bash searches let through | – | 69 |

Why the 69 Bash searches pass:

| Reason | Count |
|---|---|
| several commands in one Bash call | 20 |
| shell variables (`$f` in loops) | 12 |
| not a single symbol | 9 |
| a pipe stage other than a fixed-string `grep -v` | 12 |
| a regex `grep -v` filter | 5 |
| already answered twice in the session | 5 |
| other (`-l`, a single file, a redirect, `$(…)`) | 5 |
| answer not shorter | 1 |

- **Answered searches:** of the 7, two have a usable recorded output (1,477 and 1,912
  characters), where the answers are 1,269 and 1,511 characters.
- **The other five:** the recorded output is 42 characters, an error. In the original run,
  `cd django` failed because Claude was already in that directory. The replay resolves `cd`
  from the project root, so those five comparisons are not meaningful.

## What this predicts

- **Coverage:** arm E would answer few of Claude's Bash re-checks. Most of them are compound
  commands that a hook cannot answer safely.
- **Size:** where it can answer, the saving per call is a few hundred characters.
- **Prediction:** a held-out run is unlikely to show a saving.
- **Why measure anyway:** a measurement, not this prediction, is what decides. Amendment 20
  records the prediction before the run.
