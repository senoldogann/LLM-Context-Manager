# L3 agent benchmark

Scope: pilot design; partial (12/16 runs).
Product profile: `context-first-pilot`. Arms: A=built-ins; B=default context+index_project with context note; C=legacy full surface with legacy navigation note; F=B plus an instruction to call `context` first (forced use, not natural use).

Agent `claude` (2.1.319 (Claude Code), auth `subscription`), model `claude-opus-5-5` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `b6d055d`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median pre-index (full index per run) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | $0.083 | $0.25 | 37,771 | 1,440 | 4 | 19 s | — | 0/3 | 0/1 | 0/3 |
| B | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | $0.111 | $0.32 | 39,627 | 1,316 | 5 | 19 s | 218.3 s | 0/3 | 0/1 | 0/3 |
| C | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | $0.090 | $0.29 | 45,907 | 1,498 | 4 | 19 s | 240.3 s | 0/3 | 0/1 | 0/3 |
| F | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | $0.088 | $0.27 | 40,059 | 1,250 | 5 | 20 s | 201.7 s | 3/3 | 0/1 | 0/3 |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | B / A | C / A | F / A |
|---|---|---|---|
| Cost | 1.42 (0.94–1.47, 3 tasks) | 1.03 (0.79–1.67, 3 tasks) | 1.13 (0.88–1.24, 3 tasks) |
| Input tokens (incl. cache) | 1.05 (0.80–1.69, 3 tasks) | 1.20 (1.17–1.22, 3 tasks) | 1.42 (0.50–1.45, 3 tasks) |
| Output tokens | 0.94 (0.87–1.13, 3 tasks) | 1.02 (0.80–1.04, 3 tasks) | 1.07 (0.82–1.17, 3 tasks) |
| Turns | 1.00 (0.88–1.25, 3 tasks) | 1.00 (0.75–1.00, 3 tasks) | 1.12 (0.75–1.25, 3 tasks) |
| Wall time | 1.00 (0.93–1.00, 3 tasks) | 0.96 (0.65–1.23, 3 tasks) | 1.09 (0.73–1.16, 3 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | B | C | F | B/A | C/A | F/A |
|---|---|---|---|---|---|---|---|
| `express-trap-req-get` | 27,624 | 46,577 | 32,351 | 40,059 | 1.69 | 1.17 | 1.45 |
| `flask-callers-ensure-sync` | 37,771 | 39,627 | 45,907 | 19,046 | 1.05 | 1.22 | 0.50 |
| `serde-edit-with-bound` | 47,187 | 37,792 | 56,818 | 67,132 | 0.80 | 1.20 | 1.42 |

## Success by task category

| Category | A | B | C | F |
|---|---|---|---|---|
| callers | 1/1 | 1/1 | 1/1 | 1/1 |
| edit | 1/1 | 1/1 | 1/1 | 1/1 |
| trap | 1/1 | 1/1 | 1/1 | 1/1 |

## By task

| Task | A | B | C | F | Cost A | Cost B | Cost C | Cost F |
|---|---|---|---|---|---|---|---|---|
| `express-trap-req-get` | 1/1 | 1/1 | 1/1 | 1/1 | $0.078 | $0.111 | $0.062 | $0.088 |
| `flask-callers-ensure-sync` | 1/1 | 1/1 | 1/1 | 1/1 | $0.083 | $0.123 | $0.140 | $0.073 |
| `serde-edit-with-bound` | 1/1 | 1/1 | 1/1 | 1/1 | $0.087 | $0.082 | $0.090 | $0.108 |

## Failed runs

None.
