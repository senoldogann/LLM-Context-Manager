# L3 agent benchmark

Scope: pilot design; complete (32/32 runs).
Product profile: `context-first-pilot`. Arms: A=built-ins; B=default context+index_project with context note; C=legacy full surface with legacy navigation note; F=B plus an instruction to call `context` first (forced use, not natural use); H=B plus the opt-in Claude Code Grep hook (installed product configuration, not natural use).

Agent `claude` (2.1.319 (Claude Code), auth `subscription`), model `claude-opus-5-5` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `ed0f4e8`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 8/8 (100%) | 68%–100% | 1.00 | 1.00 | $0.074 | $0.64 | 34,860 | 1,410 | 4 | 20 s | — | 0/8 | 0/3 | 0/8 |
| B | 8/8 (100%) | 68%–100% | 1.00 | 1.00 | $0.080 | $0.80 | 38,873 | 1,309 | 4 | 18 s | 0.1 s | 0/8 | 0/3 | 0/8 |
| F | 8/8 (100%) | 68%–100% | 1.00 | 1.00 | $0.095 | $0.82 | 50,853 | 1,370 | 6 | 20 s | 0.1 s | 8/8 | 0/3 | 0/8 |
| H | 8/8 (100%) | 68%–100% | 1.00 | 1.00 | $0.075 | $0.72 | 33,129 | 1,274 | 4 | 19 s | 0.1 s | 0/8 | 0/3 | 0/8 |

## Prebuilt indexes

Arms with CCM copy an index built once per repository before their runs (the cache is shared across result directories); the arm table shows the copy time. One-time build time of the index each repository's runs copied:

| Repository | Build |
|---|---|
| django | 4468.6 s |
| express | 82.9 s |
| flask | 455.3 s |
| serde | 183.2 s |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | B / A | F / A | H / A |
|---|---|---|---|
| Cost | 1.20 (1.00–1.60, 8 tasks) | 1.31 (1.08–1.53, 8 tasks) | 1.16 (0.87–1.51, 8 tasks) |
| Input tokens (incl. cache) | 1.10 (1.05–2.10, 8 tasks) | 1.18 (0.86–1.97, 8 tasks) | 1.10 (0.72–1.64, 8 tasks) |
| Output tokens | 1.02 (0.78–1.37, 8 tasks) | 1.11 (0.85–1.31, 8 tasks) | 1.01 (0.79–1.30, 8 tasks) |
| Turns | 1.00 (0.70–1.67, 8 tasks) | 1.07 (0.70–1.67, 8 tasks) | 1.00 (0.75–1.50, 8 tasks) |
| Wall time | 1.02 (0.79–1.22, 8 tasks) | 1.23 (0.84–1.50, 8 tasks) | 1.07 (0.89–1.58, 8 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | B | F | H | B/A | F/A | H/A |
|---|---|---|---|---|---|---|---|
| `django-callers-make-aware` | 34,771 | 38,037 | 20,913 | 27,049 | 1.09 | 0.60 | 0.78 |
| `django-trap-capfirst` | 43,784 | 111,931 | 56,005 | 72,019 | 2.56 | 1.28 | 1.64 |
| `express-edit-normalize-type` | 34,948 | 36,674 | 40,211 | 46,908 | 1.05 | 1.15 | 1.34 |
| `express-trap-res-send` | 25,919 | 39,542 | 52,859 | 27,831 | 1.53 | 2.04 | 1.07 |
| `flask-edit-error-handler` | 59,279 | 38,204 | 71,813 | 38,211 | 0.64 | 1.21 | 0.64 |
| `flask-locate-app-string` | 15,238 | 16,384 | 17,060 | 25,828 | 1.08 | 1.12 | 1.69 |
| `serde-edit-wrap-field` | 76,408 | 84,946 | 65,408 | 54,684 | 1.11 | 0.86 | 0.72 |
| `serde-trap-deserialize-struct` | 24,763 | 52,099 | 48,847 | 28,047 | 2.10 | 1.97 | 1.13 |

## Success by task category

| Category | A | B | F | H |
|---|---|---|---|---|
| callers | 1/1 | 1/1 | 1/1 | 1/1 |
| edit | 3/3 | 3/3 | 3/3 | 3/3 |
| locate | 1/1 | 1/1 | 1/1 | 1/1 |
| trap | 3/3 | 3/3 | 3/3 | 3/3 |

## By task

| Task | A | B | F | H | Cost A | Cost B | Cost F | Cost H |
|---|---|---|---|---|---|---|---|---|
| `django-callers-make-aware` | 1/1 | 1/1 | 1/1 | 1/1 | $0.073 | $0.079 | $0.135 | $0.076 |
| `django-trap-capfirst` | 1/1 | 1/1 | 1/1 | 1/1 | $0.116 | $0.185 | $0.111 | $0.144 |
| `express-edit-normalize-type` | 1/1 | 1/1 | 1/1 | 1/1 | $0.076 | $0.076 | $0.082 | $0.118 |
| `express-trap-res-send` | 1/1 | 1/1 | 1/1 | 1/1 | $0.062 | $0.081 | $0.090 | $0.066 |
| `flask-edit-error-handler` | 1/1 | 1/1 | 1/1 | 1/1 | $0.092 | $0.074 | $0.099 | $0.075 |
| `flask-locate-app-string` | 1/1 | 1/1 | 1/1 | 1/1 | $0.032 | $0.033 | $0.042 | $0.048 |
| `serde-edit-wrap-field` | 1/1 | 1/1 | 1/1 | 1/1 | $0.132 | $0.177 | $0.171 | $0.114 |
| `serde-trap-deserialize-struct` | 1/1 | 1/1 | 1/1 | 1/1 | $0.057 | $0.092 | $0.088 | $0.073 |

## Hook decisions (arm H)

Greps answered with the `context` packet (deny) and greps let through (pass), per run.

| Task | Run | Deny | Pass |
|---|---|---|---|
| `django-callers-make-aware` | 1 | 0 | 1 |
| `django-trap-capfirst` | 1 | 0 | 1 |
| `express-edit-normalize-type` | 1 | 0 | 1 |
| `express-trap-res-send` | 1 | 0 | 1 |
| `flask-edit-error-handler` | 1 | 0 | 1 |
| `flask-locate-app-string` | 1 | 0 | 1 |
| `serde-edit-wrap-field` | 1 | 0 | 2 |
| `serde-trap-deserialize-struct` | 1 | 0 | 1 |

## Failed runs

None.
