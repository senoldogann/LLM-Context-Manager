# Repeated Grep after a hook answer: offline replay (tuned, not held out)

No agent was run. This replay uses the 39 Claude Code D transcripts of `heldout17-20261111/`.
Those tasks informed this change, so the result below is a tuned measurement and not a
held-out one.

## The change (D only)

In amendment 17 the hook answered a Grep in 19 of 39 Claude D runs. Eleven of those answers
were followed by another Grep for the same symbol: nine with the identical pattern and two
with another spelling (`\bglobals\(` after `globals\b`). The old hook let every repeat
through as "already answered once in this session". Its first answer had also invited the
repeat: "Run the same Grep again if you still need the raw matches".

The hook in this branch changes two things:

- **First answer:** the invitation is gone. The answer now says that every call of the
  symbol in the indexed non-test files is inside the listed callers.
- **Second Grep for the same symbol in a session:** it is answered with the raw matches at
  the listed call sites, as `path:line: text`, the shape of Grep's own `content` output. The
  answer also says how many other matching lines exist (imports, definitions, comments,
  strings, non-call uses). It is used only when it is shorter than the Grep output. A third
  Grep for the symbol always passes.

Arms A, B and C are unchanged. The hook runs only in Claude Code's D.

## Replay

`scripts/hook_replay.py` now replays a run's Greps in their recorded order in one hook session,
so a repeat meets the state left by the first answer. The released binaries were
`bin-68cec67`. The new hook is that commit's ccm-cli with this branch's ccm-mcp.

| 68 recorded Greps in the 39 D runs | Hook at `68cec67` | Hook in this branch |
|---|---|---|
| Answered with the caller packet | 19 | 19 |
| Answered with call-site lines (repeats) | – | 11 |
| Let through | 49 | 38 |
| Let through as "already answered" | 11 | 0 |

- The old hook's replay matches the live run exactly: 19 answered and 49 let through, as in
  the run records.
- All 11 repeats of an answered symbol would be answered with call-site lines. That covers
  all 9 identical repeats.
- The 11 repeats returned 30,582 characters in the recorded runs. The hook's call-site answers
  would be 12,891 characters.
- The remaining 38 Greps pass for the reasons amendment 17 reported: a pattern that is not a
  single symbol (22), a non-call reference among the callers (6), no complete call list (4),
  a glob that is not a test exclusion (3), or an answer not shorter than the grep (3).

Example (`get_docs_version`, Django). The second Grep gets the 11 call-site lines and the note
that the other 11 matches are imports and definitions. In the recorded run, the full 22-line
grep output came back instead.

## What this does not show

The replay shows what the hook would answer. It does not show what Claude does next. Without
the invitation, Claude may repeat the Grep less often. After call-site lines, it may still run
another search. Both are measured by the next preregistered held-out set (amendment 18).
