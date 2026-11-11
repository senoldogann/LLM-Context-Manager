# L3 agent benchmark

Scope: pilot design; complete (108/108 runs).
Product profile: `preregistered`. Arms: A=built-ins; B=preregistered full CCM surface; C=preregistered search_code only; D=current product (`context` + `index_project` with the context note; amendment 14).

Agent `codex` (codex-cli 0.167.0, auth `subscription`), model `gpt-6-luna` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `01b2afa`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 44/54 (81%) | 69%–90% | 0.90 | 0.96 | — | — (54 unknown) | 50,300 | 1,019 | — | 26 s | — | 0/54 | 0/0 | 0/54 |
| D | 46/54 (85%) | 73%–92% | 0.93 | 0.96 | — | — (54 unknown) | 45,831 | 854 | — | 23 s | 0.3 s | 54/54 | 0/0 | 0/54 |

## Prebuilt indexes

Arms with CCM copy an index built once per repository before their runs (the cache is shared across result directories); the arm table shows the copy time. One-time build time of the index each repository's runs copied:

| Repository | Build |
|---|---|
| django | 1562.7 s |
| scikit-learn | 898.4 s |
| tokio | 265.3 s |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | D / A |
|---|---|
| Cost | — |
| Input tokens (incl. cache) | 0.78 (0.58–1.23, 18 tasks) |
| Output tokens | 0.71 (0.60–1.10, 18 tasks) |
| Turns | — |
| Wall time | 0.82 (0.73–1.14, 18 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | D | D/A |
|---|---|---|---|
| `django-callers-constant-time-compare` | 56,292 | 32,489 | 0.58 |
| `django-callers-iri-to-uri` | 55,315 | 29,423 | 0.53 |
| `django-callers-is-aware` | 114,032 | 60,635 | 0.53 |
| `django-callers-make-hashable` | 79,571 | 33,027 | 0.42 |
| `django-impact-get-resolver` | 37,587 | 60,948 | 1.62 |
| `django-trap-get-connection` | 54,464 | 49,222 | 0.90 |
| `django-trap-urlencode` | 130,362 | 43,555 | 0.33 |
| `sklearn-callers-check-cv` | 45,361 | 86,259 | 1.90 |
| `sklearn-callers-gen-batches` | 61,338 | 39,125 | 0.64 |
| `sklearn-impact-get-config` | 38,894 | 55,670 | 1.43 |
| `sklearn-trap-shuffle` | 134,615 | 73,086 | 0.54 |
| `tokio-callers-async-op` | 50,001 | 32,951 | 0.66 |
| `tokio-callers-new-unnamed` | 41,631 | 33,018 | 0.79 |
| `tokio-callers-sleep-until` | 51,041 | 60,806 | 1.19 |
| `tokio-callers-unbounded-channel` | 47,753 | 70,573 | 1.48 |
| `tokio-callers-with-scheduler` | 47,526 | 36,878 | 0.78 |
| `tokio-impact-enter-runtime` | 35,421 | 32,692 | 0.92 |
| `tokio-trap-try-with` | 51,903 | 66,227 | 1.28 |

## Success by task category

| Category | A | D |
|---|---|---|
| callers | 27/33 | 30/33 |
| files | 5/9 | 6/9 |
| trap | 12/12 | 10/12 |

## By task

| Task | A | D | Cost A | Cost D |
|---|---|---|---|---|
| `django-callers-constant-time-compare` | 3/3 | 3/3 | — | — |
| `django-callers-iri-to-uri` | 3/3 | 3/3 | — | — |
| `django-callers-is-aware` | 0/3 | 3/3 | — | — |
| `django-callers-make-hashable` | 2/3 | 1/3 | — | — |
| `django-impact-get-resolver` | 3/3 | 3/3 | — | — |
| `django-trap-get-connection` | 3/3 | 3/3 | — | — |
| `django-trap-urlencode` | 3/3 | 3/3 | — | — |
| `sklearn-callers-check-cv` | 3/3 | 3/3 | — | — |
| `sklearn-callers-gen-batches` | 3/3 | 3/3 | — | — |
| `sklearn-impact-get-config` | 0/3 | 0/3 | — | — |
| `sklearn-trap-shuffle` | 3/3 | 1/3 | — | — |
| `tokio-callers-async-op` | 3/3 | 3/3 | — | — |
| `tokio-callers-new-unnamed` | 3/3 | 3/3 | — | — |
| `tokio-callers-sleep-until` | 2/3 | 3/3 | — | — |
| `tokio-callers-unbounded-channel` | 3/3 | 3/3 | — | — |
| `tokio-callers-with-scheduler` | 2/3 | 2/3 | — | — |
| `tokio-impact-enter-runtime` | 2/3 | 3/3 | — | — |
| `tokio-trap-try-with` | 3/3 | 3/3 | — | — |

## Failed runs

- `django-callers-is-aware` A#1: missing django/utils/timezone.py:make_aware
- `django-callers-is-aware` A#2: no answer block; missing django/contrib/admin/filters.py:__init__, django/contrib/admin/templatetags/admin_list.py:date_hierarchy, django/contrib/humanize/templatetags/humanize.py:string_for, django/core/serializers/json.py:default, django/db/backends/base/operations.py:adapt_timefield_value, django/db/backends/mysql/operations.py:adapt_datetimefield_value, django/db/backends/mysql/operations.py:adapt_timefield_value, django/db/backends/oracle/operations.py:adapt_datetimefield_value, django/db/backends/oracle/operations.py:adapt_timefield_value, django/db/backends/sqlite3/operations.py:adapt_datetimefield_value, django/db/backends/sqlite3/operations.py:adapt_timefield_value, django/db/backends/sqlite3/operations.py:convert_datetimefield_value, django/db/models/fields/__init__.py:_to_naive, django/db/models/fields/__init__.py:to_python, django/forms/utils.py:to_current_timezone, django/utils/timesince.py:timesince, django/utils/timezone.py:make_aware, django/views/decorators/http.py:condition
- `django-callers-is-aware` A#3: missing django/contrib/humanize/templatetags/humanize.py:string_for, django/core/serializers/json.py:default, django/utils/timesince.py:timesince, django/utils/timezone.py:make_aware
- `django-callers-make-hashable` A#1: missing django/utils/hashable.py:make_hashable
- `django-callers-make-hashable` D#1: missing django/utils/hashable.py:make_hashable
- `django-callers-make-hashable` D#2: missing django/utils/hashable.py:make_hashable
- `sklearn-impact-get-config` A#1: missing sklearn/base.py, sklearn/metrics/_pairwise_distances_reduction/_dispatcher.py, sklearn/utils/_array_api.py, sklearn/utils/_chunking.py, sklearn/utils/_metadata_requests.py, sklearn/utils/_param_validation.py, sklearn/utils/_pprint.py, sklearn/utils/_set_output.py, sklearn/utils/parallel.py, sklearn/utils/validation.py
- `sklearn-impact-get-config` A#2: missing sklearn/base.py, sklearn/metrics/_pairwise_distances_reduction/_dispatcher.py, sklearn/utils/_array_api.py, sklearn/utils/_chunking.py, sklearn/utils/_metadata_requests.py, sklearn/utils/_param_validation.py, sklearn/utils/_pprint.py, sklearn/utils/_set_output.py, sklearn/utils/parallel.py, sklearn/utils/validation.py
- `sklearn-impact-get-config` A#3: missing sklearn/_config.py, sklearn/base.py, sklearn/metrics/_pairwise_distances_reduction/_dispatcher.py, sklearn/utils/_array_api.py, sklearn/utils/_chunking.py, sklearn/utils/_metadata_requests.py, sklearn/utils/_param_validation.py, sklearn/utils/_pprint.py, sklearn/utils/_set_output.py, sklearn/utils/parallel.py, sklearn/utils/validation.py
- `sklearn-impact-get-config` D#1: missing sklearn/utils/validation.py
- `sklearn-impact-get-config` D#2: missing sklearn/_config.py, sklearn/base.py, sklearn/metrics/_pairwise_distances_reduction/_dispatcher.py, sklearn/utils/_array_api.py, sklearn/utils/_chunking.py, sklearn/utils/_metadata_requests.py, sklearn/utils/_param_validation.py, sklearn/utils/_pprint.py, sklearn/utils/_set_output.py, sklearn/utils/parallel.py, sklearn/utils/validation.py
- `sklearn-impact-get-config` D#3: missing sklearn/_config.py, sklearn/base.py, sklearn/metrics/_pairwise_distances_reduction/_dispatcher.py, sklearn/utils/_array_api.py, sklearn/utils/_chunking.py, sklearn/utils/_metadata_requests.py, sklearn/utils/_param_validation.py, sklearn/utils/_pprint.py, sklearn/utils/_set_output.py, sklearn/utils/parallel.py, sklearn/utils/validation.py
- `sklearn-trap-shuffle` D#2: missing sklearn/datasets/_kddcup99.py:fetch_kddcup99, sklearn/datasets/_rcv1.py:fetch_rcv1, sklearn/datasets/_samples_generator.py:make_classification, sklearn/datasets/_samples_generator.py:make_regression, sklearn/datasets/_samples_generator.py:make_circles, sklearn/datasets/_samples_generator.py:make_moons, sklearn/datasets/_samples_generator.py:make_blobs, sklearn/datasets/_samples_generator.py:make_gaussian_quantiles
- `sklearn-trap-shuffle` D#3: missing sklearn/datasets/_kddcup99.py:fetch_kddcup99, sklearn/datasets/_rcv1.py:fetch_rcv1, sklearn/datasets/_samples_generator.py:make_classification, sklearn/datasets/_samples_generator.py:make_regression, sklearn/datasets/_samples_generator.py:make_circles, sklearn/datasets/_samples_generator.py:make_moons, sklearn/datasets/_samples_generator.py:make_blobs, sklearn/datasets/_samples_generator.py:make_gaussian_quantiles
- `tokio-callers-sleep-until` A#3: missing tokio-test/src/stream_mock.rs:poll_next, tokio-test/src/io.rs:poll_read, tokio-test/src/io.rs:poll_write, tokio-util/src/time/delay_queue.rs:insert_at, tokio-util/src/time/delay_queue.rs:remove, tokio-util/src/time/delay_queue.rs:poll_idx, tokio/src/time/interval.rs:internal_interval_at, tokio/src/time/timeout.rs:timeout_at
- `tokio-callers-with-scheduler` A#2: missing tokio/src/runtime/context.rs:defer
- `tokio-callers-with-scheduler` D#3: missing tokio/src/runtime/scheduler/current_thread/mod.rs:schedule, tokio/src/runtime/scheduler/current_thread/mod.rs:unhandled_panic; extra tokio/src/runtime/scheduler/current_thread/mod.rs:Arc
- `tokio-impact-enter-runtime` A#2: missing tokio/src/runtime/scheduler/multi_thread/mod.rs; extra tokio/src/runtime/scheduler/multi_thread/ mod.rs
