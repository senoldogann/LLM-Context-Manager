# L3 agent benchmark

Scope: pilot design; complete (72/72 runs).
Product profile: `preregistered`. Arms: A=built-ins; B=preregistered full CCM surface; C=preregistered search_code only; D=current product (`context` + `index_project` with the context note; amendment 14).

Agent `codex` (codex-cli 0.167.0, auth `subscription`), model `gpt-6-luna` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `5eba8f0`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 34/36 (94%) | 82%–98% | 0.96 | 1.00 | — | — (36 unknown) | 47,928 | 917 | — | 38 s | — | 0/36 | 0/0 | 0/36 |
| D | 32/36 (89%) | 75%–96% | 0.94 | 1.00 | — | — (36 unknown) | 34,171 | 553 | — | 30 s | 1.0 s | 36/36 | 0/0 | 0/36 |

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
| Input tokens (incl. cache) | 0.91 (0.68–1.02, 12 tasks) |
| Output tokens | 0.63 (0.50–0.81, 12 tasks) |
| Turns | — |
| Wall time | 0.78 (0.70–0.97, 12 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | D | D/A |
|---|---|---|---|
| `django-callers-call-command` | 52,823 | 37,390 | 0.71 |
| `django-callers-flatten-fieldsets` | 63,231 | 41,639 | 0.66 |
| `django-impact-get-current-site` | 33,852 | 37,569 | 1.11 |
| `django-impact-get-user-model` | 32,493 | 37,257 | 1.15 |
| `django-impact-safe-join` | 31,528 | 29,158 | 0.92 |
| `django-trap-select-template` | 57,806 | 59,436 | 1.03 |
| `sklearn-callers-as-float-array` | 92,921 | 33,534 | 0.36 |
| `sklearn-callers-compute-class-weight` | 64,845 | 37,730 | 0.58 |
| `sklearn-impact-accuracy-score` | 39,044 | 34,022 | 0.87 |
| `sklearn-impact-get-scorer-names` | 50,165 | 51,239 | 1.02 |
| `sklearn-impact-pairwise-distances` | 39,088 | 35,301 | 0.90 |
| `sklearn-impact-svd-flip` | 40,323 | 37,851 | 0.94 |

## Success by task category

| Category | A | D |
|---|---|---|
| callers | 10/12 | 11/12 |
| files | 21/21 | 18/21 |
| trap | 3/3 | 3/3 |

## By task

| Task | A | D | Cost A | Cost D |
|---|---|---|---|---|
| `django-callers-call-command` | 1/3 | 2/3 | — | — |
| `django-callers-flatten-fieldsets` | 3/3 | 3/3 | — | — |
| `django-impact-get-current-site` | 3/3 | 3/3 | — | — |
| `django-impact-get-user-model` | 3/3 | 0/3 | — | — |
| `django-impact-safe-join` | 3/3 | 3/3 | — | — |
| `django-trap-select-template` | 3/3 | 3/3 | — | — |
| `sklearn-callers-as-float-array` | 3/3 | 3/3 | — | — |
| `sklearn-callers-compute-class-weight` | 3/3 | 3/3 | — | — |
| `sklearn-impact-accuracy-score` | 3/3 | 3/3 | — | — |
| `sklearn-impact-get-scorer-names` | 3/3 | 3/3 | — | — |
| `sklearn-impact-pairwise-distances` | 3/3 | 3/3 | — | — |
| `sklearn-impact-svd-flip` | 3/3 | 3/3 | — | — |

## Failed runs

- `django-callers-call-command` A#1: missing django/test/runner.py:run_checks, django/test/testcases.py:_fixture_setup, django/test/testcases.py:_fixture_teardown, django/test/testcases.py:setUpClass
- `django-callers-call-command` A#2: missing django/test/runner.py:run_checks, django/test/testcases.py:_fixture_setup, django/test/testcases.py:_fixture_teardown, django/test/testcases.py:setUpClass
- `django-callers-call-command` D#1: missing django/test/runner.py:run_checks, django/test/testcases.py:_fixture_setup, django/test/testcases.py:_fixture_teardown, django/test/testcases.py:setUpClass
- `django-impact-get-user-model` D#1: missing django/contrib/auth/forms.py, django/contrib/auth/handlers/modwsgi.py, django/contrib/auth/management/commands/changepassword.py, django/contrib/auth/views.py
- `django-impact-get-user-model` D#2: missing django/contrib/auth/forms.py, django/contrib/auth/handlers/modwsgi.py, django/contrib/auth/management/commands/changepassword.py, django/contrib/auth/views.py
- `django-impact-get-user-model` D#3: missing django/contrib/auth/forms.py, django/contrib/auth/handlers/modwsgi.py, django/contrib/auth/management/commands/changepassword.py, django/contrib/auth/views.py
