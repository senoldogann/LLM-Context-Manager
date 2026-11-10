# L3 agent benchmark

Scope: pilot design; complete (24/24 runs).
Product profile: `context-first-pilot`. Arms: A=built-ins; B=default context+index_project with context note; C=legacy full surface with legacy navigation note; F=B plus an instruction to call `context` first (forced use, not natural use).

Agent `claude` (2.1.319 (Claude Code), auth `subscription`), model `claude-opus-5-5` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `b398928`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 7/8 (88%) | 53%–98% | 0.99 | 1.00 | $0.073 | $0.56 | 35,012 | 1,264 | 4 | 16 s | — | 0/8 | 0/2 | 0/8 |
| B | 7/8 (88%) | 53%–98% | 0.99 | 1.00 | $0.074 | $0.62 | 36,119 | 1,162 | 4 | 14 s | 0.0 s | 0/8 | 0/2 | 0/8 |
| F | 8/8 (100%) | 68%–100% | 1.00 | 1.00 | $0.074 | $0.60 | 37,762 | 1,087 | 5 | 14 s | 0.0 s | 8/8 | 0/2 | 0/8 |

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

| Metric | B / A | F / A |
|---|---|---|
| Cost | 1.07 (1.02–1.26, 8 tasks) | 1.11 (0.81–1.34, 8 tasks) |
| Input tokens (incl. cache) | 1.10 (1.04–1.35, 8 tasks) | 1.17 (0.54–1.53, 8 tasks) |
| Output tokens | 1.03 (0.93–1.10, 8 tasks) | 1.01 (0.77–1.22, 8 tasks) |
| Turns | 1.00 (1.00–1.20, 8 tasks) | 1.17 (0.50–1.67, 8 tasks) |
| Wall time | 1.02 (0.87–1.07, 8 tasks) | 0.96 (0.76–1.25, 8 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | B | F | B/A | F/A |
|---|---|---|---|---|---|
| `django-callers-get-random-string` | 35,386 | 36,923 | 17,434 | 1.04 | 0.49 |
| `django-callers-make-aware` | 34,756 | 51,558 | 18,785 | 1.48 | 0.54 |
| `express-trap-res-send` | 47,396 | 61,282 | 40,987 | 1.29 | 0.86 |
| `express-trap-sendfile-helper` | 24,578 | 26,113 | 37,449 | 1.06 | 1.52 |
| `flask-edit-debug-flag` | 35,269 | 47,585 | 54,796 | 1.35 | 1.55 |
| `flask-edit-error-handler` | 59,420 | 35,315 | 38,075 | 0.59 | 0.64 |
| `serde-trap-deserialize-struct` | 25,271 | 28,200 | 38,696 | 1.12 | 1.53 |
| `serde-trap-effective-style` | 23,737 | 25,637 | 35,141 | 1.08 | 1.48 |

## Success by task category

| Category | A | B | F |
|---|---|---|---|
| callers | 1/2 | 1/2 | 2/2 |
| edit | 2/2 | 2/2 | 2/2 |
| trap | 4/4 | 4/4 | 4/4 |

## By task

| Task | A | B | F | Cost A | Cost B | Cost F |
|---|---|---|---|---|---|---|
| `django-callers-get-random-string` | 0/1 | 0/1 | 1/1 | $0.071 | $0.072 | $0.052 |
| `django-callers-make-aware` | 1/1 | 1/1 | 1/1 | $0.075 | $0.112 | $0.074 |
| `express-trap-res-send` | 1/1 | 1/1 | 1/1 | $0.082 | $0.103 | $0.082 |
| `express-trap-sendfile-helper` | 1/1 | 1/1 | 1/1 | $0.053 | $0.055 | $0.071 |
| `flask-edit-debug-flag` | 1/1 | 1/1 | 1/1 | $0.080 | $0.086 | $0.107 |
| `flask-edit-error-handler` | 1/1 | 1/1 | 1/1 | $0.090 | $0.063 | $0.073 |
| `serde-trap-deserialize-struct` | 1/1 | 1/1 | 1/1 | $0.062 | $0.075 | $0.078 |
| `serde-trap-effective-style` | 1/1 | 1/1 | 1/1 | $0.047 | $0.050 | $0.058 |

## Failed runs

- `django-callers-get-random-string` A#1: missing django/contrib/sessions/backends/base.py:_aget_new_session_key
- `django-callers-get-random-string` B#1: missing django/contrib/sessions/backends/base.py:_aget_new_session_key
