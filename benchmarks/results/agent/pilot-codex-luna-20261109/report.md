# L3 agent benchmark

Scope: pilot design; complete (9/9 runs).
Product profile: `context-first-pilot`. Arms: A=built-ins; B=default context+index_project with context note; C=legacy full surface with legacy navigation note.

Agent `codex` (codex-cli 0.167.0, auth `subscription`), model `gpt-6-luna` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `be48997`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median pre-index | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | — | — (3 unknown) | 56,716 | 890 | — | 27 s | — | 0/3 | 0/1 | 0/3 |
| B | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | — | — (3 unknown) | 57,003 | 1,030 | — | 28 s | 27.9 s | 3/3 | 0/1 | 0/3 |
| C | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | — | — (3 unknown) | 117,646 | 1,078 | — | 33 s | 28.5 s | 3/3 | 0/1 | 0/3 |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | B / A | C / A |
|---|---|---|
| Cost | — | — |
| Input tokens (incl. cache) | 1.11 (0.98–1.14, 3 tasks) | 2.07 (1.66–2.59, 3 tasks) |
| Output tokens | 1.06 (0.82–1.27, 3 tasks) | 1.32 (1.14–1.97, 3 tasks) |
| Turns | — | — |
| Wall time | 1.04 (0.88–1.10, 3 tasks) | 1.35 (1.21–1.85, 3 tasks) |

## Success by task category

| Category | A | B | C |
|---|---|---|---|
| callers | 1/1 | 1/1 | 1/1 |
| edit | 1/1 | 1/1 | 1/1 |
| trap | 1/1 | 1/1 | 1/1 |

## By task

| Task | A | B | C | Cost A | Cost B | Cost C |
|---|---|---|---|---|---|---|
| `express-trap-req-get` | 1/1 | 1/1 | 1/1 | — | — | — |
| `flask-callers-ensure-sync` | 1/1 | 1/1 | 1/1 | — | — | — |
| `serde-edit-with-bound` | 1/1 | 1/1 | 1/1 | — | — | — |

## Failed runs

None.
