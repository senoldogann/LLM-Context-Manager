# L3 agent benchmark

Scope: pilot design; complete (16/16 runs).
Product profile: `context-first-pilot`. Arms: A=built-ins; B=default context+index_project with context note; C=legacy full surface with legacy navigation note; F=B plus an instruction to call `context` first (forced use, not natural use); H=B plus the opt-in Claude Code Grep hook (installed product configuration, not natural use).

Agent `codex` (codex-cli 0.167.0, auth `subscription`), model `gpt-6-luna` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `ed0f4e8`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 8/8 (100%) | 68%–100% | 1.00 | 1.00 | — | — (8 unknown) | 63,729 | 958 | — | 43 s | — | 0/8 | 0/3 | 0/8 |
| B | 8/8 (100%) | 68%–100% | 1.00 | 1.00 | — | — (8 unknown) | 84,350 | 928 | — | 46 s | 0.1 s | 6/8 | 0/3 | 0/8 |

## Prebuilt indexes

Arms with CCM copy an index built once per repository before their runs (the cache is shared across result directories); the arm table shows the copy time. One-time build time of the index each repository's runs copied:

| Repository | Build |
|---|---|
| django | 4468.6 s |
| express | 82.9 s |
| flask | 455.3 s |
| serde | 183.2 s |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | B / A |
|---|---|
| Cost | — |
| Input tokens (incl. cache) | 1.43 (0.65–1.60, 8 tasks) |
| Output tokens | 1.15 (0.64–1.24, 8 tasks) |
| Turns | — |
| Wall time | 1.10 (0.66–1.69, 8 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | B | B/A |
|---|---|---|---|
| `django-callers-make-aware` | 69,756 | 34,010 | 0.49 |
| `django-trap-capfirst` | 80,186 | 128,336 | 1.60 |
| `express-edit-normalize-type` | 57,702 | 129,434 | 2.24 |
| `express-trap-res-send` | 44,395 | 60,678 | 1.37 |
| `flask-edit-error-handler` | 72,672 | 108,022 | 1.49 |
| `flask-locate-app-string` | 32,393 | 21,000 | 0.65 |
| `serde-edit-wrap-field` | 72,972 | 116,299 | 1.59 |
| `serde-trap-deserialize-struct` | 46,510 | 32,898 | 0.71 |

## Success by task category

| Category | A | B |
|---|---|---|
| callers | 1/1 | 1/1 |
| edit | 3/3 | 3/3 |
| locate | 1/1 | 1/1 |
| trap | 3/3 | 3/3 |

## By task

| Task | A | B | Cost A | Cost B |
|---|---|---|---|---|
| `django-callers-make-aware` | 1/1 | 1/1 | — | — |
| `django-trap-capfirst` | 1/1 | 1/1 | — | — |
| `express-edit-normalize-type` | 1/1 | 1/1 | — | — |
| `express-trap-res-send` | 1/1 | 1/1 | — | — |
| `flask-edit-error-handler` | 1/1 | 1/1 | — | — |
| `flask-locate-app-string` | 1/1 | 1/1 | — | — |
| `serde-edit-wrap-field` | 1/1 | 1/1 | — | — |
| `serde-trap-deserialize-struct` | 1/1 | 1/1 | — | — |

## Failed runs

None.
