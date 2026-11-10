# L3 agent benchmark

Scope: pilot design; complete (24/24 runs).
Product profile: `context-first-pilot`. Arms: A=built-ins; B=default context+index_project with context note; C=legacy full surface with legacy navigation note; F=B plus an instruction to call `context` first (forced use, not natural use).

Agent `claude` (2.1.319 (Claude Code), auth `subscription`), model `claude-opus-5-5` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `031d50a`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 8/8 (100%) | 68%–100% | 1.00 | 1.00 | $0.072 | $0.64 | 30,498 | 1,179 | 4 | 15 s | — | 0/8 | 0/2 | 0/8 |
| B | 8/8 (100%) | 68%–100% | 1.00 | 1.00 | $0.089 | $0.75 | 38,924 | 1,238 | 4 | 15 s | 0.0 s | 0/8 | 0/2 | 0/8 |
| F | 8/8 (100%) | 68%–100% | 1.00 | 1.00 | $0.079 | $0.71 | 42,014 | 1,272 | 4 | 15 s | 0.0 s | 8/8 | 0/2 | 0/8 |

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

| Metric | B / A | F / A |
|---|---|---|
| Cost | 1.13 (1.08–1.54, 8 tasks) | 1.14 (0.95–1.23, 8 tasks) |
| Input tokens (incl. cache) | 1.08 (1.05–1.28, 8 tasks) | 1.00 (0.74–1.42, 8 tasks) |
| Output tokens | 1.19 (0.87–1.50, 8 tasks) | 1.11 (0.91–1.44, 8 tasks) |
| Turns | 1.00 (1.00–1.43, 8 tasks) | 1.07 (1.00–1.25, 8 tasks) |
| Wall time | 1.11 (0.87–1.46, 8 tasks) | 1.08 (0.81–1.32, 8 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | B | F | B/A | F/A |
|---|---|---|---|---|---|
| `django-callers-make-aware` | 25,549 | 61,840 | 19,742 | 2.42 | 0.77 |
| `django-trap-html-escape` | 76,609 | 46,803 | 78,884 | 0.61 | 1.03 |
| `express-trap-req-get` | 25,854 | 27,861 | 37,892 | 1.08 | 1.47 |
| `express-trap-sendfile-helper` | 25,169 | 27,103 | 18,560 | 1.08 | 0.74 |
| `flask-callers-ensure-sync` | 26,674 | 28,391 | 19,138 | 1.06 | 0.72 |
| `flask-edit-error-handler` | 34,321 | 38,602 | 48,880 | 1.12 | 1.42 |
| `serde-edit-with-bound` | 57,871 | 74,138 | 55,727 | 1.28 | 0.96 |
| `serde-trap-deserialize-struct` | 37,316 | 39,246 | 46,135 | 1.05 | 1.24 |

## Success by task category

| Category | A | B | F |
|---|---|---|---|
| callers | 2/2 | 2/2 | 2/2 |
| edit | 2/2 | 2/2 | 2/2 |
| trap | 4/4 | 4/4 | 4/4 |

## By task

| Task | A | B | F | Cost A | Cost B | Cost F |
|---|---|---|---|---|---|---|
| `django-callers-make-aware` | 1/1 | 1/1 | 1/1 | $0.071 | $0.125 | $0.087 |
| `django-trap-html-escape` | 1/1 | 1/1 | 1/1 | $0.129 | $0.096 | $0.157 |
| `express-trap-req-get` | 1/1 | 1/1 | 1/1 | $0.060 | $0.068 | $0.070 |
| `express-trap-sendfile-helper` | 1/1 | 1/1 | 1/1 | $0.056 | $0.064 | $0.063 |
| `flask-callers-ensure-sync` | 1/1 | 1/1 | 1/1 | $0.076 | $0.116 | $0.069 |
| `flask-edit-error-handler` | 1/1 | 1/1 | 1/1 | $0.064 | $0.082 | $0.082 |
| `serde-edit-with-bound` | 1/1 | 1/1 | 1/1 | $0.108 | $0.117 | $0.103 |
| `serde-trap-deserialize-struct` | 1/1 | 1/1 | 1/1 | $0.073 | $0.079 | $0.075 |

## Failed runs

None.
