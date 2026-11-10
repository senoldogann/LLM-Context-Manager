# L3 agent benchmark

Scope: pilot design; complete (36/36 runs).
Product profile: `context-first-pilot`. Arms: A=built-ins; B=default context+index_project with context note; C=legacy full surface with legacy navigation note; F=B plus an instruction to call `context` first (forced use, not natural use); H=B plus the opt-in Claude Code Grep hook (installed product configuration, not natural use).

Agent `claude` (2.1.319 (Claude Code), auth `subscription`), model `claude-opus-5-5` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `e805025`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 9/9 (100%) | 70%–100% | 1.00 | 1.00 | $0.088 | $0.72 | 37,987 | 1,431 | 4 | 17 s | — | 0/9 | 0/0 | 0/9 |
| B | 9/9 (100%) | 70%–100% | 1.00 | 0.98 | $0.081 | $0.76 | 39,082 | 1,556 | 4 | 18 s | 0.1 s | 1/9 | 0/0 | 0/9 |
| F | 9/9 (100%) | 70%–100% | 1.00 | 1.00 | $0.078 | $0.67 | 29,891 | 1,186 | 3 | 15 s | 0.1 s | 9/9 | 0/0 | 0/9 |
| H | 9/9 (100%) | 70%–100% | 1.00 | 0.98 | $0.076 | $0.73 | 38,324 | 1,255 | 4 | 12 s | 0.1 s | 2/9 | 0/0 | 0/9 |

## Prebuilt indexes

Arms with CCM copy an index built once per repository before their runs (the cache is shared across result directories); the arm table shows the copy time. One-time build time of the index each repository's runs copied:

| Repository | Build |
|---|---|
| django | 1230.5 s |
| tokio | 202.7 s |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | B / A | F / A | H / A |
|---|---|---|---|
| Cost | 1.01 (0.95–1.28, 3 tasks) | 0.92 (0.84–1.10, 3 tasks) | 0.97 (0.96–1.15, 3 tasks) |
| Input tokens (incl. cache) | 0.91 (0.84–1.30, 3 tasks) | 0.87 (0.66–1.33, 3 tasks) | 0.91 (0.77–1.22, 3 tasks) |
| Output tokens | 1.07 (0.91–1.25, 3 tasks) | 0.77 (0.71–1.28, 3 tasks) | 0.97 (0.89–1.10, 3 tasks) |
| Turns | 0.94 (0.91–1.33, 3 tasks) | 0.82 (0.65–1.22, 3 tasks) | 0.91 (0.82–1.11, 3 tasks) |
| Wall time | 1.08 (0.92–1.27, 3 tasks) | 0.79 (0.76–1.42, 3 tasks) | 1.10 (0.79–1.25, 3 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | B | F | H | B/A | F/A | H/A |
|---|---|---|---|---|---|---|---|
| `django-trap-urlencode` | 53,061 | 48,458 | 35,166 | 48,521 | 0.91 | 0.66 | 0.91 |
| `tokio-callers-sleep-until` | 34,274 | 28,889 | 29,926 | 26,259 | 0.84 | 0.87 | 0.77 |
| `tokio-trap-catch-unwind` | 25,809 | 33,452 | 34,288 | 31,552 | 1.30 | 1.33 | 1.22 |

## Success by task category

| Category | A | B | F | H |
|---|---|---|---|---|
| callers | 3/3 | 3/3 | 3/3 | 3/3 |
| trap | 6/6 | 6/6 | 6/6 | 6/6 |

## By task

| Task | A | B | F | H | Cost A | Cost B | Cost F | Cost H |
|---|---|---|---|---|---|---|---|---|
| `django-trap-urlencode` | 3/3 | 3/3 | 3/3 | 3/3 | $0.097 | $0.098 | $0.081 | $0.093 |
| `tokio-callers-sleep-until` | 3/3 | 3/3 | 3/3 | 3/3 | $0.086 | $0.081 | $0.079 | $0.083 |
| `tokio-trap-catch-unwind` | 3/3 | 3/3 | 3/3 | 3/3 | $0.058 | $0.074 | $0.064 | $0.066 |

## Hook decisions (arm H)

Greps answered with the `context` packet (deny) and greps let through (pass), per run.

| Task | Run | Deny | Pass |
|---|---|---|---|
| `django-trap-urlencode` | 1 | 0 | 1 |
| `django-trap-urlencode` | 2 | 0 | 2 |
| `django-trap-urlencode` | 3 | 0 | 1 |
| `tokio-callers-sleep-until` | 1 | 0 | 1 |
| `tokio-callers-sleep-until` | 2 | 0 | 1 |
| `tokio-callers-sleep-until` | 3 | 0 | 1 |
| `tokio-trap-catch-unwind` | 1 | 0 | 1 |
| `tokio-trap-catch-unwind` | 2 | 0 | 2 |
| `tokio-trap-catch-unwind` | 3 | 0 | 1 |

## Failed runs

None.
