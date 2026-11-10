# L3 agent benchmark

Scope: pilot design; complete (48/48 runs).
Product profile: `context-first-pilot`. Arms: A=built-ins; B=default context+index_project with context note; C=legacy full surface with legacy navigation note; F=B plus an instruction to call `context` first (forced use, not natural use); H=B plus the opt-in Claude Code Grep hook (installed product configuration, not natural use).

Agent `claude` (2.1.319 (Claude Code), auth `subscription`), model `claude-opus-5-5` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `9bb6564`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 12/12 (100%) | 76%–100% | 1.00 | 1.00 | $0.073 | $0.80 | 35,320 | 1,172 | 4 | 16 s | — | 0/12 | 0/3 | 0/12 |
| B | 12/12 (100%) | 76%–100% | 1.00 | 1.00 | $0.066 | $0.77 | 30,286 | 1,058 | 4 | 14 s | 0.0 s | 3/12 | 0/3 | 0/12 |
| F | 12/12 (100%) | 76%–100% | 1.00 | 1.00 | $0.060 | $0.78 | 27,939 | 868 | 4 | 11 s | 0.0 s | 12/12 | 0/3 | 0/12 |
| H | 12/12 (100%) | 76%–100% | 1.00 | 1.00 | $0.067 | $0.76 | 27,033 | 1,004 | 3 | 12 s | 0.0 s | 3/12 | 0/3 | 0/12 |

## Prebuilt indexes

Arms with CCM copy an index built once per repository before their runs (the cache is shared across result directories); the arm table shows the copy time. One-time build time of the index each repository's runs copied:

| Repository | Build |
|---|---|
| django | 1230.5 s |
| express | 4.7 s |
| flask | 29.9 s |
| serde | 56.0 s |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | B / A | F / A | H / A |
|---|---|---|---|
| Cost | 0.96 (0.90–1.05, 4 tasks) | 1.04 (0.75–1.15, 4 tasks) | 0.96 (0.88–1.01, 4 tasks) |
| Input tokens (incl. cache) | 1.06 (0.53–1.08, 4 tasks) | 0.99 (0.51–1.27, 4 tasks) | 0.95 (0.53–0.97, 4 tasks) |
| Output tokens | 0.98 (0.70–1.04, 4 tasks) | 0.92 (0.56–1.29, 4 tasks) | 0.88 (0.73–1.02, 4 tasks) |
| Turns | 1.00 (0.75–1.08, 4 tasks) | 0.94 (0.50–1.20, 4 tasks) | 0.87 (0.75–1.07, 4 tasks) |
| Wall time | 0.97 (0.73–1.18, 4 tasks) | 0.96 (0.64–1.09, 4 tasks) | 0.86 (0.70–0.89, 4 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | B | F | H | B/A | F/A | H/A |
|---|---|---|---|---|---|---|---|
| `django-callers-iri-to-uri` | 35,351 | 18,904 | 17,982 | 18,634 | 0.53 | 0.51 | 0.53 |
| `express-locate-trust` | 23,666 | 24,984 | 24,095 | 22,370 | 1.06 | 1.02 | 0.95 |
| `flask-edit-method-route` | 38,361 | 40,843 | 48,769 | 37,098 | 1.06 | 1.27 | 0.97 |
| `serde-types-deserializer` | 36,394 | 39,198 | 35,050 | 35,009 | 1.08 | 0.96 | 0.96 |

## Success by task category

| Category | A | B | F | H |
|---|---|---|---|---|
| callers | 3/3 | 3/3 | 3/3 | 3/3 |
| edit | 3/3 | 3/3 | 3/3 | 3/3 |
| locate | 3/3 | 3/3 | 3/3 | 3/3 |
| types | 3/3 | 3/3 | 3/3 | 3/3 |

## By task

| Task | A | B | F | H | Cost A | Cost B | Cost F | Cost H |
|---|---|---|---|---|---|---|---|---|
| `django-callers-iri-to-uri` | 3/3 | 3/3 | 3/3 | 3/3 | $0.075 | $0.067 | $0.056 | $0.066 |
| `express-locate-trust` | 3/3 | 3/3 | 3/3 | 3/3 | $0.043 | $0.043 | $0.049 | $0.042 |
| `flask-edit-method-route` | 3/3 | 3/3 | 3/3 | 3/3 | $0.074 | $0.078 | $0.084 | $0.074 |
| `serde-types-deserializer` | 3/3 | 3/3 | 3/3 | 3/3 | $0.075 | $0.070 | $0.070 | $0.070 |

## Hook decisions (arm H)

Greps answered with the `context` packet (deny) and greps let through (pass), per run.

| Task | Run | Deny | Pass |
|---|---|---|---|
| `django-callers-iri-to-uri` | 1 | 1 | 0 |
| `django-callers-iri-to-uri` | 2 | 1 | 0 |
| `django-callers-iri-to-uri` | 3 | 1 | 0 |
| `express-locate-trust` | 1 | 0 | 1 |
| `express-locate-trust` | 2 | 0 | 1 |
| `express-locate-trust` | 3 | 0 | 1 |
| `flask-edit-method-route` | 1 | 0 | 3 |
| `flask-edit-method-route` | 2 | 0 | 2 |
| `flask-edit-method-route` | 3 | 0 | 2 |
| `serde-types-deserializer` | 1 | 0 | 2 |
| `serde-types-deserializer` | 2 | 0 | 3 |
| `serde-types-deserializer` | 3 | 0 | 3 |

## Failed runs

None.
