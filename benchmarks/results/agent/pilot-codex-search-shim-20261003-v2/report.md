# Codex rg-wrapper pilot (valid run): compact search output did not save agent tokens

Measured on 2026-10-03 at commit `3814a87` with `uv run python -m search.pilot run` (settings in
`settings.json`, generated table in `summary.md`, one record per run in `runs/`; transcripts stay
local). Codex CLI 0.160.0, `gpt-6.1-sol`, effort high, ChatGPT subscription, 6 tasks × 2 arms × 1
repetition. Both arms put an rg wrapper for Codex's bundled rg first in PATH and disable the Codex
shell snapshot; A's wrapper only logs the call, S's rewrites line-shaped output into the CCM compact
form (`search/rg_shim.py`). The L3 tasks, prompt, Codex flags and scorer are unchanged.

## Result

| | A (plain rg) | S (compact rg) |
|---|---:|---:|
| Correct final answers | 6/6 | 6/6 |
| Total tokens (input incl. cached + output) | 293,585 | 334,025 |
| Shell commands / rg commands / file reads | 22 / 18 / 9 | 28 / 21 / 12 |

Per-task S/A total tokens: express-callers-res-set 1.16, express-trap-req-get 1.38,
flask-callers-ensure-sync 1.12, flask-trap-load-dotenv 1.04, serde-callers-expr-is-missing 1.04,
serde-edit-with-bound 1.01 (median 1.08; S above A in all six).

The wrapper worked as designed: 24 logged calls in S (12 compacted, 11 list/count calls passed
through, 1 kept raw because compact was not smaller). The compacted calls showed 8,235 of 56,627 raw
bytes (0.15).

## Why

In express-trap-req-get the same broad search returned 12.6 KB of raw lines in A, which contained the
code the agent needed. In S it returned 1.5 KB of function names and line numbers, and the agent then
ran `cat lib/request.js` and `cat lib/response.js` (together with a second compact search, 41 KB of
output) to see the code. Compact output hides match text; the agent fetches it back by reading whole
files, which costs more than the search output saved. The structure-preserving "full" variant keeps
the text but was not smaller than RTK in the offline pre-study (median 1.07,
`../../search-prestudy-20261003/report.md`).

## Noise and limits

- One repetition. The invalid attempt 1 (`../pilot-codex-search-shim-20261003/`), where the wrapper
  never ran, doubles as an A/A check: per-task ratios 0.89–1.55 (median 1.05) with no effect. The
  size of the increase is therefore not established; the absence of a saving is consistent with the
  mechanism above.
- Codex only (no Claude Code authentication in this environment), three small and medium
  repositories, six search-heavy tasks.
- The shell snapshot is disabled in both arms, unlike the L3 harness default. With it enabled the
  wrapper was never invoked, which also means the L3 harness has not verified which rg Codex uses.

## Conclusion

Structure-aware compaction of the agent's own search output shrinks that output but, in this form,
does not reduce Codex's total token use and led to more file reads. The pre-study's offline saving
does not carry over to agent quota.
