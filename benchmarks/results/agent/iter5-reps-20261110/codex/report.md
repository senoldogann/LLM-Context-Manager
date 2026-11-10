# L3 agent benchmark

Scope: pilot design; complete (24/24 runs).
Product profile: `context-first-pilot`. Arms: A=built-ins; B=default context+index_project with context note; C=legacy full surface with legacy navigation note; F=B plus an instruction to call `context` first (forced use, not natural use); H=B plus the opt-in Claude Code Grep hook (installed product configuration, not natural use).

Agent `codex` (codex-cli 0.167.0, auth `subscription`), model `gpt-6-luna` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `9bb6564`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 12/12 (100%) | 76%–100% | 1.00 | 1.00 | — | — (12 unknown) | 38,046 | 520 | — | 19 s | — | 0/12 | 0/3 | 0/12 |
| B | 12/12 (100%) | 76%–100% | 1.00 | 1.00 | — | — (12 unknown) | 49,870 | 616 | — | 22 s | 0.0 s | 12/12 | 0/3 | 0/12 |

## Prebuilt indexes

Arms with CCM copy an index built once per repository before their runs (the cache is shared across result directories); the arm table shows the copy time. One-time build time of the index each repository's runs copied:

| Repository | Build |
|---|---|
| django | 1230.5 s |
| express | 4.7 s |
| flask | 29.9 s |
| serde | 56.0 s |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | B / A |
|---|---|
| Cost | — |
| Input tokens (incl. cache) | 1.43 (0.61–1.77, 4 tasks) |
| Output tokens | 1.21 (0.62–1.87, 4 tasks) |
| Turns | — |
| Wall time | 1.25 (0.65–1.52, 4 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | B | B/A |
|---|---|---|---|
| `django-callers-iri-to-uri` | 54,117 | 32,959 | 0.61 |
| `express-locate-trust` | 31,474 | 36,621 | 1.16 |
| `flask-edit-method-route` | 54,921 | 93,048 | 1.69 |
| `serde-types-deserializer` | 31,732 | 56,196 | 1.77 |

## Success by task category

| Category | A | B |
|---|---|---|
| callers | 3/3 | 3/3 |
| edit | 3/3 | 3/3 |
| locate | 3/3 | 3/3 |
| types | 3/3 | 3/3 |

## By task

| Task | A | B | Cost A | Cost B |
|---|---|---|---|---|
| `django-callers-iri-to-uri` | 3/3 | 3/3 | — | — |
| `express-locate-trust` | 3/3 | 3/3 | — | — |
| `flask-edit-method-route` | 3/3 | 3/3 | — | — |
| `serde-types-deserializer` | 3/3 | 3/3 | — | — |

## Failed runs

None.
