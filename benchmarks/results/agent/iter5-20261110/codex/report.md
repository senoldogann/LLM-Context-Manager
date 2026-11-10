# L3 agent benchmark

Scope: pilot design; complete (16/16 runs).
Product profile: `context-first-pilot`. Arms: A=built-ins; B=default context+index_project with context note; C=legacy full surface with legacy navigation note; F=B plus an instruction to call `context` first (forced use, not natural use); H=B plus the opt-in Claude Code Grep hook (installed product configuration, not natural use).

Agent `codex` (codex-cli 0.167.0, auth `subscription`), model `gpt-6-luna` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `9bb6564`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 8/8 (100%) | 68%–100% | 1.00 | 1.00 | — | — (8 unknown) | 52,899 | 910 | — | 25 s | — | 0/8 | 0/3 | 0/8 |
| B | 7/8 (88%) | 53%–98% | 0.94 | 1.00 | — | — (8 unknown) | 53,788 | 734 | — | 24 s | 0.0 s | 6/8 | 0/3 | 0/8 |

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
| Input tokens (incl. cache) | 0.95 (0.61–1.06, 8 tasks) |
| Output tokens | 0.94 (0.86–1.22, 8 tasks) |
| Turns | — |
| Wall time | 0.98 (0.71–1.23, 8 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | B | B/A |
|---|---|---|---|
| `django-callers-iri-to-uri` | 66,584 | 46,444 | 0.70 |
| `django-trap-capfirst` | 168,390 | 76,004 | 0.45 |
| `express-edit-normalize-type` | 61,610 | 61,132 | 0.99 |
| `express-locate-trust` | 31,422 | 32,835 | 1.04 |
| `flask-edit-method-route` | 44,188 | 93,343 | 2.11 |
| `flask-locate-app-string` | 35,911 | 32,613 | 0.91 |
| `serde-edit-wrap-field` | 115,897 | 70,134 | 0.61 |
| `serde-types-deserializer` | 30,840 | 32,833 | 1.06 |

## Success by task category

| Category | A | B |
|---|---|---|
| callers | 1/1 | 1/1 |
| edit | 3/3 | 2/3 |
| locate | 2/2 | 2/2 |
| trap | 1/1 | 1/1 |
| types | 1/1 | 1/1 |

## By task

| Task | A | B | Cost A | Cost B |
|---|---|---|---|---|
| `django-callers-iri-to-uri` | 1/1 | 1/1 | — | — |
| `django-trap-capfirst` | 1/1 | 1/1 | — | — |
| `express-edit-normalize-type` | 1/1 | 0/1 | — | — |
| `express-locate-trust` | 1/1 | 1/1 | — | — |
| `flask-edit-method-route` | 1/1 | 1/1 | — | — |
| `flask-locate-app-string` | 1/1 | 1/1 | — | — |
| `serde-edit-wrap-field` | 1/1 | 1/1 | — | — |
| `serde-types-deserializer` | 1/1 | 1/1 | — | — |

## Failed runs

- `express-edit-normalize-type` B#1: missing lib/utils.js:normalizeTypes
