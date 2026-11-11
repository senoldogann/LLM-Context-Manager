# Why Claude Code skipped CCM in arm D (large-repository set)

Data: the 54 Claude Code D runs of `large-repo-20261111/` (commit `01b2afa`, amendment 15). No
agent was run for this analysis. The transcripts are read with a short script, and the recorded
Grep calls are replayed offline through the Grep hook with `scripts/hook_replay.py`.

## What the transcripts show

- **The first tool decides it.** In all 33 runs without a CCM call, the first tool was `Grep`.
  In all 21 runs with one, it was `context`. No run switched to CCM after a Grep.
- **The question does not predict it.** The same question went to CCM in some repetitions and
  to Grep in others. The clear pattern is by category: none of the five name-collision traps
  (`get_connection`, `urlencode`, `shuffle`, `try_with` and the tuned `urlencode`) got a CCM
  call (0/15). Caller and impact questions got one in 21 of 39 runs.
- **After a Grep, Claude reconstructs the callers by hand.** It runs Bash loops over each
  matching file (`grep -n '^class '`, `sed -n`, small `ast` scripts) or reads files to find the
  function around each call site. That reconstruction is the work `context` does in one call.
- **When Claude called CCM, it still checked with a grep.** In 19 of 21 runs a Grep or Bash
  search followed the `context` answer. That happened even when the packet said "call list
  complete … no other grep is needed" (19 of the 21 packets). The answers then say "the index
  and a plain text search give the same result".
- **The server instructions and the usage note already say to use `context` instead of
  Grep.** Text alone did not change the first tool in 33 of 54 runs.

| Task | D runs that called CCM |
|---|---|
| `django-callers-constant-time-compare` | 0/3 |
| `django-callers-iri-to-uri` | 3/3 |
| `django-callers-is-aware` | 3/3 |
| `django-callers-make-hashable` | 0/3 |
| `django-impact-get-resolver` | 2/3 |
| `django-trap-get-connection` | 0/3 |
| `django-trap-urlencode` | 0/3 |
| `sklearn-callers-check-cv` | 3/3 |
| `sklearn-callers-gen-batches` | 2/3 |
| `sklearn-impact-get-config` | 1/3 |
| `sklearn-trap-shuffle` | 0/3 |
| `tokio-callers-async-op` | 2/3 |
| `tokio-callers-new-unnamed` | 1/3 |
| `tokio-callers-sleep-until` | 0/3 |
| `tokio-callers-unbounded-channel` | 1/3 |
| `tokio-callers-with-scheduler` | 3/3 |
| `tokio-impact-enter-runtime` | 0/3 |
| `tokio-trap-try-with` | 0/3 |

## A D-only fix: answer the Grep itself

Claude reaches for Grep, so the product meets it there. The opt-in PreToolUse hook (`ccm-mcp
hook claude-pre-tool-use`) answers a single-symbol Grep with the compact `context` answer. It
does so only when the index is fresh, the call list is complete and the answer is shorter than
the grep output. Replaying the 70 Grep calls of the 54 D runs exposed three problems, and the
change in this PR fixes them:

1. **Globs.** 25 Greps excluded only test directories (`!**/tests/**`,
   `!**/{tests,benches,examples}/**`). The hook refused every glob. It now accepts globs that
   exclude only test, bench or example directories, the scope the packet already covers. It
   drops callers in those directories from its answer and states how many it dropped, and it
   leaves them out of the grep size estimate.
2. **Wrong answer on a trap.** For `try_with` the hook returned 13 "callers" of the
   task-local `LocalKey::try_with`. Twelve of them call the standard library's thread-local
   `CONTEXT.try_with` and were labelled `may call (receiver not resolved)` in the full packet.
   The compact answer dropped that label. The hook now lets the grep run unless every caller is
   a proven `calls` relation.
3. **Scope.** With a `benches` exclusion, the answer for `unbounded_channel` still listed
   callers in `benches/`. These are now dropped as in point 1.

| Replay of the 70 D Greps | Hook at `01b2afa` | Hook in this PR |
|---|---|---|
| Greps answered | 19 | 27 |
| Grep output of the answered calls / hook answer (chars) | 43,727 / 21,950 | 65,655 / 31,870 |
| Runs without a CCM call whose first Grep is answered | 11 of 33 | 15 of 33 |
| Runs with a CCM call whose first Grep is answered | 8 of 21 | 12 of 21 |
| Wrong "complete" answers | 1 (`try_with`, not reached in the replay because of its glob) | 0 |

The hook's answers were scored against the gold for all ten tasks it answers. Eight match
exactly. `unbounded_channel` matches after the scope fix. `make_hashable` leaves out the
function's recursive call to itself, which is the definition note of amendment 15.

The remaining unanswered Greps are regexes with several alternatives (21) or targets whose
packet does not claim a complete list (12). Those are mostly the traps and `get_config`, whose
dotted path is fixed in #26.

## What this changes and what it does not

This only touches D's product configuration for Claude Code. Arms A, B and C are unchanged, and
Codex has no hook mechanism in the harness, so its D stays as it is. Whether the hook lowers
Claude's tokens is not measured here. The next preregistered held-out set (amendment 17) runs D
with the hook installed for Claude Code.
