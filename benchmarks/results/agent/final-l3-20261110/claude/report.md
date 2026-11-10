# L3 agent benchmark

Scope: preregistered final design; complete (288/288 runs).
Product profile: `preregistered`. Arms: A=built-ins; B=preregistered full CCM surface; C=preregistered search_code only; D=current product (`context` + `index_project` with the context note; amendment 14).

Agent `claude` (2.1.319 (Claude Code), auth `subscription`), model `claude-opus-5-5` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `6d502c8`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 72/72 (100%) | 95%–100% | 1.00 | 1.00 | $0.073 | $5.37 | 34,834 | 1,146 | 4 | 16 s | — | 0/72 | 0/27 | 0/72 |
| B | 72/72 (100%) | 95%–100% | 1.00 | 1.00 | $0.074 | $5.53 | 43,513 | 1,110 | 4 | 16 s | 0.0 s | 0/72 | 0/27 | 0/72 |
| C | 72/72 (100%) | 95%–100% | 1.00 | 1.00 | $0.074 | $5.40 | 37,180 | 1,101 | 4 | 16 s | 0.0 s | 0/72 | 0/27 | 0/72 |
| D | 72/72 (100%) | 95%–100% | 1.00 | 1.00 | $0.072 | $5.40 | 36,616 | 1,118 | 4 | 14 s | 0.0 s | 2/72 | 0/27 | 0/72 |

## Prebuilt indexes

Arms with CCM copy an index built once per repository before their runs (the cache is shared across result directories); the arm table shows the copy time. One-time build time of the index each repository's runs copied:

