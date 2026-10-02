# L3 agent benchmark

Scope: pilot design; complete (9/9 runs).
Product profile: `context-first-pilot`. Arms: A=built-ins; B=default context+index_project with context note; C=legacy full surface with legacy navigation note.

Agent `codex` (codex-cli 0.160.0, auth `subscription`), model `gpt-6.1-sol` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.4.0 at `4185640`.

Cost is an API-price estimate (the agent reports none): `gpt-6.1-sol` at $2.00 input, $0.10 cached input and $10.00 output per 1M tokens, standard tier, from https://developers.openai.com/api/docs/pricing (2026-10-02). On a subscription it shows what the same tokens would cost through the API; tokens are the quota measure.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median pre-index | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | $0.041 | $0.11 | 48,302 | 713 | — | 33 s | — | 0/3 | 0/1 | 0/3 |
| B | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | $0.052 | $0.16 | 65,583 | 580 | — | 32 s | 26.9 s | 3/3 | 0/1 | 0/3 |
| C | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | $0.043 | $0.16 | 54,792 | 475 | — | 25 s | 26.8 s | 3/3 | 0/1 | 0/3 |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | B / A | C / A |
|---|---|---|
| Cost | 1.27 (1.03–2.36, 3 tasks) | 1.05 (0.64–3.42, 3 tasks) |
| Input tokens (incl. cache) | 1.29 (0.96–1.82, 3 tasks) | 1.21 (0.79–2.19, 3 tasks) |
| Output tokens | 0.94 (0.81–1.36, 3 tasks) | 1.19 (0.40–1.26, 3 tasks) |
| Turns | — | — |
| Wall time | 1.00 (0.95–1.25, 3 tasks) | 1.09 (0.49–1.30, 3 tasks) |

## Success by task category

| Category | A | B | C |
|---|---|---|---|
| callers | 1/1 | 1/1 | 1/1 |
| edit | 1/1 | 1/1 | 1/1 |
| trap | 1/1 | 1/1 | 1/1 |

## By task

| Task | A | B | C | Cost A | Cost B | Cost C |
|---|---|---|---|---|---|---|
| `express-trap-req-get` | 1/1 | 1/1 | 1/1 | $0.044 | $0.045 | $0.028 |
| `flask-callers-ensure-sync` | 1/1 | 1/1 | 1/1 | $0.041 | $0.052 | $0.043 |
| `serde-edit-with-bound` | 1/1 | 1/1 | 1/1 | $0.026 | $0.061 | $0.089 |

## Failed runs

None.
