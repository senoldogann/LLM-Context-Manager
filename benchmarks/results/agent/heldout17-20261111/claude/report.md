# L3 agent benchmark

Scope: pilot design; complete (78/78 runs).
Product profile: `preregistered`. Arms: A=built-ins; B=preregistered full CCM surface; C=preregistered search_code only; D=current product (`context` + `index_project` with the context note; amendment 14).

Agent `claude` (2.1.319 (Claude Code), auth `subscription`), model `claude-opus-5-5` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `68cec67`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 38/39 (97%) | 87%–100% | 1.00 | 1.00 | $0.077 | $3.28 | 35,377 | 1,332 | 4 | 17 s | — | 0/39 | 0/0 | 0/39 |
| D | 39/39 (100%) | 91%–100% | 1.00 | 1.00 | $0.076 | $3.07 | 31,205 | 1,188 | 4 | 15 s | 0.2 s | 7/39 | 0/0 | 0/39 |

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
| Cost | 0.95 (0.88–1.08, 13 tasks) |
| Input tokens (incl. cache) | 0.94 (0.77–1.10, 13 tasks) |
| Output tokens | 0.90 (0.79–0.97, 13 tasks) |
| Turns | 0.92 (0.75–1.09, 13 tasks) |
| Wall time | 0.90 (0.79–1.04, 13 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | D | D/A |
|---|---|---|---|
| `django-callers-get-default-timezone` | 31,384 | 37,323 | 1.19 |
| `django-callers-get-text-list` | 39,154 | 26,051 | 0.67 |
| `django-callers-log-response` | 31,274 | 33,115 | 1.06 |
| `django-impact-get-docs-version` | 22,161 | 29,740 | 1.34 |
| `django-trap-resolve` | 78,185 | 53,256 | 0.68 |
| `sklearn-callers-empirical-covariance` | 31,419 | 29,030 | 0.92 |
| `sklearn-callers-gen-even-slices` | 36,115 | 27,954 | 0.77 |
| `sklearn-callers-indexable` | 39,192 | 31,994 | 0.82 |
| `sklearn-impact-check-scoring` | 25,888 | 34,456 | 1.33 |
| `sklearn-trap-normalize` | 81,466 | 41,651 | 0.51 |
| `tokio-callers-globals` | 35,375 | 33,325 | 0.94 |
| `tokio-callers-uninit` | 42,961 | 47,264 | 1.10 |
| `tokio-trap-new-with-interest-and-handle` | 34,409 | 32,528 | 0.95 |

## Success by task category

| Category | A | D |
|---|---|---|
| callers | 24/24 | 24/24 |
| files | 6/6 | 6/6 |
| trap | 8/9 | 9/9 |

## By task

| Task | A | D | Cost A | Cost D |
|---|---|---|---|---|
| `django-callers-get-default-timezone` | 3/3 | 3/3 | $0.071 | $0.076 |
| `django-callers-get-text-list` | 3/3 | 3/3 | $0.078 | $0.053 |
| `django-callers-log-response` | 3/3 | 3/3 | $0.064 | $0.080 |
| `django-impact-get-docs-version` | 3/3 | 3/3 | $0.051 | $0.057 |
| `django-trap-resolve` | 2/3 | 3/3 | $0.125 | $0.110 |
| `sklearn-callers-empirical-covariance` | 3/3 | 3/3 | $0.087 | $0.083 |
| `sklearn-callers-gen-even-slices` | 3/3 | 3/3 | $0.073 | $0.071 |
| `sklearn-callers-indexable` | 3/3 | 3/3 | $0.089 | $0.074 |
| `sklearn-impact-check-scoring` | 3/3 | 3/3 | $0.066 | $0.084 |
| `sklearn-trap-normalize` | 3/3 | 3/3 | $0.147 | $0.107 |
| `tokio-callers-globals` | 3/3 | 3/3 | $0.069 | $0.063 |
| `tokio-callers-uninit` | 3/3 | 3/3 | $0.099 | $0.091 |
| `tokio-trap-new-with-interest-and-handle` | 3/3 | 3/3 | $0.077 | $0.073 |

## Hook decisions (arms with the Grep hook)

Greps answered with the `context` packet (deny) and greps let through (pass), per run.

| Task | Arm | Run | Deny | Pass |
|---|---|---|---|---|
| `django-callers-get-default-timezone` | D | 1 | 0 | 1 |
| `django-callers-get-default-timezone` | D | 2 | 0 | 1 |
| `django-callers-get-default-timezone` | D | 3 | 0 | 1 |
| `django-callers-get-text-list` | D | 1 | 1 | 1 |
| `django-callers-get-text-list` | D | 2 | 1 | 1 |
| `django-callers-get-text-list` | D | 3 | 1 | 1 |
| `django-callers-log-response` | D | 1 | 0 | 1 |
| `django-callers-log-response` | D | 2 | 0 | 1 |
| `django-callers-log-response` | D | 3 | 0 | 1 |
| `django-impact-get-docs-version` | D | 1 | 1 | 1 |
| `django-impact-get-docs-version` | D | 2 | 1 | 2 |
| `django-impact-get-docs-version` | D | 3 | 1 | 2 |
| `django-trap-resolve` | D | 1 | 0 | 2 |
| `django-trap-resolve` | D | 2 | 0 | 2 |
| `django-trap-resolve` | D | 3 | 0 | 2 |
| `sklearn-callers-empirical-covariance` | D | 1 | 0 | 1 |
| `sklearn-callers-empirical-covariance` | D | 2 | 0 | 1 |
| `sklearn-callers-empirical-covariance` | D | 3 | 0 | 1 |
| `sklearn-callers-gen-even-slices` | D | 1 | 1 | 1 |
| `sklearn-callers-gen-even-slices` | D | 2 | 1 | 1 |
| `sklearn-callers-gen-even-slices` | D | 3 | 1 | 1 |
| `sklearn-callers-indexable` | D | 1 | 1 | 2 |
| `sklearn-callers-indexable` | D | 2 | 0 | 1 |
| `sklearn-callers-indexable` | D | 3 | 1 | 1 |
| `sklearn-impact-check-scoring` | D | 1 | 1 | 1 |
| `sklearn-impact-check-scoring` | D | 2 | 1 | 0 |
| `sklearn-impact-check-scoring` | D | 3 | 1 | 1 |
| `sklearn-trap-normalize` | D | 1 | 1 | 2 |
| `sklearn-trap-normalize` | D | 2 | 1 | 1 |
| `sklearn-trap-normalize` | D | 3 | 1 | 2 |
| `tokio-callers-globals` | D | 1 | 0 | 1 |
| `tokio-callers-globals` | D | 2 | 1 | 1 |
| `tokio-callers-globals` | D | 3 | 1 | 1 |
| `tokio-callers-uninit` | D | 1 | 0 | 2 |
| `tokio-callers-uninit` | D | 2 | 0 | 2 |
| `tokio-callers-uninit` | D | 3 | 0 | 2 |
| `tokio-trap-new-with-interest-and-handle` | D | 1 | 0 | 1 |
| `tokio-trap-new-with-interest-and-handle` | D | 2 | 0 | 1 |
| `tokio-trap-new-with-interest-and-handle` | D | 3 | 0 | 1 |

## Failed runs

- `django-trap-resolve` A#1: missing django/views/debug.py:get_caller; extra django/views/debug.py:technical_404_response
