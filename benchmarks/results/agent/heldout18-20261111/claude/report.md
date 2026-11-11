# L3 agent benchmark

Scope: pilot design; complete (78/78 runs).
Product profile: `preregistered`. Arms: A=built-ins; B=preregistered full CCM surface; C=preregistered search_code only; D=current product (`context` + `index_project` with the context note; amendment 14).

Agent `claude` (2.1.319 (Claude Code), auth `subscription`), model `claude-opus-5-5` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `ad29ffc`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 39/39 (100%) | 91%–100% | 1.00 | 1.00 | $0.077 | $3.00 | 36,593 | 1,237 | 4 | 16 s | — | 0/39 | 0/0 | 0/39 |
| D | 39/39 (100%) | 91%–100% | 1.00 | 1.00 | $0.072 | $3.04 | 36,596 | 1,185 | 4 | 16 s | 0.6 s | 7/39 | 0/0 | 0/39 |

## Prebuilt indexes

Arms with CCM copy an index built once per repository before their runs (the cache is shared across result directories); the arm table shows the copy time. One-time build time of the index each repository's runs copied:

| Repository | Build |
|---|---|
| django | 1140.6 s |
| scikit-learn | 578.0 s |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | D / A |
|---|---|
| Cost | 1.08 (0.92–1.11, 13 tasks) |
| Input tokens (incl. cache) | 1.06 (0.87–1.38, 13 tasks) |
| Output tokens | 0.93 (0.89–1.02, 13 tasks) |
| Turns | 1.00 (0.92–1.29, 13 tasks) |
| Wall time | 1.05 (0.87–1.12, 13 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | D | D/A |
|---|---|---|---|
| `django-callers-get-current-timezone` | 28,246 | 39,072 | 1.38 |
| `django-callers-get-default-renderer` | 34,097 | 43,805 | 1.28 |
| `django-callers-make-model-tuple` | 41,338 | 33,293 | 0.81 |
| `django-callers-prefix-validation-error` | 36,096 | 37,420 | 1.04 |
| `django-callers-resolve-url` | 37,757 | 32,511 | 0.86 |
| `django-impact-conditional-escape` | 18,803 | 26,271 | 1.40 |
| `django-trap-load-backend` | 36,163 | 38,227 | 1.06 |
| `sklearn-callers-compute-sample-weight` | 40,064 | 39,865 | 1.00 |
| `sklearn-callers-device` | 62,330 | 66,654 | 1.07 |
| `sklearn-callers-label-binarize` | 44,516 | 37,666 | 0.85 |
| `sklearn-callers-softmax` | 37,507 | 65,928 | 1.76 |
| `sklearn-callers-supported-float-dtypes` | 43,509 | 37,827 | 0.87 |
| `sklearn-impact-check-non-negative` | 20,197 | 43,311 | 2.14 |

## Success by task category

| Category | A | D |
|---|---|---|
| callers | 30/30 | 30/30 |
| files | 6/6 | 6/6 |
| trap | 3/3 | 3/3 |

## By task

| Task | A | D | Cost A | Cost D |
|---|---|---|---|---|
| `django-callers-get-current-timezone` | 3/3 | 3/3 | $0.064 | $0.077 |
| `django-callers-get-default-renderer` | 3/3 | 3/3 | $0.063 | $0.070 |
| `django-callers-make-model-tuple` | 3/3 | 3/3 | $0.073 | $0.067 |
| `django-callers-prefix-validation-error` | 3/3 | 3/3 | $0.068 | $0.073 |
| `django-callers-resolve-url` | 3/3 | 3/3 | $0.085 | $0.063 |
| `django-impact-conditional-escape` | 3/3 | 3/3 | $0.050 | $0.054 |
| `django-trap-load-backend` | 3/3 | 3/3 | $0.075 | $0.077 |
| `sklearn-callers-compute-sample-weight` | 3/3 | 3/3 | $0.087 | $0.080 |
| `sklearn-callers-device` | 3/3 | 3/3 | $0.117 | $0.128 |
| `sklearn-callers-label-binarize` | 3/3 | 3/3 | $0.092 | $0.078 |
| `sklearn-callers-softmax` | 3/3 | 3/3 | $0.080 | $0.096 |
| `sklearn-callers-supported-float-dtypes` | 3/3 | 3/3 | $0.088 | $0.077 |
| `sklearn-impact-check-non-negative` | 3/3 | 3/3 | $0.059 | $0.073 |

## Hook decisions (arms with the Grep hook)

Greps answered with the `context` packet (deny) and greps let through (pass), per run.

| Task | Arm | Run | Deny | Pass |
|---|---|---|---|---|
| `django-callers-get-current-timezone` | D | 1 | 1 | 0 |
| `django-callers-get-current-timezone` | D | 2 | 1 | 0 |
| `django-callers-get-current-timezone` | D | 3 | 1 | 0 |
| `django-callers-get-default-renderer` | D | 1 | 0 | 1 |
| `django-callers-get-default-renderer` | D | 2 | 0 | 1 |
| `django-callers-get-default-renderer` | D | 3 | 0 | 1 |
| `django-callers-make-model-tuple` | D | 1 | 0 | 1 |
| `django-callers-make-model-tuple` | D | 2 | 0 | 1 |
| `django-callers-make-model-tuple` | D | 3 | 0 | 1 |
| `django-callers-prefix-validation-error` | D | 1 | 0 | 1 |
| `django-callers-prefix-validation-error` | D | 2 | 0 | 1 |
| `django-callers-prefix-validation-error` | D | 3 | 0 | 1 |
| `django-callers-resolve-url` | D | 1 | 1 | 0 |
| `django-callers-resolve-url` | D | 2 | 1 | 0 |
| `django-callers-resolve-url` | D | 3 | 1 | 0 |
| `django-impact-conditional-escape` | D | 1 | 0 | 2 |
| `django-impact-conditional-escape` | D | 2 | 0 | 3 |
| `django-impact-conditional-escape` | D | 3 | 0 | 1 |
| `django-trap-load-backend` | D | 1 | 0 | 1 |
| `django-trap-load-backend` | D | 2 | 0 | 1 |
| `django-trap-load-backend` | D | 3 | 0 | 1 |
| `sklearn-callers-compute-sample-weight` | D | 1 | 1 | 0 |
| `sklearn-callers-compute-sample-weight` | D | 2 | 1 | 0 |
| `sklearn-callers-compute-sample-weight` | D | 3 | 1 | 0 |
| `sklearn-callers-device` | D | 1 | 0 | 4 |
| `sklearn-callers-device` | D | 2 | 0 | 5 |
| `sklearn-callers-device` | D | 3 | 0 | 2 |
| `sklearn-callers-label-binarize` | D | 1 | 1 | 0 |
| `sklearn-callers-label-binarize` | D | 2 | 1 | 0 |
| `sklearn-callers-label-binarize` | D | 3 | 0 | 1 |
| `sklearn-callers-softmax` | D | 1 | 1 | 0 |
| `sklearn-callers-softmax` | D | 2 | 1 | 0 |
| `sklearn-callers-softmax` | D | 3 | 1 | 0 |
| `sklearn-callers-supported-float-dtypes` | D | 1 | 0 | 1 |
| `sklearn-callers-supported-float-dtypes` | D | 2 | 0 | 1 |
| `sklearn-callers-supported-float-dtypes` | D | 3 | 0 | 1 |
| `sklearn-impact-check-non-negative` | D | 1 | 1 | 0 |
| `sklearn-impact-check-non-negative` | D | 2 | 1 | 0 |
| `sklearn-impact-check-non-negative` | D | 3 | 1 | 0 |

## Failed runs

None.
