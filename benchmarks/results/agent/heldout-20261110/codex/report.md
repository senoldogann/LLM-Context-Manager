# L3 agent benchmark

Scope: pilot design; complete (9/9 runs).
Product profile: `context-first-pilot`. Arms: A=built-ins; B=default context+index_project with context note; C=legacy full surface with legacy navigation note; F=B plus an instruction to call `context` first (forced use, not natural use).

Agent `codex` (codex-cli 0.167.0, auth `subscription`), model `gpt-6-luna` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `b6d055d`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median pre-index | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | — | — (3 unknown) | 45,301 | 562 | — | 24 s | — | 0/3 | 0/0 | 0/3 |
| B | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | — | — (3 unknown) | 32,554 | 522 | — | 19 s | 216.0 s | 3/3 | 0/0 | 0/3 |
| C | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | — | — (3 unknown) | 105,477 | 1,142 | — | 32 s | 264.3 s | 3/3 | 0/0 | 0/3 |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | B / A | C / A |
|---|---|---|
| Cost | — | — |
| Input tokens (incl. cache) | 0.89 (0.71–1.01, 3 tasks) | 2.47 (1.42–2.76, 3 tasks) |
| Output tokens | 0.93 (0.80–1.04, 3 tasks) | 1.65 (1.42–2.17, 3 tasks) |
| Turns | — | — |
| Wall time | 0.94 (0.78–1.51, 3 tasks) | 1.25 (1.21–2.05, 3 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | B | C | B/A | C/A |
|---|---|---|---|---|---|
| `express-callers-res-set` | 74,489 | 74,964 | 105,477 | 1.01 | 1.42 |
| `flask-trap-load-dotenv` | 45,301 | 32,303 | 111,952 | 0.71 | 2.47 |
| `serde-callers-expr-is-missing` | 36,638 | 32,554 | 101,219 | 0.89 | 2.76 |

## Success by task category

| Category | A | B | C |
|---|---|---|---|
| callers | 2/2 | 2/2 | 2/2 |
| trap | 1/1 | 1/1 | 1/1 |

## By task

| Task | A | B | C | Cost A | Cost B | Cost C |
|---|---|---|---|---|---|---|
| `express-callers-res-set` | 1/1 | 1/1 | 1/1 | — | — | — |
| `flask-trap-load-dotenv` | 1/1 | 1/1 | 1/1 | — | — | — |
| `serde-callers-expr-is-missing` | 1/1 | 1/1 | 1/1 | — | — | — |

## Failed runs

None.
