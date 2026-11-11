# Second large-repository held-out set (amendment 17), 2026-11-11

Design, decision rule and stopping rule: preregistration amendment 17. Task selection:
`SELECTION.md`; plan: `PLAN.md`. Both were committed before the first run.

## What was measured

- **Harness and product commit:** `68cec67`. Both `settings.json` files give
  `harness_revision` = `ccm_commit` = `68cec67` and `ccm_dirty: false`.
- **Release build:** one build from a clean clone at that commit, ccm-cli `8630a4d2f446…` and
  ccm-mcp `e2918f8f7192…` (full SHA-256 in `settings.json`). Index caches were built once
  with it: Django 1145 s, Tokio 237 s, scikit-learn 579 s.
- **Agents:** codex-cli 0.167.0 with `gpt-6-luna` and Claude Code 2.1.319 with
  `claude-opus-5-5`, both at effort high, on subscriptions, with local embeddings. Both were
  started through `scripts/run_until_done.py`, detached from the supervising session, and ran
  from 06:20 to 06:59 EET.
- **Runs:** 78 per agent, 13 tasks × arms A and D × 3 repetitions, all recorded in one pass.
  No run hit a usage limit: no `quota-stop.json`, and no transcript with
  `"status":"rejected"` or HTTP 429. No run is partial. Scoring uses the scorer of
  amendment 16.
- **Arm D:** the product with #26 (dotted Python paths) and #28 (hook fixes). For Claude Code,
  D also installs the PreToolUse Grep hook and is labelled **D (context + hook)**. Codex's D
  has no hook. B and C were not run; their definitions are unchanged.

The ratio is input tokens with cache, per task: the D mean over repetitions divided by the A
mean. The median over tasks is given with a fixed-seed bootstrap 95 % interval. The tables
come from `scripts/subset_report.py`; the full reports are `codex/report.md` and
`claude/report.md`.

## Codex

| Task | A tokens | D tokens | D/A | A ok | D ok | D runs calling CCM | Pilot rule |
|---|---:|---:|---:|---:|---:|---:|---|
| `django-callers-log-response` | 56,859 | 50,574 | 0.89 | 3/3 | 3/3 | 3/3 | – |
| `django-callers-get-default-timezone` | 63,677 | 37,533 | 0.59 | 3/3 | 3/3 | 3/3 | saving |
| `django-callers-get-text-list` | 75,714 | 47,038 | 0.62 | 3/3 | 3/3 | 3/3 | saving |
| `django-impact-get-docs-version` | 42,334 | 37,136 | 0.88 | 3/3 | 3/3 | 3/3 | – |
| `django-trap-resolve` | 127,244 | 72,128 | 0.57 | 0/3 | 3/3 | 3/3 | saving |
| `sklearn-callers-indexable` | 79,866 | 41,819 | 0.52 | 3/3 | 3/3 | 3/3 | saving |
| `sklearn-callers-empirical-covariance` | 61,360 | 34,089 | 0.56 | 2/3 | 3/3 | 3/3 | saving |
| `sklearn-callers-gen-even-slices` | 65,688 | 44,108 | 0.67 | 3/3 | 3/3 | 3/3 | – |
| `sklearn-impact-check-scoring` | 42,257 | 41,954 | 0.99 | 3/3 | 3/3 | 3/3 | – |
| `sklearn-trap-normalize` | 82,998 | 59,499 | 0.72 | 3/3 | 2/3 | 3/3 | saving |
| `tokio-callers-uninit` | 44,155 | 34,491 | 0.78 | 3/3 | 3/3 | 2/3 | – |
| `tokio-callers-globals` | 33,585 | 71,377 | 2.13 | 3/3 | 3/3 | 3/3 | – |
| `tokio-trap-new-with-interest-and-handle` | 38,292 | 51,168 | 1.34 | 3/3 | 3/3 | 3/3 | – |

- D/A median: **0.72 (0.59–0.89, 13 tasks)**
- Success: A 35/39 (76 %–96 %), D 38/39 (87 %–100 %)
- Other ratios: output tokens 0.77 (0.67–0.85), wall time 0.82 (0.72–0.95)
- By category: callers 0.65 (0.56–0.89, 8 tasks); impact 0.94 (0.88–0.99, 2); trap 0.72
  (0.57–1.34, 3)
- By repository: Django 0.62 (0.57–0.89, 5); scikit-learn 0.67 (0.52–0.99, 5); Tokio 1.34
  (0.78–2.13, 3)
- Pilot rule met on 6 of 13 tasks

**Decision (amendment 17): saving shown.** The upper bound is below 1.00, and D's success is
not lower than A's. It is three runs higher.

## Claude Code, D (context + hook)

