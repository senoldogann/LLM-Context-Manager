# Fourth held-out set, Codex (amendment 19), 2026-11-11

Design, decision rule and stopping rule: preregistration amendment 19. Task selection:
`SELECTION.md`; plan: `PLAN.md`. Both were committed before the first run.

## What was measured

- **Harness and product commit:** `5eba8f0`. `codex/settings.json` gives `harness_revision` =
  `ccm_commit` = `5eba8f0` and `ccm_dirty: false`.
- **Release build:** one build, ccm-cli `8630a4d2f446…` (unchanged, so the index caches were
  reused) and ccm-mcp `af7dbaa3cbf0…`.
- **Agent:** codex-cli 0.167.0 with `gpt-6-luna`, effort high, subscription, local embeddings.
  Started detached through `scripts/run_until_done.py`, 10:21–11:25 EET.
- **Runs:** 72 (12 tasks × arms A and D × 3), all recorded in one pass. No run hit a usage
  limit and none is partial.
- **Arm D:** the product at `5eba8f0`, which includes three `context` changes:
  - each call-site line is listed once;
  - `Type::name` is resolved by its owner;
  - complete packets carry a `Files with calls (N): …` line.
- **Arms not run:** Claude Code was not run (its D route ended in amendment 18). B and C were
  not run.

## Codex, all twelve tasks — primary

| Task | A tokens | D tokens | D/A | A ok | D ok | D runs calling CCM | Pilot rule |
|---|---:|---:|---:|---:|---:|---:|---|
| `django-callers-flatten-fieldsets` | 63,231 | 41,639 | 0.66 | 3/3 | 3/3 | 3/3 | saving |
| `django-callers-call-command` | 52,823 | 37,390 | 0.71 | 1/3 | 2/3 | 3/3 | saving |
| `django-impact-get-user-model` | 32,493 | 37,257 | 1.15 | 3/3 | 0/3 | 3/3 | – |
| `django-impact-safe-join` | 31,528 | 29,158 | 0.92 | 3/3 | 3/3 | 3/3 | – |
| `django-impact-get-current-site` | 33,852 | 37,569 | 1.11 | 3/3 | 3/3 | 3/3 | – |
| `django-trap-select-template` | 57,806 | 59,436 | 1.03 | 3/3 | 3/3 | 3/3 | – |
| `sklearn-callers-compute-class-weight` | 64,845 | 37,730 | 0.58 | 3/3 | 3/3 | 3/3 | saving |
| `sklearn-callers-as-float-array` | 92,921 | 33,534 | 0.36 | 3/3 | 3/3 | 3/3 | saving |
| `sklearn-impact-get-scorer-names` | 50,165 | 51,239 | 1.02 | 3/3 | 3/3 | 3/3 | – |
| `sklearn-impact-svd-flip` | 40,323 | 37,851 | 0.94 | 3/3 | 3/3 | 3/3 | – |
| `sklearn-impact-pairwise-distances` | 39,088 | 35,301 | 0.90 | 3/3 | 3/3 | 3/3 | saving |
| `sklearn-impact-accuracy-score` | 39,044 | 34,022 | 0.87 | 3/3 | 3/3 | 3/3 | saving |

- D/A median: **0.91 (0.68–1.02, 12 tasks)**
- Success: A 34/36 (82 %–98 %), D 32/36 (75 %–96 %)
- Other ratios: output tokens 0.63 (0.50–0.81), wall time 0.78 (0.70–0.97)
- By category: callers 0.62 (0.36–0.71, 4 tasks); impact 0.94 (0.90–1.11, 7); trap 1.03 (1)
- By repository: Django 0.98 (0.68–1.13, 6); scikit-learn 0.89 (0.47–0.98, 6)

**Decision (amendment 19): no detectable difference.** The upper bound is 1.02. This set is
dominated by impact tasks (7 of 12), the kind where D had cost more before. The four callers
tasks again show a clear saving (0.36–0.71).

## Impact subset — secondary

- D/A median over the seven "which files" tasks: **0.94 (0.90–1.11)**.
- Success: A 21/21, D 18/21.
- **Decision: no saving shown. The stopping rule applies:** the files line did not turn
  "which files" questions into a saving.
- **Cost compared with before:** lower than in amendments 17 and 18 (1.30–1.70 on the costly
  tasks there).
- **Re-checks:** Codex ran `rg` or `grep` after `context` in 5 of 21 impact D runs, against 7 of
  12 before.

## A defect this run found

`django-impact-get-user-model` failed in all three D runs. Each missed the same four files:
`auth/forms.py`, `auth/handlers/modwsgi.py`, `management/commands/changepassword.py` and
`auth/views.py`. Each of them calls `get_user_model()` at module level
(`UserModel = get_user_model()`). The new `Files with calls` line listed only files with a call
inside a function, so it said 5 files instead of 9. Codex trusted it.

Because the impact line stops under the preregistered rule and the files line was wrong, the
line was **removed** from the product after the run, before merging. The merged code keeps the
other two changes (one line per call site, `Type::name` resolution), which are correctness
fixes.

This run measured those two changes only together with the files line. It found no failure
attributable to them in that combination. The run does not evaluate either change on its own,
nor the product without the files line. The measured commit `5eba8f0` still contains the files
line; this README describes that measurement.

## Failed runs

| Run | What was missing |
|---|---|
| Codex `django-callers-call-command` A#1, A#2 and D#1 | the four callers in `django/test/` (`run_checks`, `_fixture_setup`, `_fixture_teardown`, `setUpClass`), although the question says `django/test/` counts |
| Codex `django-impact-get-user-model` D#1, D#2 and D#3 | four files with module-level calls, missing from the files line (see above) |

## Owner audit

`scripts/owner_audit.py` flags no run.

## What this shows

- **Callers questions:** in three held-out sets Codex's D saved tokens on large-repository
  callers questions.
  - Callers category 0.62 here.
  - In amendments 17 and 18 the whole set showed a saving: 0.72 and 0.60.
- **"Which files" questions:** the run did not detect a saving. The interval, 0.94 (0.90–1.11),
  includes both a saving and a cost. The one-line file list introduced an error and was
  removed.
- **A set dominated by impact tasks** gives no detectable overall difference.
