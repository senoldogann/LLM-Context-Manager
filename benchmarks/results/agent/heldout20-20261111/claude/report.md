# L3 agent benchmark

Scope: pilot design; complete (117/117 runs).
Product profile: `preregistered`. Arms: A=built-ins; B=preregistered full CCM surface; C=preregistered search_code only; D=current product (`context` + `index_project` with the context note; amendment 14).

Agent `claude` (2.1.319 (Claude Code), auth `subscription`), model `claude-opus-5-5` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `403f36c`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 39/39 (100%) | 91%–100% | 1.00 | 1.00 | $0.075 | $2.93 | 35,911 | 1,231 | 4 | 16 s | — | 0/39 | 0/0 | 0/39 |
| D | 39/39 (100%) | 91%–100% | 1.00 | 1.00 | $0.071 | $2.83 | 34,903 | 1,154 | 4 | 15 s | 0.6 s | 14/39 | 0/0 | 0/39 |
| E | 39/39 (100%) | 91%–100% | 1.00 | 1.00 | $0.076 | $2.95 | 39,796 | 1,223 | 5 | 16 s | 0.5 s | 15/39 | 0/0 | 0/39 |

## Prebuilt indexes

Arms with CCM copy an index built once per repository before their runs (the cache is shared across result directories); the arm table shows the copy time. One-time build time of the index each repository's runs copied:

