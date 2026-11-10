# L3 agent benchmark

Scope: pilot design; complete (12/12 runs).
Product profile: `context-first-pilot`. Arms: A=built-ins; B=default context+index_project with context note; C=legacy full surface with legacy navigation note; F=B plus an instruction to call `context` first (forced use, not natural use).

Agent `claude` (2.1.319 (Claude Code), auth `subscription`), model `claude-opus-5-5` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `b6d055d`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median pre-index (full index per run) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | $0.072 | $0.21 | 26,384 | 1,047 | 3 | 16 s | — | 0/3 | 0/0 | 0/3 |
| B | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | $0.071 | $0.21 | 27,592 | 1,077 | 3 | 17 s | 220.3 s | 0/3 | 0/0 | 0/3 |
| C | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | $0.072 | $0.21 | 32,754 | 1,004 | 4 | 15 s | 255.3 s | 0/3 | 0/0 | 0/3 |
| F | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | $0.057 | $0.20 | 27,357 | 709 | 3 | 11 s | 159.7 s | 3/3 | 0/0 | 0/3 |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | B / A | C / A | F / A |
|---|---|---|---|
| Cost | 0.99 (0.98–1.03, 3 tasks) | 1.01 (0.86–1.04, 3 tasks) | 0.78 (0.73–1.27, 3 tasks) |
| Input tokens (incl. cache) | 1.04 (1.03–1.05, 3 tasks) | 1.24 (0.94–1.25, 3 tasks) | 0.67 (0.60–1.53, 3 tasks) |
| Output tokens | 1.03 (0.88–1.10, 3 tasks) | 0.97 (0.85–1.01, 3 tasks) | 0.68 (0.63–1.09, 3 tasks) |
| Turns | 1.00 (1.00–1.00, 3 tasks) | 1.00 (0.80–1.33, 3 tasks) | 0.67 (0.60–1.33, 3 tasks) |
| Wall time | 0.86 (0.85–1.12, 3 tasks) | 0.87 (0.75–1.04, 3 tasks) | 0.80 (0.52–1.38, 3 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | B | C | F | B/A | C/A | F/A |
|---|---|---|---|---|---|---|---|
| `express-callers-res-set` | 26,208 | 27,266 | 32,662 | 40,182 | 1.04 | 1.25 | 1.53 |
| `flask-trap-load-dotenv` | 45,384 | 46,765 | 42,879 | 27,357 | 1.03 | 0.94 | 0.60 |
| `serde-callers-expr-is-missing` | 26,384 | 27,592 | 32,754 | 17,723 | 1.05 | 1.24 | 0.67 |

## Success by task category

| Category | A | B | C | F |
|---|---|---|---|---|
| callers | 2/2 | 2/2 | 2/2 | 2/2 |
| trap | 1/1 | 1/1 | 1/1 | 1/1 |

## By task

| Task | A | B | C | F | Cost A | Cost B | Cost C | Cost F |
|---|---|---|---|---|---|---|---|---|
| `express-callers-res-set` | 1/1 | 1/1 | 1/1 | 1/1 | $0.069 | $0.071 | $0.072 | $0.088 |
| `flask-trap-load-dotenv` | 1/1 | 1/1 | 1/1 | 1/1 | $0.073 | $0.072 | $0.063 | $0.057 |
| `serde-callers-expr-is-missing` | 1/1 | 1/1 | 1/1 | 1/1 | $0.072 | $0.070 | $0.073 | $0.052 |

## Failed runs

None.
