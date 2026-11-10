# L3 agent benchmark

Scope: pilot design; complete (18/18 runs).
Product profile: `context-first-pilot`. Arms: A=built-ins; B=default context+index_project with context note; C=legacy full surface with legacy navigation note; F=B plus an instruction to call `context` first (forced use, not natural use); H=B plus the opt-in Claude Code Grep hook (installed product configuration, not natural use).

Agent `codex` (codex-cli 0.167.0, auth `subscription`), model `gpt-6-luna` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `4c5f193`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 9/9 (100%) | 70%–100% | 1.00 | 1.00 | — | — (9 unknown) | 56,474 | 818 | — | 25 s | — | 0/9 | 0/0 | 0/9 |
| B | 9/9 (100%) | 70%–100% | 1.00 | 1.00 | — | — (9 unknown) | 92,878 | 1,482 | — | 51 s | 0.1 s | 8/9 | 0/0 | 0/9 |

## Prebuilt indexes

Arms with CCM copy an index built once per repository before their runs (the cache is shared across result directories); the arm table shows the copy time. One-time build time of the index each repository's runs copied:

| Repository | Build |
|---|---|
| tokio | 239.8 s |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | B / A |
|---|---|
| Cost | — |
| Input tokens (incl. cache) | 1.46 (1.29–2.28, 3 tasks) |
| Output tokens | 1.38 (1.32–2.05, 3 tasks) |
| Turns | — |
| Wall time | 2.31 (1.35–2.62, 3 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | B | B/A |
|---|---|---|---|
| `tokio-callers-async-op` | 53,196 | 121,252 | 2.28 |
| `tokio-trap-catch-unwind` | 38,526 | 49,625 | 1.29 |
| `tokio-trap-yield-now` | 80,154 | 117,158 | 1.46 |

## Success by task category

| Category | A | B |
|---|---|---|
| callers | 3/3 | 3/3 |
| trap | 6/6 | 6/6 |

## By task

| Task | A | B | Cost A | Cost B |
|---|---|---|---|---|
| `tokio-callers-async-op` | 3/3 | 3/3 | — | — |
| `tokio-trap-catch-unwind` | 3/3 | 3/3 | — | — |
| `tokio-trap-yield-now` | 3/3 | 3/3 | — | — |

## Failed runs

None.