| Task | A tokens | D tokens | D/A | A ok | D ok | D runs calling `context` | Pilot rule |
|---|---:|---:|---:|---:|---:|---:|---|
| `django-callers-log-response` | 31,274 | 33,115 | 1.06 | 3/3 | 3/3 | 0/3 | – |
| `django-callers-get-default-timezone` | 31,384 | 37,323 | 1.19 | 3/3 | 3/3 | 0/3 | – |
| `django-callers-get-text-list` | 39,154 | 26,051 | 0.67 | 3/3 | 3/3 | 0/3 | saving |
| `django-impact-get-docs-version` | 22,161 | 29,740 | 1.34 | 3/3 | 3/3 | 0/3 | – |
| `django-trap-resolve` | 78,185 | 53,256 | 0.68 | 2/3 | 3/3 | 0/3 | saving |
| `sklearn-callers-indexable` | 39,192 | 31,994 | 0.82 | 3/3 | 3/3 | 2/3 | – |
| `sklearn-callers-empirical-covariance` | 31,419 | 29,030 | 0.92 | 3/3 | 3/3 | 0/3 | saving |
| `sklearn-callers-gen-even-slices` | 36,115 | 27,954 | 0.77 | 3/3 | 3/3 | 0/3 | saving |
| `sklearn-impact-check-scoring` | 25,888 | 34,456 | 1.33 | 3/3 | 3/3 | 2/3 | – |
| `sklearn-trap-normalize` | 81,466 | 41,651 | 0.51 | 3/3 | 3/3 | 1/3 | saving |
| `tokio-callers-uninit` | 42,961 | 47,264 | 1.10 | 3/3 | 3/3 | 1/3 | – |
| `tokio-callers-globals` | 35,375 | 33,325 | 0.94 | 3/3 | 3/3 | 1/3 | – |
| `tokio-trap-new-with-interest-and-handle` | 34,409 | 32,528 | 0.95 | 3/3 | 3/3 | 0/3 | – |

- D/A median: **0.94 (0.77–1.10, 13 tasks)**
- Success: A 38/39 (87 %–100 %), D 39/39 (91 %–100 %)
- Other ratios: cost 0.95 (0.88–1.08), output tokens 0.90 (0.79–0.97), turns 0.92 (0.75–1.09),
  wall time 0.90 (0.79–1.04)
- By category: callers 0.93 (0.77–1.10, 8 tasks); impact 1.34 (1.33–1.34, 2); trap 0.68
  (0.51–0.95, 3)
- By repository: Django 1.06 (0.67–1.34, 5); scikit-learn 0.82 (0.51–1.33, 5); Tokio 0.95
  (0.94–1.10, 3)
- Pilot rule met on 5 of 13 tasks

**Decision (amendment 17): no detectable difference.** The interval contains 1.00.

### Hook decisions (Claude D runs)

The hook saw 68 Greps in the 39 D runs. It answered 19 of them, in 19 runs (one per run), and
let 49 through:

- 22: the pattern was not a single symbol;
- 11: the symbol had already been answered in the same session;
- 6: a caller was a non-call reference. Example: `log_response` is passed to `sync_to_async`
  in `get_response_async`;
- 4: the packet did not claim a complete list;
- 3: the glob was not a test exclusion;
- 3: the answer was not shorter than the grep output.

Claude called `context` itself in 7 of 39 D runs. CCM answered something, by a call or by the
hook, in 22 of 39. The hook's answers did not end the search. In 9 of the 19, Claude's next search was the same
Grep. The hook's own message invites that ("Run the same Grep again if you still need the raw
matches"), and the second Grep passes as "already answered". In most of the other 10, Claude
searched again with another pattern or with Bash.

The clearest case is `get_docs_version` (D/A 1.34). In all three runs the sequence was: Grep,
hook answer, the same Grep again, and sometimes a further import search. The hook answer was
therefore an extra turn on top of the grep, not a replacement for it. Per task and run, the
decisions are in `claude/report.md`.

## Failed runs

All runs are kept in `<agent>/runs/`.

| Run | What was missing or wrong |
|---|---|
| Codex `django-trap-resolve` A#1, A#2, A#3 | `django/test/client.py:request` (Client and AsyncClient), although the question says the `django/test/` package counts |
| Codex `sklearn-callers-empirical-covariance` A#3 | named three `_robust_covariance.py` functions (`_c_step`, `fit`, `reweight_covariance`) that call a different function |
| Codex `sklearn-trap-normalize` D#2 | `_twenty_newsgroups.py:fetch_20newsgroups_vectorized` (calls it as `preprocessing.normalize`) |
| Claude `django-trap-resolve` A#1 | named `technical_404_response` instead of `get_caller` in `views/debug.py` |

## Owner audit

`scripts/owner_audit.py` scores nothing. It flags four passing Codex A runs:

- `django-callers-get-default-timezone` A#1–A#3 name `TimeFormat.r`; the method is
  `DateFormat.r`.
- `sklearn-callers-empirical-covariance` A#2 names `GraphLasso.fit`; the class is
  `GraphicalLasso`.

It flags no D run and no Claude run.

## What this shows

- **Codex:** with the dotted-path fix and the other product changes since amendment 15, it
  used about a quarter fewer input tokens on new held-out caller, trap and impact questions in
  large repositories, and it failed less.
  - The saving is in Django and scikit-learn.
  - On the three Tokio tasks D cost more (median 1.34). On `globals`, D used 2.13 times A's
    tokens.
- **Claude Code:** with the hook in its D, it did not show a measurable difference.
  - The hook answered a Grep in about half of the D runs. Claude often searched again
    afterwards, and it called `context` itself in 7 of 39 runs.
  - The saving is in the traps: `normalize` 0.51, `resolve` 0.68.
- **Next limit for Claude:** what Claude does after an answer, not whether it gets one. In 9
  of the 19 hook answers, it re-ran the same Grep.
- **Amendment 15 versus 17:** Codex had no detectable difference in amendment 15 (0.91,
  0.59–1.45), before the dotted-path fix. Amendment 17 uses a different, smaller task set and
  a changed product, so the two results are separate measurements, not a before and after on
  the same tasks.
