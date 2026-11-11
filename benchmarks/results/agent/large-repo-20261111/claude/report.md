# L3 agent benchmark

Scope: pilot design; complete (108/108 runs).
Product profile: `preregistered`. Arms: A=built-ins; B=preregistered full CCM surface; C=preregistered search_code only; D=current product (`context` + `index_project` with the context note; amendment 14).

Agent `claude` (2.1.319 (Claude Code), auth `subscription`), model `claude-opus-5-5` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `01b2afa`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 53/54 (98%) | 90%–100% | 1.00 | 0.99 | $0.084 | $4.59 | 35,596 | 1,532 | 4 | 18 s | — | 0/54 | 0/0 | 0/54 |
| D | 54/54 (100%) | 93%–100% | 1.00 | 0.99 | $0.077 | $4.44 | 28,062 | 1,270 | 3 | 15 s | 0.2 s | 21/54 | 0/0 | 0/54 |

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
| Cost | 1.02 (0.92–1.07, 18 tasks) |
| Input tokens (incl. cache) | 0.85 (0.71–1.05, 18 tasks) |
| Output tokens | 0.88 (0.77–1.04, 18 tasks) |
| Turns | 0.93 (0.82–1.00, 18 tasks) |
| Wall time | 0.82 (0.73–0.93, 18 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | D | D/A |
|---|---|---|---|
| `django-callers-constant-time-compare` | 35,615 | 37,106 | 1.04 |
| `django-callers-iri-to-uri` | 35,369 | 18,656 | 0.53 |
| `django-callers-is-aware` | 36,731 | 26,660 | 0.73 |
| `django-callers-make-hashable` | 29,262 | 39,795 | 1.36 |
| `django-impact-get-resolver` | 18,816 | 18,124 | 0.96 |
| `django-trap-get-connection` | 27,684 | 32,106 | 1.16 |
| `django-trap-urlencode` | 49,914 | 40,870 | 0.82 |
| `sklearn-callers-check-cv` | 27,452 | 20,769 | 0.76 |
| `sklearn-callers-gen-batches` | 38,789 | 34,351 | 0.89 |
| `sklearn-impact-get-config` | 19,543 | 18,430 | 0.94 |
| `sklearn-trap-shuffle` | 68,388 | 78,369 | 1.15 |
| `tokio-callers-async-op` | 42,890 | 28,963 | 0.68 |
| `tokio-callers-new-unnamed` | 46,739 | 25,381 | 0.54 |
| `tokio-callers-sleep-until` | 44,502 | 31,491 | 0.71 |
| `tokio-callers-unbounded-channel` | 26,636 | 28,997 | 1.09 |
| `tokio-callers-with-scheduler` | 41,007 | 28,535 | 0.70 |
| `tokio-impact-enter-runtime` | 15,507 | 16,554 | 1.07 |
| `tokio-trap-try-with` | 41,909 | 28,831 | 0.69 |

## Success by task category

| Category | A | D |
|---|---|---|
| callers | 32/33 | 33/33 |
| files | 9/9 | 9/9 |
| trap | 12/12 | 12/12 |

## By task

| Task | A | D | Cost A | Cost D |
|---|---|---|---|---|
| `django-callers-constant-time-compare` | 3/3 | 3/3 | $0.082 | $0.075 |
| `django-callers-iri-to-uri` | 3/3 | 3/3 | $0.074 | $0.079 |
| `django-callers-is-aware` | 3/3 | 3/3 | $0.086 | $0.079 |
| `django-callers-make-hashable` | 3/3 | 3/3 | $0.079 | $0.093 |
| `django-impact-get-resolver` | 3/3 | 3/3 | $0.050 | $0.061 |
| `django-trap-get-connection` | 3/3 | 3/3 | $0.075 | $0.076 |
| `django-trap-urlencode` | 3/3 | 3/3 | $0.094 | $0.096 |
| `sklearn-callers-check-cv` | 3/3 | 3/3 | $0.086 | $0.092 |
| `sklearn-callers-gen-batches` | 3/3 | 3/3 | $0.099 | $0.104 |
| `sklearn-impact-get-config` | 3/3 | 3/3 | $0.062 | $0.073 |
| `sklearn-trap-shuffle` | 3/3 | 3/3 | $0.146 | $0.152 |
| `tokio-callers-async-op` | 3/3 | 3/3 | $0.102 | $0.074 |
| `tokio-callers-new-unnamed` | 3/3 | 3/3 | $0.103 | $0.080 |
| `tokio-callers-sleep-until` | 3/3 | 3/3 | $0.101 | $0.081 |
| `tokio-callers-unbounded-channel` | 3/3 | 3/3 | $0.063 | $0.069 |
| `tokio-callers-with-scheduler` | 2/3 | 3/3 | $0.083 | $0.076 |
| `tokio-impact-enter-runtime` | 3/3 | 3/3 | $0.044 | $0.044 |
| `tokio-trap-try-with` | 3/3 | 3/3 | $0.102 | $0.077 |

## Failed runs

- `tokio-callers-with-scheduler` A#2: missing tokio/src/runtime/scheduler/current_thread/mod.rs:schedule, tokio/src/runtime/scheduler/current_thread/mod.rs:unhandled_panic; extra tokio/src/runtime/scheduler/current_thread/mod.rs:Arc