| Repository | Build |
|---|---|
| django | 3239.5 s |
| scikit-learn | 1014.5 s |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | D / A | E / A |
|---|---|---|
| Cost | 0.98 (0.90–0.99, 13 tasks) | 1.04 (0.93–1.07, 13 tasks) |
| Input tokens (incl. cache) | 1.02 (0.94–1.06, 13 tasks) | 1.13 (1.07–1.16, 13 tasks) |
| Output tokens | 0.95 (0.79–0.98, 13 tasks) | 0.97 (0.88–1.07, 13 tasks) |
| Turns | 1.00 (0.92–1.08, 13 tasks) | 1.14 (1.08–1.18, 13 tasks) |
| Wall time | 1.00 (0.89–1.05, 13 tasks) | 1.08 (1.00–1.11, 13 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | D | E | D/A | E/A |
|---|---|---|---|---|---|
| `django-callers-get-version-tuple` | 35,286 | 36,954 | 39,205 | 1.05 | 1.11 |
| `django-callers-make-naive` | 34,304 | 36,060 | 39,623 | 1.05 | 1.16 |
| `django-callers-number-format` | 31,834 | 33,656 | 34,491 | 1.06 | 1.08 |
| `django-callers-strip-quotes` | 40,956 | 41,823 | 47,611 | 1.02 | 1.16 |
| `django-callers-validate-file-name` | 28,942 | 28,837 | 35,301 | 1.00 | 1.22 |
| `django-impact-is-naive` | 24,283 | 22,705 | 25,901 | 0.94 | 1.07 |
| `django-trap-resolve-relation` | 44,211 | 43,874 | 50,766 | 0.99 | 1.15 |
| `sklearn-callers-check-scalar` | 31,273 | 22,877 | 23,200 | 0.73 | 0.74 |
| `sklearn-callers-check-x-y` | 46,077 | 33,671 | 41,547 | 0.73 | 0.90 |
| `sklearn-callers-has-fit-parameter` | 37,521 | 40,192 | 38,701 | 1.07 | 1.03 |
| `sklearn-callers-pairwise-distances-chunked` | 37,618 | 30,845 | 42,564 | 0.82 | 1.13 |
| `sklearn-callers-pairwise-kernels` | 30,538 | 38,841 | 43,564 | 1.27 | 1.43 |
| `sklearn-impact-randomized-svd` | 26,927 | 36,089 | 39,883 | 1.34 | 1.48 |

## Success by task category

| Category | A | D | E |
|---|---|---|---|
| callers | 30/30 | 30/30 | 30/30 |
| files | 6/6 | 6/6 | 6/6 |
| trap | 3/3 | 3/3 | 3/3 |

## By task

| Task | A | D | E | Cost A | Cost D | Cost E |
|---|---|---|---|---|---|---|
| `django-callers-get-version-tuple` | 3/3 | 3/3 | 3/3 | $0.072 | $0.068 | $0.068 |
| `django-callers-make-naive` | 3/3 | 3/3 | 3/3 | $0.067 | $0.066 | $0.070 |
| `django-callers-number-format` | 3/3 | 3/3 | 3/3 | $0.065 | $0.065 | $0.070 |
| `django-callers-strip-quotes` | 3/3 | 3/3 | 3/3 | $0.091 | $0.080 | $0.079 |
| `django-callers-validate-file-name` | 3/3 | 3/3 | 3/3 | $0.061 | $0.049 | $0.056 |
| `django-impact-is-naive` | 3/3 | 3/3 | 3/3 | $0.053 | $0.052 | $0.057 |
| `django-trap-resolve-relation` | 3/3 | 3/3 | 3/3 | $0.087 | $0.095 | $0.099 |
| `sklearn-callers-check-scalar` | 3/3 | 3/3 | 3/3 | $0.090 | $0.081 | $0.078 |
| `sklearn-callers-check-x-y` | 3/3 | 3/3 | 3/3 | $0.091 | $0.080 | $0.084 |
| `sklearn-callers-has-fit-parameter` | 3/3 | 3/3 | 3/3 | $0.080 | $0.074 | $0.076 |
| `sklearn-callers-pairwise-distances-chunked` | 3/3 | 3/3 | 3/3 | $0.078 | $0.077 | $0.084 |
| `sklearn-callers-pairwise-kernels` | 3/3 | 3/3 | 3/3 | $0.073 | $0.088 | $0.091 |
| `sklearn-impact-randomized-svd` | 3/3 | 3/3 | 3/3 | $0.069 | $0.071 | $0.074 |

## Hook decisions (arms with the Grep hook)

Greps answered with the `context` packet (deny) and greps let through (pass), per run.

| Task | Arm | Run | Deny | Pass |
|---|---|---|---|---|
| `django-callers-get-version-tuple` | D | 1 | 1 | 0 |
| `django-callers-get-version-tuple` | D | 2 | 1 | 0 |
| `django-callers-get-version-tuple` | D | 3 | 1 | 0 |
| `django-callers-get-version-tuple` | E | 1 | 1 | 2 |
| `django-callers-get-version-tuple` | E | 2 | 1 | 2 |
| `django-callers-get-version-tuple` | E | 3 | 1 | 3 |
| `django-callers-make-naive` | D | 1 | 0 | 1 |
| `django-callers-make-naive` | D | 2 | 0 | 1 |
| `django-callers-make-naive` | D | 3 | 0 | 1 |
| `django-callers-make-naive` | E | 1 | 0 | 3 |
| `django-callers-make-naive` | E | 2 | 0 | 3 |
| `django-callers-make-naive` | E | 3 | 0 | 4 |
| `django-callers-number-format` | D | 1 | 0 | 1 |
| `django-callers-number-format` | D | 2 | 0 | 2 |
| `django-callers-number-format` | D | 3 | 0 | 1 |
| `django-callers-number-format` | E | 1 | 0 | 1 |
| `django-callers-number-format` | E | 2 | 0 | 3 |
| `django-callers-number-format` | E | 3 | 0 | 5 |
| `django-callers-strip-quotes` | D | 1 | 1 | 0 |
| `django-callers-strip-quotes` | D | 2 | 1 | 0 |
| `django-callers-strip-quotes` | D | 3 | 1 | 0 |
| `django-callers-strip-quotes` | E | 1 | 2 | 0 |
| `django-callers-strip-quotes` | E | 2 | 2 | 4 |
| `django-callers-strip-quotes` | E | 3 | 2 | 2 |
| `django-callers-validate-file-name` | D | 1 | 1 | 0 |
| `django-callers-validate-file-name` | D | 2 | 1 | 0 |
| `django-callers-validate-file-name` | D | 3 | 1 | 0 |
| `django-callers-validate-file-name` | E | 1 | 1 | 3 |
| `django-callers-validate-file-name` | E | 2 | 1 | 1 |
| `django-callers-validate-file-name` | E | 3 | 2 | 1 |
| `django-impact-is-naive` | D | 1 | 0 | 2 |
| `django-impact-is-naive` | D | 2 | 0 | 2 |
| `django-impact-is-naive` | D | 3 | 0 | 1 |
| `django-impact-is-naive` | E | 1 | 0 | 2 |
| `django-impact-is-naive` | E | 2 | 0 | 2 |
| `django-impact-is-naive` | E | 3 | 0 | 3 |
| `django-trap-resolve-relation` | D | 1 | 0 | 2 |
| `django-trap-resolve-relation` | D | 2 | 0 | 2 |
| `django-trap-resolve-relation` | D | 3 | 0 | 1 |
| `django-trap-resolve-relation` | E | 1 | 0 | 4 |
| `django-trap-resolve-relation` | E | 2 | 0 | 3 |
| `django-trap-resolve-relation` | E | 3 | 0 | 5 |
| `sklearn-callers-check-scalar` | D | 1 | 0 | 1 |
| `sklearn-callers-check-scalar` | D | 2 | 0 | 1 |
| `sklearn-callers-check-scalar` | D | 3 | 0 | 1 |
| `sklearn-callers-check-scalar` | E | 1 | 0 | 1 |
| `sklearn-callers-check-scalar` | E | 2 | 0 | 2 |
| `sklearn-callers-check-scalar` | E | 3 | 0 | 1 |
| `sklearn-callers-check-x-y` | D | 1 | 0 | 1 |
| `sklearn-callers-check-x-y` | D | 2 | 1 | 0 |
| `sklearn-callers-check-x-y` | D | 3 | 0 | 1 |
| `sklearn-callers-check-x-y` | E | 1 | 1 | 2 |
| `sklearn-callers-check-x-y` | E | 2 | 1 | 2 |
| `sklearn-callers-check-x-y` | E | 3 | 1 | 2 |
| `sklearn-callers-has-fit-parameter` | D | 1 | 0 | 1 |
| `sklearn-callers-has-fit-parameter` | D | 2 | 1 | 0 |
| `sklearn-callers-has-fit-parameter` | D | 3 | 1 | 0 |
| `sklearn-callers-has-fit-parameter` | E | 1 | 2 | 1 |
| `sklearn-callers-has-fit-parameter` | E | 2 | 1 | 2 |
| `sklearn-callers-has-fit-parameter` | E | 3 | 1 | 2 |
| `sklearn-callers-pairwise-distances-chunked` | D | 1 | 1 | 0 |
| `sklearn-callers-pairwise-distances-chunked` | D | 2 | 1 | 0 |
| `sklearn-callers-pairwise-distances-chunked` | D | 3 | 0 | 1 |
| `sklearn-callers-pairwise-distances-chunked` | E | 1 | 2 | 1 |
| `sklearn-callers-pairwise-distances-chunked` | E | 2 | 2 | 1 |
| `sklearn-callers-pairwise-distances-chunked` | E | 3 | 1 | 2 |
| `sklearn-callers-pairwise-kernels` | D | 1 | 1 | 0 |
| `sklearn-callers-pairwise-kernels` | D | 2 | 1 | 0 |
| `sklearn-callers-pairwise-kernels` | D | 3 | 1 | 0 |
| `sklearn-callers-pairwise-kernels` | E | 1 | 1 | 2 |
| `sklearn-callers-pairwise-kernels` | E | 2 | 1 | 2 |
| `sklearn-callers-pairwise-kernels` | E | 3 | 1 | 2 |
| `sklearn-impact-randomized-svd` | D | 1 | 1 | 0 |
| `sklearn-impact-randomized-svd` | D | 2 | 1 | 0 |
| `sklearn-impact-randomized-svd` | D | 3 | 1 | 0 |
| `sklearn-impact-randomized-svd` | E | 1 | 1 | 2 |
| `sklearn-impact-randomized-svd` | E | 2 | 2 | 2 |
| `sklearn-impact-randomized-svd` | E | 3 | 1 | 2 |

## Failed runs

None.
