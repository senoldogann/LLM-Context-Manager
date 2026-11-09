# L3 agent benchmark

Scope: pilot design; complete (9/9 runs).
Product profile: `context-first-pilot`. Arms: A=built-ins; B=default context+index_project with context note; C=legacy full surface with legacy navigation note.

Agent `claude` (2.1.319 (Claude Code), auth `subscription`), model `claude-opus-5-5` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `be48997`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median pre-index | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | $0.090 | $0.27 | 49,564 | 1,404 | 5 | 18 s | — | 0/3 | 0/1 | 0/3 |
| B | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | $0.089 | $0.29 | 38,201 | 1,628 | 5 | 18 s | 58.4 s | 0/3 | 0/1 | 0/3 |
| C | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | $0.093 | $0.31 | 34,900 | 1,360 | 4 | 17 s | 83.1 s | 0/3 | 0/1 | 0/3 |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | B / A | C / A |
|---|---|---|
| Cost | 0.91 (0.89–1.53, 3 tasks) | 1.01 (0.95–1.51, 3 tasks) |
| Input tokens (incl. cache) | 0.96 (0.77–1.02, 3 tasks) | 0.86 (0.69–1.16, 3 tasks) |
| Output tokens | 1.05 (1.02–1.16, 3 tasks) | 1.01 (0.76–1.16, 3 tasks) |
| Turns | 1.00 (0.88–1.00, 3 tasks) | 0.80 (0.75–1.12, 3 tasks) |
| Wall time | 1.00 (0.97–1.05, 3 tasks) | 0.96 (0.61–1.00, 3 tasks) |

## Success by task category

| Category | A | B | C |
|---|---|---|---|
| callers | 1/1 | 1/1 | 1/1 |
| edit | 1/1 | 1/1 | 1/1 |
| trap | 1/1 | 1/1 | 1/1 |

## By task

| Task | A | B | C | Cost A | Cost B | Cost C |
|---|---|---|---|---|---|---|
| `express-trap-req-get` | 1/1 | 1/1 | 1/1 | $0.090 | $0.080 | $0.091 |
| `flask-callers-ensure-sync` | 1/1 | 1/1 | 1/1 | $0.081 | $0.125 | $0.123 |
| `serde-edit-with-bound` | 1/1 | 1/1 | 1/1 | $0.098 | $0.089 | $0.093 |

## Failed runs

None.
