# L3 agent benchmark

Scope: pilot design; complete (9/9 runs).

Agent `claude` (2.1.287 (Claude Code), auth `subscription`), model `claude-opus-5-5` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.4.0 at `e4b9cec`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median pre-index | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | $0.074 | $0.23 | 34,790 | 1,110 | 4 | 16 s | — | 0/3 | 0/1 | 0/3 |
| B | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | $0.111 | $0.31 | 43,953 | 1,153 | 4 | 15 s | 73.8 s | 0/3 | 0/1 | 0/3 |
| C | 3/3 (100%) | 44%–100% | 1.00 | 0.94 | $0.106 | $0.31 | 48,042 | 1,798 | 5 | 20 s | 55.2 s | 0/3 | 0/1 | 0/3 |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | B / A | C / A |
|---|---|---|
| Cost | 1.33 (1.02–1.78, 3 tasks) | 1.27 (0.94–1.87, 3 tasks) |
| Input tokens (incl. cache) | 1.25 (1.24–2.04, 3 tasks) | 1.67 (0.74–1.88, 3 tasks) |
| Output tokens | 1.04 (1.03–1.37, 3 tasks) | 1.27 (1.01–1.62, 3 tasks) |
| Turns | 1.00 (1.00–1.43, 3 tasks) | 1.14 (1.00–1.67, 3 tasks) |
| Wall time | 1.00 (0.95–1.20, 3 tasks) | 1.34 (0.90–1.49, 3 tasks) |

## Success by task category

| Category | A | B | C |
|---|---|---|---|
| callers | 1/1 | 1/1 | 1/1 |
| edit | 1/1 | 1/1 | 1/1 |
| trap | 1/1 | 1/1 | 1/1 |

## By task

| Task | A | B | C | Cost A | Cost B | Cost C |
|---|---|---|---|---|---|---|
| `express-trap-req-get` | 1/1 | 1/1 | 1/1 | $0.074 | $0.075 | $0.069 |
| `flask-callers-ensure-sync` | 1/1 | 1/1 | 1/1 | $0.071 | $0.127 | $0.133 |
| `serde-edit-with-bound` | 1/1 | 1/1 | 1/1 | $0.084 | $0.111 | $0.106 |

## Failed runs

None.
