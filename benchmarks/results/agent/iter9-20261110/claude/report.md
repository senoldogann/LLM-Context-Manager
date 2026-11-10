# L3 agent benchmark

Scope: pilot design; complete (36/36 runs).
Product profile: `context-first-pilot`. Arms: A=built-ins; B=default context+index_project with context note; C=legacy full surface with legacy navigation note; F=B plus an instruction to call `context` first (forced use, not natural use); H=B plus the opt-in Claude Code Grep hook (installed product configuration, not natural use).

Agent `claude` (2.1.319 (Claude Code), auth `subscription`), model `claude-opus-5-5` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `31502d6`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 9/9 (100%) | 70%–100% | 1.00 | 1.00 | $0.081 | $0.73 | 37,301 | 1,216 | 4 | 15 s | — | 0/9 | 0/0 | 0/9 |
| B | 9/9 (100%) | 70%–100% | 1.00 | 1.00 | $0.072 | $0.69 | 27,471 | 1,193 | 4 | 13 s | 0.1 s | 7/9 | 0/0 | 0/9 |
| F | 9/9 (100%) | 70%–100% | 1.00 | 1.00 | $0.063 | $0.62 | 27,700 | 1,094 | 3 | 13 s | 0.1 s | 9/9 | 0/0 | 0/9 |
| H | 9/9 (100%) | 70%–100% | 1.00 | 1.00 | $0.076 | $0.71 | 29,776 | 1,173 | 4 | 15 s | 0.1 s | 7/9 | 0/0 | 0/9 |

## Prebuilt indexes

Arms with CCM copy an index built once per repository before their runs (the cache is shared across result directories); the arm table shows the copy time. One-time build time of the index each repository's runs copied:

| Repository | Build |
|---|---|
| tokio | 265.3 s |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | B / A | F / A | H / A |
|---|---|---|---|
| Cost | 0.93 (0.89–1.09, 3 tasks) | 0.77 (0.61–1.31, 3 tasks) | 1.10 (0.74–1.20, 3 tasks) |
| Input tokens (incl. cache) | 0.93 (0.50–1.18, 3 tasks) | 0.72 (0.63–1.58, 3 tasks) | 1.06 (0.75–1.25, 3 tasks) |
| Output tokens | 1.03 (0.75–1.19, 3 tasks) | 0.91 (0.60–1.14, 3 tasks) | 1.05 (0.77–1.19, 3 tasks) |
| Turns | 0.92 (0.82–1.56, 3 tasks) | 0.75 (0.73–1.33, 3 tasks) | 1.08 (1.00–1.22, 3 tasks) |
| Wall time | 1.00 (0.69–1.12, 3 tasks) | 0.82 (0.77–1.24, 3 tasks) | 1.10 (0.82–1.19, 3 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | B | F | H | B/A | F/A | H/A |
|---|---|---|---|---|---|---|---|
| `tokio-callers-async-op` | 38,298 | 19,221 | 24,317 | 28,803 | 0.50 | 0.63 | 0.75 |
| `tokio-callers-poll-proceed` | 25,772 | 30,362 | 40,716 | 32,269 | 1.18 | 1.58 | 1.25 |
| `tokio-callers-with-scheduler` | 37,457 | 34,750 | 26,904 | 39,717 | 0.93 | 0.72 | 1.06 |

## Success by task category

| Category | A | B | F | H |
|---|---|---|---|---|
| callers | 9/9 | 9/9 | 9/9 | 9/9 |

## By task

| Task | A | B | F | H | Cost A | Cost B | Cost F | Cost H |
|---|---|---|---|---|---|---|---|---|
| `tokio-callers-async-op` | 3/3 | 3/3 | 3/3 | 3/3 | $0.098 | $0.087 | $0.060 | $0.072 |
| `tokio-callers-poll-proceed` | 3/3 | 3/3 | 3/3 | 3/3 | $0.064 | $0.070 | $0.084 | $0.077 |
| `tokio-callers-with-scheduler` | 3/3 | 3/3 | 3/3 | 3/3 | $0.080 | $0.075 | $0.062 | $0.089 |

## Hook decisions (arm H)

Greps answered with the `context` packet (deny) and greps let through (pass), per run.

| Task | Run | Deny | Pass |
|---|---|---|---|
| `tokio-callers-async-op` | 1 | 1 | 1 |
| `tokio-callers-async-op` | 2 | 1 | 0 |
| `tokio-callers-async-op` | 3 | 1 | 0 |
| `tokio-callers-poll-proceed` | 1 | 0 | 1 |
| `tokio-callers-poll-proceed` | 2 | 0 | 2 |
| `tokio-callers-poll-proceed` | 3 | 0 | 2 |
| `tokio-callers-with-scheduler` | 1 | 0 | 1 |
| `tokio-callers-with-scheduler` | 2 | 0 | 1 |
| `tokio-callers-with-scheduler` | 3 | 0 | 2 |

## Failed runs

None.
