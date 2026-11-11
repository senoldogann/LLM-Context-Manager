# L3 agent benchmark

Scope: pilot design; complete (78/78 runs).
Product profile: `preregistered`. Arms: A=built-ins; B=preregistered full CCM surface; C=preregistered search_code only; D=current product (`context` + `index_project` with the context note; amendment 14).

Agent `codex` (codex-cli 0.167.0, auth `subscription`), model `gpt-6-luna` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `68cec67`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 35/39 (90%) | 76%–96% | 0.99 | 0.98 | — | — (39 unknown) | 52,392 | 1,087 | — | 28 s | — | 0/39 | 0/0 | 0/39 |
| D | 38/39 (97%) | 87%–100% | 1.00 | 1.00 | — | — (39 unknown) | 45,823 | 845 | — | 22 s | 0.2 s | 38/39 | 0/0 | 0/39 |

## Prebuilt indexes

Arms with CCM copy an index built once per repository before their runs (the cache is shared across result directories); the arm table shows the copy time. One-time build time of the index each repository's runs copied:

| Repository | Build |
|---|---|
| django | 1140.6 s |
| scikit-learn | 578.0 s |
| tokio | 236.7 s |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | D / A |
|---|---|
| Cost | — |
| Input tokens (incl. cache) | 0.72 (0.59–0.89, 13 tasks) |
| Output tokens | 0.77 (0.67–0.85, 13 tasks) |
| Turns | — |
| Wall time | 0.82 (0.72–0.95, 13 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | D | D/A |
|---|---|---|---|
| `django-callers-get-default-timezone` | 63,677 | 37,533 | 0.59 |
| `django-callers-get-text-list` | 75,714 | 47,038 | 0.62 |
| `django-callers-log-response` | 56,859 | 50,574 | 0.89 |
| `django-impact-get-docs-version` | 42,334 | 37,136 | 0.88 |
| `django-trap-resolve` | 127,244 | 72,128 | 0.57 |
| `sklearn-callers-empirical-covariance` | 61,360 | 34,089 | 0.56 |
| `sklearn-callers-gen-even-slices` | 65,688 | 44,108 | 0.67 |
| `sklearn-callers-indexable` | 79,866 | 41,819 | 0.52 |
| `sklearn-impact-check-scoring` | 42,257 | 41,954 | 0.99 |
| `sklearn-trap-normalize` | 82,998 | 59,499 | 0.72 |
| `tokio-callers-globals` | 33,585 | 71,377 | 2.13 |
| `tokio-callers-uninit` | 44,155 | 34,491 | 0.78 |
| `tokio-trap-new-with-interest-and-handle` | 38,292 | 51,168 | 1.34 |

## Success by task category

| Category | A | D |
|---|---|---|
| callers | 23/24 | 24/24 |
| files | 6/6 | 6/6 |
| trap | 6/9 | 8/9 |

## By task

| Task | A | D | Cost A | Cost D |
|---|---|---|---|---|
| `django-callers-get-default-timezone` | 3/3 | 3/3 | — | — |
| `django-callers-get-text-list` | 3/3 | 3/3 | — | — |
| `django-callers-log-response` | 3/3 | 3/3 | — | — |
| `django-impact-get-docs-version` | 3/3 | 3/3 | — | — |
| `django-trap-resolve` | 0/3 | 3/3 | — | — |
| `sklearn-callers-empirical-covariance` | 2/3 | 3/3 | — | — |
| `sklearn-callers-gen-even-slices` | 3/3 | 3/3 | — | — |
| `sklearn-callers-indexable` | 3/3 | 3/3 | — | — |
| `sklearn-impact-check-scoring` | 3/3 | 3/3 | — | — |
| `sklearn-trap-normalize` | 3/3 | 2/3 | — | — |
| `tokio-callers-globals` | 3/3 | 3/3 | — | — |
| `tokio-callers-uninit` | 3/3 | 3/3 | — | — |
| `tokio-trap-new-with-interest-and-handle` | 3/3 | 3/3 | — | — |

## Failed runs

- `django-trap-resolve` A#1: missing django/test/client.py:request
- `django-trap-resolve` A#2: missing django/test/client.py:request
- `django-trap-resolve` A#3: missing django/test/client.py:request
- `sklearn-callers-empirical-covariance` A#3: extra sklearn/covariance/_robust_covariance.py:_c_step, sklearn/covariance/_robust_covariance.py:fit, sklearn/covariance/_robust_covariance.py:reweight_covariance
- `sklearn-trap-normalize` D#2: missing sklearn/datasets/_twenty_newsgroups.py:fetch_20newsgroups_vectorized