| Repository | Build |
|---|---|
| express | 11.8 s |
| flask | 68.2 s |
| serde | 120.7 s |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | B / A | C / A | D / A |
|---|---|---|---|
| Cost | 1.03 (0.98–1.08, 24 tasks) | 1.02 (0.99–1.06, 24 tasks) | 1.00 (0.96–1.05, 24 tasks) |
| Input tokens (incl. cache) | 1.24 (1.15–1.28, 24 tasks) | 1.01 (0.94–1.05, 24 tasks) | 1.05 (0.91–1.06, 24 tasks) |
| Output tokens | 0.97 (0.95–1.01, 24 tasks) | 0.99 (0.93–1.06, 24 tasks) | 0.98 (0.91–1.01, 24 tasks) |
| Turns | 1.00 (0.91–1.00, 24 tasks) | 0.95 (0.92–1.09, 24 tasks) | 0.97 (0.90–1.00, 24 tasks) |
| Wall time | 0.96 (0.88–1.04, 24 tasks) | 1.00 (0.94–1.06, 24 tasks) | 0.91 (0.85–0.94, 24 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | B | C | D | B/A | C/A | D/A |
|---|---|---|---|---|---|---|---|
| `express-callers-res-set` | 26,322 | 37,230 | 34,683 | 27,824 | 1.41 | 1.32 | 1.06 |
| `express-edit-handle-request` | 46,166 | 52,515 | 42,793 | 43,727 | 1.14 | 0.93 | 0.95 |
| `express-edit-normalize-type` | 49,882 | 63,304 | 42,336 | 40,108 | 1.27 | 0.85 | 0.80 |
| `express-edit-stringify` | 36,624 | 54,625 | 45,869 | 42,488 | 1.49 | 1.25 | 1.16 |
| `express-locate-trust` | 23,556 | 30,476 | 24,547 | 24,933 | 1.29 | 1.04 | 1.06 |
| `express-trap-req-get` | 26,795 | 44,333 | 46,813 | 35,425 | 1.65 | 1.75 | 1.32 |
| `express-trap-res-send` | 40,362 | 48,397 | 37,821 | 53,063 | 1.20 | 0.94 | 1.31 |
| `express-trap-sendfile-helper` | 24,976 | 31,795 | 25,820 | 32,365 | 1.27 | 1.03 | 1.30 |
| `flask-callers-ensure-sync` | 43,989 | 50,810 | 34,325 | 40,125 | 1.16 | 0.78 | 0.91 |
| `flask-edit-debug-flag` | 64,174 | 64,293 | 54,054 | 50,368 | 1.00 | 0.84 | 0.78 |
| `flask-edit-error-handler` | 53,150 | 48,937 | 54,658 | 40,774 | 0.92 | 1.03 | 0.77 |
| `flask-edit-method-route` | 41,559 | 47,737 | 39,888 | 40,151 | 1.15 | 0.96 | 0.97 |
| `flask-files-globals` | 27,908 | 35,132 | 26,132 | 29,407 | 1.26 | 0.94 | 1.05 |
| `flask-locate-app-string` | 18,294 | 27,300 | 21,690 | 19,263 | 1.49 | 1.19 | 1.05 |
| `flask-trap-load-dotenv` | 42,438 | 47,559 | 39,929 | 42,842 | 1.12 | 0.94 | 1.01 |
| `flask-types-scaffold` | 25,819 | 32,518 | 30,424 | 27,403 | 1.26 | 1.18 | 1.06 |
| `serde-callers-expr-is-missing` | 27,884 | 33,270 | 27,624 | 21,189 | 1.19 | 0.99 | 0.76 |
| `serde-edit-deserialize-seq` | 64,825 | 63,466 | 65,500 | 56,514 | 0.98 | 1.01 | 0.87 |
| `serde-edit-with-bound` | 68,385 | 58,079 | 49,698 | 56,657 | 0.85 | 0.73 | 0.83 |
| `serde-edit-wrap-field` | 58,631 | 75,106 | 62,325 | 72,767 | 1.28 | 1.06 | 1.24 |
| `serde-locate-tag-conflict` | 23,890 | 30,507 | 28,102 | 25,441 | 1.28 | 1.18 | 1.06 |
| `serde-trap-deserialize-struct` | 36,024 | 36,866 | 31,450 | 38,179 | 1.02 | 0.87 | 1.06 |
| `serde-trap-effective-style` | 23,958 | 30,860 | 24,968 | 25,267 | 1.29 | 1.04 | 1.05 |
| `serde-types-deserializer` | 31,562 | 38,568 | 32,118 | 25,928 | 1.22 | 1.02 | 0.82 |

## Success by task category

| Category | A | B | C | D |
|---|---|---|---|---|
| callers | 9/9 | 9/9 | 9/9 | 9/9 |
| edit | 27/27 | 27/27 | 27/27 | 27/27 |
| files | 3/3 | 3/3 | 3/3 | 3/3 |
| locate | 9/9 | 9/9 | 9/9 | 9/9 |
| trap | 18/18 | 18/18 | 18/18 | 18/18 |
| types | 6/6 | 6/6 | 6/6 | 6/6 |

## By task

| Task | A | B | C | D | Cost A | Cost B | Cost C | Cost D |
|---|---|---|---|---|---|---|---|---|
| `express-callers-res-set` | 3/3 | 3/3 | 3/3 | 3/3 | $0.070 | $0.074 | $0.077 | $0.071 |
| `express-edit-handle-request` | 3/3 | 3/3 | 3/3 | 3/3 | $0.102 | $0.098 | $0.090 | $0.099 |
| `express-edit-normalize-type` | 3/3 | 3/3 | 3/3 | 3/3 | $0.105 | $0.097 | $0.072 | $0.090 |
| `express-edit-stringify` | 3/3 | 3/3 | 3/3 | 3/3 | $0.079 | $0.088 | $0.086 | $0.085 |
| `express-locate-trust` | 3/3 | 3/3 | 3/3 | 3/3 | $0.042 | $0.045 | $0.042 | $0.042 |
| `express-trap-req-get` | 3/3 | 3/3 | 3/3 | 3/3 | $0.067 | $0.088 | $0.094 | $0.077 |
| `express-trap-res-send` | 3/3 | 3/3 | 3/3 | 3/3 | $0.071 | $0.070 | $0.071 | $0.081 |
| `express-trap-sendfile-helper` | 3/3 | 3/3 | 3/3 | 3/3 | $0.055 | $0.061 | $0.057 | $0.060 |
| `flask-callers-ensure-sync` | 3/3 | 3/3 | 3/3 | 3/3 | $0.084 | $0.085 | $0.077 | $0.088 |
| `flask-edit-debug-flag` | 3/3 | 3/3 | 3/3 | 3/3 | $0.102 | $0.107 | $0.107 | $0.100 |
| `flask-edit-error-handler` | 3/3 | 3/3 | 3/3 | 3/3 | $0.082 | $0.081 | $0.086 | $0.077 |
| `flask-edit-method-route` | 3/3 | 3/3 | 3/3 | 3/3 | $0.075 | $0.074 | $0.072 | $0.070 |
| `flask-files-globals` | 3/3 | 3/3 | 3/3 | 3/3 | $0.058 | $0.062 | $0.060 | $0.062 |
| `flask-locate-app-string` | 3/3 | 3/3 | 3/3 | 3/3 | $0.037 | $0.046 | $0.039 | $0.037 |
| `flask-trap-load-dotenv` | 3/3 | 3/3 | 3/3 | 3/3 | $0.071 | $0.068 | $0.067 | $0.076 |
| `flask-types-scaffold` | 3/3 | 3/3 | 3/3 | 3/3 | $0.055 | $0.057 | $0.059 | $0.056 |
| `serde-callers-expr-is-missing` | 3/3 | 3/3 | 3/3 | 3/3 | $0.062 | $0.074 | $0.072 | $0.058 |
| `serde-edit-deserialize-seq` | 3/3 | 3/3 | 3/3 | 3/3 | $0.119 | $0.109 | $0.108 | $0.103 |
| `serde-edit-with-bound` | 3/3 | 3/3 | 3/3 | 3/3 | $0.104 | $0.097 | $0.094 | $0.096 |
| `serde-edit-wrap-field` | 3/3 | 3/3 | 3/3 | 3/3 | $0.114 | $0.133 | $0.129 | $0.150 |
| `serde-locate-tag-conflict` | 3/3 | 3/3 | 3/3 | 3/3 | $0.045 | $0.046 | $0.050 | $0.047 |
| `serde-trap-deserialize-struct` | 3/3 | 3/3 | 3/3 | 3/3 | $0.074 | $0.070 | $0.073 | $0.074 |
| `serde-trap-effective-style` | 3/3 | 3/3 | 3/3 | 3/3 | $0.049 | $0.053 | $0.048 | $0.048 |
| `serde-types-deserializer` | 3/3 | 3/3 | 3/3 | 3/3 | $0.070 | $0.062 | $0.070 | $0.056 |

## Failed runs

None.
