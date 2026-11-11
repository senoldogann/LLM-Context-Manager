# Third large-repository held-out set (amendment 18), 2026-11-11

Design, decision rule and stopping rule: preregistration amendment 18. Task selection:
`SELECTION.md`; plan: `PLAN.md`. Both were committed before the first run.

## What was measured

- **Harness and product commit:** `ad29ffc`. Both `settings.json` files give
  `harness_revision` = `ccm_commit` = `ad29ffc` and `ccm_dirty: false`.
- **Release build:** one build from a clean clone, ccm-cli `8630a4d2f446…` (byte-identical to
  amendment 17's, so its index caches were reused) and ccm-mcp `841bbe3a107b…`.
- **Agents:** codex-cli 0.167.0 with `gpt-6-luna` and Claude Code 2.1.319 with
  `claude-opus-5-5`. Both at effort high, on subscriptions, with local embeddings, started
  detached through `scripts/run_until_done.py` between 08:13 and 09:00 EET.
- **Runs:** 78 per agent (13 tasks × arms A and D × 3 repetitions), all recorded in one pass.
  No run hit a usage limit: no `quota-stop.json`, and no transcript with
  `"status":"rejected"` or HTTP 429. No run is partial.
- **Arm D:** the product at `ad29ffc`. For Claude Code that includes the Grep hook with the
  changed repeat handling, labelled **D (context + hook)**. Codex's D has no hook and is the
  amendment-17 product; its run checks whether amendment 17's result replicates on new tasks.
  A, B and C are unchanged; B and C were not run.

The ratio is input tokens with cache, per task: the D mean over repetitions divided by the A
mean. The median over tasks is given with a fixed-seed bootstrap 95 % interval, from
`scripts/subset_report.py`. The full reports are `claude/report.md` and `codex/report.md`.

## Claude Code, D (context + hook) — primary

| Task | A tokens | D tokens | D/A | A ok | D ok | D runs calling `context` | Pilot rule |
|---|---:|---:|---:|---:|---:|---:|---|
| `django-callers-get-current-timezone` | 28,246 | 39,072 | 1.38 | 3/3 | 3/3 | 0/3 | – |
| `django-callers-prefix-validation-error` | 36,096 | 37,420 | 1.04 | 3/3 | 3/3 | 0/3 | – |
| `django-callers-make-model-tuple` | 41,338 | 33,293 | 0.81 | 3/3 | 3/3 | 0/3 | saving |
| `django-callers-get-default-renderer` | 34,097 | 43,805 | 1.28 | 3/3 | 3/3 | 0/3 | – |
| `django-callers-resolve-url` | 37,757 | 32,511 | 0.86 | 3/3 | 3/3 | 0/3 | saving |
| `django-impact-conditional-escape` | 18,803 | 26,271 | 1.40 | 3/3 | 3/3 | 0/3 | – |
| `django-trap-load-backend` | 36,163 | 38,227 | 1.06 | 3/3 | 3/3 | 0/3 | – |
| `sklearn-callers-compute-sample-weight` | 40,064 | 39,865 | 1.00 | 3/3 | 3/3 | 2/3 | – |
| `sklearn-callers-softmax` | 37,507 | 65,928 | 1.76 | 3/3 | 3/3 | 0/3 | – |
| `sklearn-callers-label-binarize` | 44,516 | 37,666 | 0.85 | 3/3 | 3/3 | 3/3 | saving |
| `sklearn-callers-device` | 62,330 | 66,654 | 1.07 | 3/3 | 3/3 | 2/3 | – |
| `sklearn-callers-supported-float-dtypes` | 43,509 | 37,827 | 0.87 | 3/3 | 3/3 | 0/3 | – |
| `sklearn-impact-check-non-negative` | 20,197 | 43,311 | 2.14 | 3/3 | 3/3 | 0/3 | – |

- D/A median: **1.06 (0.87–1.38, 13 tasks)**
- Success: A 39/39, D 39/39 (91 %–100 % each)
- Other ratios: cost 1.08 (0.92–1.11), output tokens 0.93 (0.89–1.02), turns 1.00
  (0.92–1.29), wall time 1.05 (0.87–1.12)
- By category: callers 1.02 (0.86–1.28, 10 tasks); impact 1.77 (1.40–2.14, 2); trap 1.06
  (1 task)
- By repository: Django 1.06 (0.86–1.38, 7); scikit-learn 1.03 (0.86–1.95, 6)
- Pilot rule met on 3 of 13 tasks

**Decision (amendment 18): no detectable difference. The stopping rule applies.**

### What Claude did with the hook's answers

The hook saw 50 Greps in the 39 D runs. It answered 17 of them, one per run, all with the
caller packet. It let 33 through:

- 15: the pattern was not a single symbol;
- 9: the answer was not shorter than the grep output;
- 6: a non-call reference was among the callers;
- 3: the packet did not claim a complete list.

Claude called `context` itself in 7 runs. CCM answered by call or hook in 20 of 39.

- **No repeated Grep.** With the invitation gone, Claude never repeated the answered Grep:
  0 of 17, against 9 of 19 in amendment 17. So the new call-site answer was never used.
- **The repeat moved to Bash.** After all 17 hook answers, Claude's next step was a Bash
  `grep -rn` for the same symbol. For example: `grep -rn "get_current_timezone\b" django |
  grep -v "def get_current_timezone\b"`. The hook does not see Bash. The verification moved
  from the Grep tool to Bash rather than stopping.
- **Impact tasks cost most:** `check_non_negative` 2.14 and `conditional_escape` 1.40. There
  the grep output is short, and the hook answer plus the Bash re-check came on top of it.

## Codex — replication check

| Task | A tokens | D tokens | D/A | A ok | D ok | D runs calling CCM | Pilot rule |
|---|---:|---:|---:|---:|---:|---:|---|
| `django-callers-get-current-timezone` | 79,962 | 32,983 | 0.41 | 2/3 | 3/3 | 3/3 | saving |
| `django-callers-prefix-validation-error` | 46,431 | 37,186 | 0.80 | 3/3 | 3/3 | 3/3 | saving |
| `django-callers-make-model-tuple` | 60,266 | 46,367 | 0.77 | 3/3 | 3/3 | 3/3 | saving |
| `django-callers-get-default-renderer` | 57,733 | 33,012 | 0.57 | 3/3 | 3/3 | 3/3 | saving |
| `django-callers-resolve-url` | 75,039 | 47,581 | 0.63 | 3/3 | 3/3 | 3/3 | saving |
| `django-impact-conditional-escape` | 32,917 | 55,851 | 1.70 | 3/3 | 3/3 | 3/3 | – |
| `django-trap-load-backend` | 76,082 | 46,001 | 0.60 | 3/3 | 3/3 | 3/3 | saving |
| `sklearn-callers-compute-sample-weight` | 100,061 | 29,494 | 0.29 | 3/3 | 3/3 | 3/3 | saving |
| `sklearn-callers-softmax` | 79,603 | 42,253 | 0.53 | 2/3 | 3/3 | 3/3 | saving |
| `sklearn-callers-label-binarize` | 90,696 | 33,687 | 0.37 | 2/3 | 3/3 | 3/3 | saving |
| `sklearn-callers-device` | 107,989 | 73,834 | 0.68 | 3/3 | 3/3 | 3/3 | saving |
| `sklearn-callers-supported-float-dtypes` | 70,324 | 32,823 | 0.47 | 3/3 | 3/3 | 3/3 | saving |
| `sklearn-impact-check-non-negative` | 39,756 | 51,501 | 1.30 | 3/3 | 3/3 | 3/3 | – |

- D/A median: **0.60 (0.47–0.77, 13 tasks)**
- Success: A 36/39 (80 %–97 %), D 39/39 (91 %–100 %)
- Other ratios: output tokens 0.66 (0.50–0.89), wall time 0.69 (0.60–0.84)
- By category: callers 0.55 (0.41–0.70, 10 tasks); impact 1.50 (1.30–1.70, 2); trap 0.60
  (1 task)
- By repository: Django 0.63 (0.57–0.80, 7); scikit-learn 0.50 (0.33–0.99, 6)
- Pilot rule met on 11 of 13 tasks

**Saving shown, as in amendment 17 (0.72, 0.59–0.89).** The two impact tasks cost more. That
matches amendment 17, where impact was the weakest category for Codex too (0.94).

## Failed runs

All three failures are Codex A runs; every record is kept in `<agent>/runs/`.

| Run | What was missing or wrong |
|---|---|
| Codex `django-callers-get-current-timezone` A#3 | wrote `django/models/query.py:datetimes` for `django/db/models/query.py` |
| Codex `sklearn-callers-label-binarize` A#1 | `sklearn/metrics/_ranking.py:_multiclass_roc_auc_score` |
| Codex `sklearn-callers-softmax` A#2 | named `predict_log_proba` instead of `predict_proba` in `_logistic.py` |

## Owner audit

`scripts/owner_audit.py` scores nothing. It flags passing runs that name an owner the source
does not support:

- Claude `django-callers-make-model-tuple` A#1 and D#1: `StateApps.get_concrete_model_key` and
  `StateApps._find_concrete_model_from_proxy`; both are methods of `ProjectState`.
- Codex `django-trap-load-backend` A#1 and A#2: `PersistentRemoteUserMiddleware.with_perm` and
  `PermissionManager.with_perm`.
- Codex `sklearn-callers-label-binarize` A#3: `_BaseNB.partial_fit`; the method is on
  `_BaseDiscreteNB`.

## What this shows

- **Codex:** CCM's D lowers input tokens on held-out caller and trap questions in large
  repositories, on two independent sets.
  - Amendment 17 measured 0.72 (0.59–0.89); this set measures 0.60 (0.47–0.77).
  - Success is equal or better.
  - Impact questions ("which files call X") are the exception: there D costs more.
- **Claude Code:** the D-only route (`context`, the hook, the hook's repeat handling) gave no
  measurable saving on two held-out sets: 0.94 (0.77–1.10) and now 1.06 (0.87–1.38).
  - The transcripts show why. Claude seldom calls `context` itself (7 of 39 runs here).
  - When the hook answers its Grep, it verifies the answer with its own text search. In
    amendment 17 the re-check was the same Grep; here it is Bash `grep -rn`.
  - The answer is then extra context, not a replacement for the search.
  - Under the stopping rule of amendment 18, this ends the work on Claude Code adoption through
    D-only changes.
