# L3 agent benchmark

Scope: pilot design; complete (9/9 runs).
Product profile: `context-first-pilot`. Arms: A=built-ins; B=default context+index_project with context note; C=legacy full surface with legacy navigation note.

Agent `codex` (codex-cli 0.160.0, auth `subscription`), model `gpt-6.1-sol` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.4.0 at `f649a40`.

Cost is an API-price estimate (the agent reports none): `gpt-6.1-sol` at $2.00 input, $0.10 cached input and $10.00 output per 1M tokens, standard tier, from https://developers.openai.com/api/docs/pricing (2026-10-02). On a subscription it shows what the same tokens would cost through the API; tokens are the quota measure.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median pre-index | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | $0.036 | $0.11 | 47,797 | 522 | — | 28 s | — | 0/3 | 0/1 | 0/3 |
| B | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | $0.042 | $0.12 | 63,748 | 567 | — | 29 s | 26.9 s | 3/3 | 0/1 | 0/3 |
| C | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | $0.047 | $0.15 | 67,790 | 620 | — | 30 s | 26.9 s | 3/3 | 0/1 | 0/3 |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | B / A | C / A |
|---|---|---|
| Cost | 1.00 (0.93–1.16, 3 tasks) | 1.31 (0.98–1.70, 3 tasks) |
| Input tokens (incl. cache) | 1.31 (1.07–1.36, 3 tasks) | 1.31 (1.11–2.01, 3 tasks) |
| Output tokens | 1.09 (0.69–1.17, 3 tasks) | 1.59 (0.60–2.38, 3 tasks) |
| Turns | — | — |
| Wall time | 1.05 (0.80–1.08, 3 tasks) | 1.45 (0.72–1.89, 3 tasks) |

## Success by task category

| Category | A | B | C |
|---|---|---|---|
| callers | 1/1 | 1/1 | 1/1 |
| edit | 1/1 | 1/1 | 1/1 |
| trap | 1/1 | 1/1 | 1/1 |

## By task

| Task | A | B | C | Cost A | Cost B | Cost C |
|---|---|---|---|---|---|---|
| `express-trap-req-get` | 1/1 | 1/1 | 1/1 | $0.036 | $0.033 | $0.047 |
| `flask-callers-ensure-sync` | 1/1 | 1/1 | 1/1 | $0.042 | $0.043 | $0.042 |
| `serde-edit-with-bound` | 1/1 | 1/1 | 1/1 | $0.036 | $0.042 | $0.062 |

## Failed runs

None.
