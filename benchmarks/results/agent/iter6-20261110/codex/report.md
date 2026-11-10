# L3 agent benchmark

Scope: pilot design; complete (12/12 runs).
Product profile: `context-first-pilot`. Arms: A=built-ins; B=default context+index_project with context note; C=legacy full surface with legacy navigation note; F=B plus an instruction to call `context` first (forced use, not natural use); H=B plus the opt-in Claude Code Grep hook (installed product configuration, not natural use).

Agent `codex` (codex-cli 0.167.0, auth `subscription`), model `gpt-6-luna` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `516ed4c`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 6/6 (100%) | 61%–100% | 1.00 | 1.00 | — | — (6 unknown) | 69,619 | 1,011 | — | 28 s | — | 0/6 | 0/0 | 0/6 |
| B | 6/6 (100%) | 61%–100% | 1.00 | 0.98 | — | — (6 unknown) | 40,099 | 790 | — | 32 s | 0.4 s | 6/6 | 0/0 | 0/6 |

## Prebuilt indexes

Arms with CCM copy an index built once per repository before their runs (the cache is shared across result directories); the arm table shows the copy time. One-time build time of the index each repository's runs copied:

| Repository | Build |
|---|---|
| django | 1230.5 s |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | B / A |
|---|---|
| Cost | — |
| Input tokens (incl. cache) | 0.59 (0.51–0.67, 2 tasks) |
| Output tokens | 0.79 (0.72–0.86, 2 tasks) |
| Turns | — |
| Wall time | 0.88 (0.83–0.92, 2 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | B | B/A |
|---|---|---|---|
| `django-callers-constant-time-compare` | 47,936 | 32,338 | 0.67 |
| `django-trap-urlencode` | 115,445 | 59,233 | 0.51 |

## Success by task category

| Category | A | B |
|---|---|---|
| callers | 3/3 | 3/3 |
| trap | 3/3 | 3/3 |

## By task

| Task | A | B | Cost A | Cost B |
|---|---|---|---|---|
| `django-callers-constant-time-compare` | 3/3 | 3/3 | — | — |
| `django-trap-urlencode` | 3/3 | 3/3 | — | — |

## Failed runs

None.
