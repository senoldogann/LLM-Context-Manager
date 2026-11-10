# L3 agent benchmark

Scope: pilot design; complete (32/32 runs).
Product profile: `context-first-pilot`. Arms: A=built-ins; B=default context+index_project with context note; C=legacy full surface with legacy navigation note; F=B plus an instruction to call `context` first (forced use, not natural use); H=B plus the opt-in Claude Code Grep hook (installed product configuration, not natural use).

Agent `claude` (2.1.319 (Claude Code), auth `subscription`), model `claude-opus-5-5` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `9bb6564`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 8/8 (100%) | 68%–100% | 1.00 | 1.00 | $0.076 | $0.63 | 43,102 | 1,392 | 5 | 17 s | — | 0/8 | 0/3 | 0/8 |
| B | 8/8 (100%) | 68%–100% | 1.00 | 1.00 | $0.080 | $0.65 | 36,946 | 1,162 | 4 | 15 s | 0.0 s | 1/8 | 0/3 | 0/8 |
| F | 8/8 (100%) | 68%–100% | 1.00 | 1.00 | $0.067 | $0.56 | 34,450 | 1,066 | 4 | 13 s | 0.0 s | 8/8 | 0/3 | 0/8 |
| H | 8/8 (100%) | 68%–100% | 1.00 | 1.00 | $0.071 | $0.65 | 35,724 | 1,149 | 4 | 13 s | 0.0 s | 1/8 | 0/3 | 0/8 |

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
| Cost | 1.04 (0.85–1.23, 8 tasks) | 1.08 (0.61–1.12, 8 tasks) | 1.03 (0.81–1.34, 8 tasks) |
| Input tokens (incl. cache) | 1.08 (0.75–1.48, 8 tasks) | 0.92 (0.64–1.32, 8 tasks) | 1.07 (0.59–1.58, 8 tasks) |
| Output tokens | 1.03 (0.67–1.31, 8 tasks) | 0.95 (0.49–1.14, 8 tasks) | 0.96 (0.66–1.15, 8 tasks) |
| Turns | 1.00 (0.71–1.33, 8 tasks) | 0.88 (0.67–1.25, 8 tasks) | 0.94 (0.57–1.33, 8 tasks) |
| Wall time | 1.10 (0.80–1.38, 8 tasks) | 0.93 (0.58–1.13, 8 tasks) | 0.96 (0.73–1.27, 8 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | B | F | H | B/A | F/A | H/A |
|---|---|---|---|---|---|---|---|
| `django-callers-iri-to-uri` | 56,682 | 18,014 | 18,085 | 18,671 | 0.32 | 0.32 | 0.33 |
| `django-trap-capfirst` | 77,701 | 58,045 | 49,837 | 45,850 | 0.75 | 0.64 | 0.59 |
| `express-edit-normalize-type` | 54,406 | 47,081 | 41,311 | 37,163 | 0.87 | 0.76 | 0.68 |
| `express-locate-trust` | 23,440 | 25,285 | 16,777 | 24,931 | 1.08 | 0.72 | 1.06 |
| `flask-edit-method-route` | 34,549 | 37,658 | 46,889 | 37,183 | 1.09 | 1.36 | 1.08 |
| `flask-locate-app-string` | 15,714 | 25,846 | 17,090 | 25,562 | 1.64 | 1.09 | 1.63 |
| `serde-edit-wrap-field` | 51,654 | 63,083 | 68,048 | 81,401 | 1.22 | 1.32 | 1.58 |
| `serde-types-deserializer` | 24,522 | 36,233 | 27,590 | 34,284 | 1.48 | 1.13 | 1.40 |

## Success by task category

| Category | A | B | F | H |
|---|---|---|---|---|
| callers | 1/1 | 1/1 | 1/1 | 1/1 |
| edit | 3/3 | 3/3 | 3/3 | 3/3 |
| locate | 2/2 | 2/2 | 2/2 | 2/2 |
| trap | 1/1 | 1/1 | 1/1 | 1/1 |
| types | 1/1 | 1/1 | 1/1 | 1/1 |

## By task

| Task | A | B | F | H | Cost A | Cost B | Cost F | Cost H |
|---|---|---|---|---|---|---|---|---|
| `django-callers-iri-to-uri` | 1/1 | 1/1 | 1/1 | 1/1 | $0.095 | $0.060 | $0.058 | $0.066 |
| `django-trap-capfirst` | 1/1 | 1/1 | 1/1 | 1/1 | $0.140 | $0.120 | $0.080 | $0.114 |
| `express-edit-normalize-type` | 1/1 | 1/1 | 1/1 | 1/1 | $0.085 | $0.082 | $0.090 | $0.078 |
| `express-locate-trust` | 1/1 | 1/1 | 1/1 | 1/1 | $0.041 | $0.044 | $0.039 | $0.042 |
| `flask-edit-method-route` | 1/1 | 1/1 | 1/1 | 1/1 | $0.067 | $0.078 | $0.075 | $0.076 |
| `flask-locate-app-string` | 1/1 | 1/1 | 1/1 | 1/1 | $0.035 | $0.092 | $0.042 | $0.047 |
| `serde-edit-wrap-field` | 1/1 | 1/1 | 1/1 | 1/1 | $0.108 | $0.110 | $0.119 | $0.167 |
| `serde-types-deserializer` | 1/1 | 1/1 | 1/1 | 1/1 | $0.054 | $0.067 | $0.059 | $0.056 |

## Hook decisions (arm H)

Greps answered with the `context` packet (deny) and greps let through (pass), per run.

| Task | Run | Deny | Pass |
|---|---|---|---|
| `django-callers-iri-to-uri` | 1 | 1 | 0 |
| `django-trap-capfirst` | 1 | 0 | 1 |
| `express-edit-normalize-type` | 1 | 0 | 1 |
| `express-locate-trust` | 1 | 0 | 1 |
| `flask-edit-method-route` | 1 | 0 | 2 |
| `flask-locate-app-string` | 1 | 0 | 1 |
| `serde-edit-wrap-field` | 1 | 0 | 1 |
| `serde-types-deserializer` | 1 | 0 | 3 |

## Failed runs

None.
