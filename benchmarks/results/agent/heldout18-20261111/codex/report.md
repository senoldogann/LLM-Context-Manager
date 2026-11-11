# L3 agent benchmark

Scope: pilot design; complete (78/78 runs).
Product profile: `preregistered`. Arms: A=built-ins; B=preregistered full CCM surface; C=preregistered search_code only; D=current product (`context` + `index_project` with the context note; amendment 14).

Agent `codex` (codex-cli 0.167.0, auth `subscription`), model `gpt-6-luna` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `ad29ffc`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 36/39 (92%) | 80%–97% | 0.99 | 0.99 | — | — (39 unknown) | 65,691 | 1,179 | — | 31 s | — | 0/39 | 0/0 | 0/39 |
| D | 39/39 (100%) | 91%–100% | 1.00 | 1.00 | — | — (39 unknown) | 33,697 | 737 | — | 24 s | 0.6 s | 39/39 | 0/0 | 0/39 |

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
| Cost | — |
| Input tokens (incl. cache) | 0.60 (0.47–0.77, 13 tasks) |
| Output tokens | 0.66 (0.50–0.89, 13 tasks) |
| Turns | — |
| Wall time | 0.69 (0.60–0.84, 13 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | D | D/A |
|---|---|---|---|
| `django-callers-get-current-timezone` | 79,962 | 32,983 | 0.41 |
| `django-callers-get-default-renderer` | 57,733 | 33,012 | 0.57 |
| `django-callers-make-model-tuple` | 60,266 | 46,367 | 0.77 |
| `django-callers-prefix-validation-error` | 46,431 | 37,186 | 0.80 |
| `django-callers-resolve-url` | 75,039 | 47,581 | 0.63 |
| `django-impact-conditional-escape` | 32,917 | 55,851 | 1.70 |
| `django-trap-load-backend` | 76,082 | 46,001 | 0.60 |
| `sklearn-callers-compute-sample-weight` | 100,061 | 29,494 | 0.29 |
| `sklearn-callers-device` | 107,989 | 73,834 | 0.68 |
| `sklearn-callers-label-binarize` | 90,696 | 33,687 | 0.37 |
| `sklearn-callers-softmax` | 79,603 | 42,253 | 0.53 |
| `sklearn-callers-supported-float-dtypes` | 70,324 | 32,823 | 0.47 |
| `sklearn-impact-check-non-negative` | 39,756 | 51,501 | 1.30 |

## Success by task category

| Category | A | D |
|---|---|---|
| callers | 27/30 | 30/30 |
| files | 6/6 | 6/6 |
| trap | 3/3 | 3/3 |

## By task

| Task | A | D | Cost A | Cost D |
|---|---|---|---|---|
| `django-callers-get-current-timezone` | 2/3 | 3/3 | — | — |
| `django-callers-get-default-renderer` | 3/3 | 3/3 | — | — |
| `django-callers-make-model-tuple` | 3/3 | 3/3 | — | — |
| `django-callers-prefix-validation-error` | 3/3 | 3/3 | — | — |
| `django-callers-resolve-url` | 3/3 | 3/3 | — | — |
| `django-impact-conditional-escape` | 3/3 | 3/3 | — | — |
| `django-trap-load-backend` | 3/3 | 3/3 | — | — |
| `sklearn-callers-compute-sample-weight` | 3/3 | 3/3 | — | — |
| `sklearn-callers-device` | 3/3 | 3/3 | — | — |
| `sklearn-callers-label-binarize` | 2/3 | 3/3 | — | — |
| `sklearn-callers-softmax` | 2/3 | 3/3 | — | — |
| `sklearn-callers-supported-float-dtypes` | 3/3 | 3/3 | — | — |
| `sklearn-impact-check-non-negative` | 3/3 | 3/3 | — | — |

## Failed runs

- `django-callers-get-current-timezone` A#3: missing django/db/models/query.py:datetimes; extra django/models/query.py:datetimes
- `sklearn-callers-label-binarize` A#1: missing sklearn/metrics/_ranking.py:_multiclass_roc_auc_score
- `sklearn-callers-softmax` A#2: missing sklearn/linear_model/_logistic.py:predict_proba; extra sklearn/linear_model/_logistic.py:predict_log_proba
