# L3 agent benchmark

Scope: pilot design; partial (10/12 runs).
Product profile: `context-first-pilot`. Arms: A=built-ins; B=default context+index_project with context note; C=legacy full surface with legacy navigation note; F=B plus an instruction to call `context` first (forced use, not natural use).

Agent `codex` (codex-cli 0.167.0, auth `subscription`), model `gpt-6-luna` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `b6d055d`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median pre-index (full index per run) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 4/4 (100%) | 51%–100% | 1.00 | 1.00 | — | — (4 unknown) | 59,717 | 882 | — | 26 s | — | 0/4 | 0/1 | 0/4 |
| B | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | — | — (3 unknown) | 71,414 | 825 | — | 27 s | 220.3 s | 3/3 | 0/1 | 0/3 |
| C | 3/3 (100%) | 44%–100% | 1.00 | 1.00 | — | — (3 unknown) | 99,684 | 1,596 | — | 43 s | 238.2 s | 3/3 | 0/1 | 0/3 |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | B / A | C / A |
|---|---|---|
| Cost | — | — |
| Input tokens (incl. cache) | 1.20 (0.64–1.21, 3 tasks) | 1.97 (1.64–3.38, 3 tasks) |
| Output tokens | 0.96 (0.78–1.17, 3 tasks) | 1.93 (1.10–2.10, 3 tasks) |
| Turns | — | — |
| Wall time | 1.05 (0.75–2.25, 3 tasks) | 1.68 (1.44–2.35, 3 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | B | C | B/A | C/A |
|---|---|---|---|---|---|
| `django-trap-html-escape` | 280,146 | — | — | — | — |
| `express-trap-req-get` | 59,942 | 72,543 | 98,102 | 1.21 | 1.64 |
| `flask-callers-ensure-sync` | 50,646 | 32,383 | 99,684 | 0.64 | 1.97 |
| `serde-edit-with-bound` | 59,492 | 71,414 | 200,871 | 1.20 | 3.38 |

## Success by task category

| Category | A | B | C |
|---|---|---|---|
| callers | 1/1 | 1/1 | 1/1 |
| edit | 1/1 | 1/1 | 1/1 |
| trap | 2/2 | 1/1 | 1/1 |

## By task

| Task | A | B | C | Cost A | Cost B | Cost C |
|---|---|---|---|---|---|---|
| `django-trap-html-escape` | 1/1 | — | — | — | — | — |
| `express-trap-req-get` | 1/1 | 1/1 | 1/1 | — | — | — |
| `flask-callers-ensure-sync` | 1/1 | 1/1 | 1/1 | — | — | — |
| `serde-edit-with-bound` | 1/1 | 1/1 | 1/1 | — | — | — |

## Failed runs

None.
