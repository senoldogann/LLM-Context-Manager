# L3 agent benchmark

Scope: pilot design; complete (24/24 runs).
Product profile: `context-first-pilot`. Arms: A=built-ins; B=default context+index_project with context note; C=legacy full surface with legacy navigation note; F=B plus an instruction to call `context` first (forced use, not natural use); H=B plus the opt-in Claude Code Grep hook (installed product configuration, not natural use).

Agent `claude` (2.1.319 (Claude Code), auth `subscription`), model `claude-opus-5-5` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `516ed4c`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 6/6 (100%) | 61%–100% | 1.00 | 1.00 | $0.081 | $0.52 | 44,672 | 1,548 | 5 | 19 s | — | 0/6 | 0/0 | 0/6 |
| B | 6/6 (100%) | 61%–100% | 1.00 | 1.00 | $0.075 | $0.48 | 34,197 | 1,211 | 4 | 14 s | 0.4 s | 3/6 | 0/0 | 0/6 |
| F | 6/6 (100%) | 61%–100% | 1.00 | 1.00 | $0.079 | $0.48 | 31,433 | 1,144 | 4 | 15 s | 0.4 s | 6/6 | 0/0 | 0/6 |
| H | 6/6 (100%) | 61%–100% | 1.00 | 1.00 | $0.077 | $0.49 | 34,580 | 1,274 | 4 | 16 s | 0.4 s | 3/6 | 0/0 | 0/6 |

## Prebuilt indexes

Arms with CCM copy an index built once per repository before their runs (the cache is shared across result directories); the arm table shows the copy time. One-time build time of the index each repository's runs copied:

| Repository | Build |
|---|---|
| django | 1230.5 s |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | B / A | F / A | H / A |
|---|---|---|---|
| Cost | 0.90 (0.75–1.05, 2 tasks) | 0.91 (0.73–1.09, 2 tasks) | 0.92 (0.71–1.13, 2 tasks) |
| Input tokens (incl. cache) | 0.79 (0.51–1.08, 2 tasks) | 0.84 (0.58–1.10, 2 tasks) | 0.89 (0.43–1.34, 2 tasks) |
| Output tokens | 0.81 (0.60–1.02, 2 tasks) | 0.75 (0.59–0.90, 2 tasks) | 0.82 (0.58–1.06, 2 tasks) |
| Turns | 0.86 (0.71–1.00, 2 tasks) | 0.89 (0.57–1.21, 2 tasks) | 0.93 (0.64–1.21, 2 tasks) |
| Wall time | 0.76 (0.59–0.93, 2 tasks) | 0.79 (0.70–0.88, 2 tasks) | 0.84 (0.59–1.10, 2 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | B | F | H | B/A | F/A | H/A |
|---|---|---|---|---|---|---|---|
| `django-callers-constant-time-compare` | 41,399 | 21,259 | 23,815 | 17,687 | 0.51 | 0.58 | 0.43 |
| `django-trap-urlencode` | 43,876 | 47,230 | 48,211 | 58,959 | 1.08 | 1.10 | 1.34 |

## Success by task category

| Category | A | B | F | H |
|---|---|---|---|---|
| callers | 3/3 | 3/3 | 3/3 | 3/3 |
| trap | 3/3 | 3/3 | 3/3 | 3/3 |

## By task

| Task | A | B | F | H | Cost A | Cost B | Cost F | Cost H |
|---|---|---|---|---|---|---|---|---|
| `django-callers-constant-time-compare` | 3/3 | 3/3 | 3/3 | 3/3 | $0.078 | $0.059 | $0.057 | $0.056 |
| `django-trap-urlencode` | 3/3 | 3/3 | 3/3 | 3/3 | $0.095 | $0.100 | $0.103 | $0.107 |

## Hook decisions (arm H)

Greps answered with the `context` packet (deny) and greps let through (pass), per run.

| Task | Run | Deny | Pass |
|---|---|---|---|
| `django-callers-constant-time-compare` | 1 | 1 | 0 |
| `django-callers-constant-time-compare` | 2 | 1 | 0 |
| `django-callers-constant-time-compare` | 3 | 1 | 0 |
| `django-trap-urlencode` | 1 | 0 | 2 |
| `django-trap-urlencode` | 2 | 0 | 1 |
| `django-trap-urlencode` | 3 | 0 | 2 |

## Failed runs

None.
