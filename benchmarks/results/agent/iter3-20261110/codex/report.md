# L3 agent benchmark

Scope: pilot design; complete (16/16 runs).
Product profile: `context-first-pilot`. Arms: A=built-ins; B=default context+index_project with context note; C=legacy full surface with legacy navigation note; F=B plus an instruction to call `context` first (forced use, not natural use).

Agent `codex` (codex-cli 0.167.0, auth `subscription`), model `gpt-6-luna` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `b398928`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 7/8 (88%) | 53%–98% | 0.88 | 0.88 | — | — (8 unknown) | 56,023 | 780 | — | 22 s | — | 0/8 | 0/2 | 0/8 |
| B | 7/8 (88%) | 53%–98% | 0.97 | 1.00 | — | — (8 unknown) | 40,568 | 711 | — | 21 s | 0.0 s | 8/8 | 0/2 | 0/8 |

## Prebuilt indexes

Arms with CCM copy an index built once per repository before their runs (the cache is shared across result directories); the arm table shows the copy time. One-time build time of the index each repository's runs copied:

| Repository | Build |
|---|---|
| django | 2304.7 s |
| express | 9.1 s |
| flask | 59.2 s |
| serde | 93.1 s |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | B / A |
|---|---|
| Cost | — |
| Input tokens (incl. cache) | 1.07 (0.53–1.42, 8 tasks) |
| Output tokens | 0.75 (0.51–1.41, 8 tasks) |
| Turns | — |
| Wall time | 0.86 (0.56–1.38, 8 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | B | B/A |
|---|---|---|---|
| `django-callers-get-random-string` | 61,342 | 32,519 | 0.53 |
| `django-callers-make-aware` | 67,919 | 34,497 | 0.51 |
| `express-trap-res-send` | 32,843 | 46,640 | 1.42 |
| `express-trap-sendfile-helper` | 31,832 | 55,169 | 1.73 |
| `flask-edit-debug-flag` | 69,772 | 78,977 | 1.13 |
| `flask-edit-error-handler` | 61,504 | 84,779 | 1.38 |
| `serde-trap-deserialize-struct` | 50,704 | 32,439 | 0.64 |
| `serde-trap-effective-style` | 31,810 | 32,128 | 1.01 |

## Success by task category

| Category | A | B |
|---|---|---|
| callers | 2/2 | 2/2 |
| edit | 2/2 | 2/2 |
| trap | 3/4 | 3/4 |

## By task

| Task | A | B | Cost A | Cost B |
|---|---|---|---|---|
| `django-callers-get-random-string` | 1/1 | 1/1 | — | — |
| `django-callers-make-aware` | 1/1 | 1/1 | — | — |
| `express-trap-res-send` | 1/1 | 0/1 | — | — |
| `express-trap-sendfile-helper` | 0/1 | 1/1 | — | — |
| `flask-edit-debug-flag` | 1/1 | 1/1 | — | — |
| `flask-edit-error-handler` | 1/1 | 1/1 | — | — |
| `serde-trap-deserialize-struct` | 1/1 | 1/1 | — | — |
| `serde-trap-effective-style` | 1/1 | 1/1 | — | — |

## Failed runs

- `express-trap-res-send` B#1: missing lib/response.js:render
- `express-trap-sendfile-helper` A#1: missing lib/response.js:sendFile, lib/response.js:sendfile
