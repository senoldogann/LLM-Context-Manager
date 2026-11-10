# L3 agent benchmark

Scope: pilot design; complete (16/16 runs).
Product profile: `context-first-pilot`. Arms: A=built-ins; B=default context+index_project with context note; C=legacy full surface with legacy navigation note; F=B plus an instruction to call `context` first (forced use, not natural use).

Agent `codex` (codex-cli 0.167.0, auth `subscription`), model `gpt-6-luna` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `031d50a`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 7/8 (88%) | 53%–98% | 0.88 | 0.88 | — | — (8 unknown) | 53,416 | 720 | — | 21 s | — | 0/8 | 0/2 | 0/8 |
| B | 7/8 (88%) | 53%–98% | 0.88 | 0.88 | — | — (8 unknown) | 46,531 | 775 | — | 22 s | 0.0 s | 7/8 | 0/2 | 0/8 |

## Prebuilt indexes

Arms with CCM copy an index built once per repository before their runs (the cache is shared across result directories); the arm table shows the copy time. One-time build time of the index each repository's runs copied:

| Repository | Build |
|---|---|
| django | 1348.7 s |
| express | 23.1 s |
| flask | 187.8 s |
| serde | 290.5 s |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | B / A |
|---|---|
| Cost | — |
| Input tokens (incl. cache) | 0.85 (0.55–1.40, 8 tasks) |
| Output tokens | 0.93 (0.56–1.19, 8 tasks) |
| Turns | — |
| Wall time | 0.92 (0.62–1.23, 8 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | B | B/A |
|---|---|---|---|
| `django-callers-make-aware` | 43,335 | 34,603 | 0.80 |
| `django-trap-html-escape` | 259,771 | 78,475 | 0.30 |
| `express-trap-req-get` | 58,757 | 32,366 | 0.55 |
| `express-trap-sendfile-helper` | 31,646 | 44,218 | 1.40 |
| `flask-callers-ensure-sync` | 50,077 | 43,401 | 0.87 |
| `flask-edit-error-handler` | 56,756 | 84,841 | 1.49 |
| `serde-edit-with-bound` | 67,870 | 56,543 | 0.83 |
| `serde-trap-deserialize-struct` | 41,972 | 48,844 | 1.16 |

## Success by task category

| Category | A | B |
|---|---|---|
| callers | 2/2 | 2/2 |
| edit | 2/2 | 2/2 |
| trap | 3/4 | 3/4 |

## By task

| Task | A | B | Cost A | Cost B |
|---|---|---|---|---|
| `django-callers-make-aware` | 1/1 | 1/1 | — | — |
| `django-trap-html-escape` | 1/1 | 1/1 | — | — |
| `express-trap-req-get` | 1/1 | 1/1 | — | — |
| `express-trap-sendfile-helper` | 0/1 | 0/1 | — | — |
| `flask-callers-ensure-sync` | 1/1 | 1/1 | — | — |
| `flask-edit-error-handler` | 1/1 | 1/1 | — | — |
| `serde-edit-with-bound` | 1/1 | 1/1 | — | — |
| `serde-trap-deserialize-struct` | 1/1 | 1/1 | — | — |

## Failed runs

- `express-trap-sendfile-helper` A#1: missing lib/response.js:sendFile, lib/response.js:sendfile
- `express-trap-sendfile-helper` B#1: missing lib/response.js:sendFile, lib/response.js:sendfile
