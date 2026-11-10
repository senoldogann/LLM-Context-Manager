# L3 agent benchmark

Scope: preregistered final design; complete (288/288 runs).
Product profile: `preregistered`. Arms: A=built-ins; B=preregistered full CCM surface; C=preregistered search_code only; D=current product (`context` + `index_project` with the context note; amendment 14).

Agent `codex` (codex-cli 0.167.0, auth `subscription`), model `gpt-6-luna` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `6d502c8`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 66/72 (92%) | 83%–96% | 0.94 | 0.96 | — | — (72 unknown) | 47,253 | 732 | — | 21 s | — | 0/72 | 0/27 | 0/72 |
| B | 65/72 (90%) | 81%–95% | 0.96 | 0.97 | — | — (72 unknown) | 100,586 | 1,222 | — | 31 s | 0.0 s | 72/72 | 0/27 | 0/72 |
| C | 59/72 (82%) | 72%–89% | 0.90 | 0.94 | — | — (72 unknown) | 118,726 | 1,140 | — | 33 s | 0.0 s | 72/72 | 0/27 | 0/72 |
| D | 61/72 (85%) | 75%–91% | 0.95 | 0.98 | — | — (72 unknown) | 58,956 | 832 | — | 24 s | 0.0 s | 65/72 | 0/27 | 0/72 |

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
| Cost | — | — | — |
| Input tokens (incl. cache) | 1.78 (1.68–2.09, 24 tasks) | 2.09 (1.85–2.65, 24 tasks) | 1.12 (1.06–1.30, 24 tasks) |
| Output tokens | 1.49 (1.33–1.71, 24 tasks) | 1.47 (1.31–1.81, 24 tasks) | 1.09 (1.01–1.26, 24 tasks) |
| Turns | — | — | — |
| Wall time | 1.43 (1.33–1.59, 24 tasks) | 1.50 (1.37–1.74, 24 tasks) | 1.16 (1.01–1.26, 24 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | B | C | D | B/A | C/A | D/A |
|---|---|---|---|---|---|---|---|
| `express-callers-res-set` | 65,961 | 94,599 | 137,429 | 32,995 | 1.43 | 2.08 | 0.50 |
| `express-edit-handle-request` | 83,269 | 153,393 | 117,211 | 104,032 | 1.84 | 1.41 | 1.25 |
| `express-edit-normalize-type` | 70,220 | 120,713 | 137,809 | 78,610 | 1.72 | 1.96 | 1.12 |
| `express-edit-stringify` | 119,150 | 146,727 | 139,470 | 128,351 | 1.23 | 1.17 | 1.08 |
| `express-locate-trust` | 31,524 | 53,197 | 32,923 | 32,788 | 1.69 | 1.04 | 1.04 |
| `express-trap-req-get` | 49,738 | 134,729 | 127,237 | 32,199 | 2.71 | 2.56 | 0.65 |
| `express-trap-res-send` | 40,866 | 61,215 | 135,291 | 56,293 | 1.50 | 3.31 | 1.38 |
| `express-trap-sendfile-helper` | 31,827 | 71,921 | 97,468 | 36,949 | 2.26 | 3.06 | 1.16 |
| `flask-callers-ensure-sync` | 44,893 | 97,395 | 135,548 | 32,888 | 2.17 | 3.02 | 0.73 |
| `flask-edit-debug-flag` | 83,055 | 115,341 | 140,017 | 74,073 | 1.39 | 1.69 | 0.89 |
| `flask-edit-error-handler` | 60,861 | 100,602 | 129,290 | 82,393 | 1.65 | 2.12 | 1.35 |
| `flask-edit-method-route` | 54,984 | 111,068 | 85,401 | 78,550 | 2.02 | 1.55 | 1.43 |
| `flask-files-globals` | 27,948 | 120,272 | 89,858 | 73,313 | 4.30 | 3.22 | 2.62 |
| `flask-locate-app-string` | 33,620 | 65,735 | 66,341 | 73,146 | 1.96 | 1.97 | 2.18 |
| `flask-trap-load-dotenv` | 44,649 | 77,742 | 153,787 | 50,442 | 1.74 | 3.44 | 1.13 |
| `flask-types-scaffold` | 39,208 | 71,204 | 82,021 | 54,297 | 1.82 | 2.09 | 1.38 |
| `serde-callers-expr-is-missing` | 54,936 | 67,143 | 137,254 | 32,965 | 1.22 | 2.50 | 0.60 |
| `serde-edit-deserialize-seq` | 67,954 | 155,242 | 130,240 | 71,878 | 2.28 | 1.92 | 1.06 |
| `serde-edit-with-bound` | 72,119 | 123,922 | 110,594 | 80,679 | 1.72 | 1.53 | 1.12 |
| `serde-edit-wrap-field` | 74,306 | 155,228 | 144,595 | 92,839 | 2.09 | 1.95 | 1.25 |
| `serde-locate-tag-conflict` | 32,599 | 70,363 | 56,069 | 61,246 | 2.16 | 1.72 | 1.88 |
| `serde-trap-deserialize-struct` | 42,372 | 71,144 | 112,428 | 45,423 | 1.68 | 2.65 | 1.07 |
| `serde-trap-effective-style` | 44,584 | 60,194 | 143,500 | 35,944 | 1.35 | 3.22 | 0.81 |
| `serde-types-deserializer` | 37,501 | 214,530 | 131,668 | 48,682 | 5.72 | 3.51 | 1.30 |

## Success by task category

| Category | A | B | C | D |
|---|---|---|---|---|
| callers | 9/9 | 7/9 | 7/9 | 9/9 |
| edit | 25/27 | 27/27 | 25/27 | 23/27 |
| files | 2/3 | 1/3 | 1/3 | 1/3 |
| locate | 9/9 | 9/9 | 8/9 | 9/9 |
| trap | 15/18 | 15/18 | 12/18 | 16/18 |
| types | 6/6 | 6/6 | 6/6 | 3/6 |

## By task

| Task | A | B | C | D | Cost A | Cost B | Cost C | Cost D |
|---|---|---|---|---|---|---|---|---|
| `express-callers-res-set` | 3/3 | 1/3 | 1/3 | 3/3 | — | — | — | — |
| `express-edit-handle-request` | 3/3 | 3/3 | 3/3 | 2/3 | — | — | — | — |
| `express-edit-normalize-type` | 1/3 | 3/3 | 1/3 | 0/3 | — | — | — | — |
| `express-edit-stringify` | 3/3 | 3/3 | 3/3 | 3/3 | — | — | — | — |
| `express-locate-trust` | 3/3 | 3/3 | 3/3 | 3/3 | — | — | — | — |
| `express-trap-req-get` | 3/3 | 2/3 | 1/3 | 3/3 | — | — | — | — |
| `express-trap-res-send` | 3/3 | 3/3 | 2/3 | 3/3 | — | — | — | — |
| `express-trap-sendfile-helper` | 0/3 | 1/3 | 0/3 | 2/3 | — | — | — | — |
| `flask-callers-ensure-sync` | 3/3 | 3/3 | 3/3 | 3/3 | — | — | — | — |
| `flask-edit-debug-flag` | 3/3 | 3/3 | 3/3 | 3/3 | — | — | — | — |
| `flask-edit-error-handler` | 3/3 | 3/3 | 3/3 | 3/3 | — | — | — | — |
| `flask-edit-method-route` | 3/3 | 3/3 | 3/3 | 3/3 | — | — | — | — |
| `flask-files-globals` | 2/3 | 1/3 | 1/3 | 1/3 | — | — | — | — |
| `flask-locate-app-string` | 3/3 | 3/3 | 2/3 | 3/3 | — | — | — | — |
| `flask-trap-load-dotenv` | 3/3 | 3/3 | 3/3 | 2/3 | — | — | — | — |
| `flask-types-scaffold` | 3/3 | 3/3 | 3/3 | 0/3 | — | — | — | — |
| `serde-callers-expr-is-missing` | 3/3 | 3/3 | 3/3 | 3/3 | — | — | — | — |
| `serde-edit-deserialize-seq` | 3/3 | 3/3 | 3/3 | 3/3 | — | — | — | — |
| `serde-edit-with-bound` | 3/3 | 3/3 | 3/3 | 3/3 | — | — | — | — |
| `serde-edit-wrap-field` | 3/3 | 3/3 | 3/3 | 3/3 | — | — | — | — |
| `serde-locate-tag-conflict` | 3/3 | 3/3 | 3/3 | 3/3 | — | — | — | — |
| `serde-trap-deserialize-struct` | 3/3 | 3/3 | 3/3 | 3/3 | — | — | — | — |
| `serde-trap-effective-style` | 3/3 | 3/3 | 3/3 | 3/3 | — | — | — | — |
| `serde-types-deserializer` | 3/3 | 3/3 | 3/3 | 3/3 | — | — | — | — |

## Failed runs

- `express-callers-res-set` B#2: missing lib/router/index.js:sendOptionsResponse
- `express-callers-res-set` B#3: missing lib/router/index.js:sendOptionsResponse
- `express-callers-res-set` C#1: missing lib/response.js:attachment
- `express-callers-res-set` C#3: missing lib/response.js:attachment
- `express-edit-handle-request` D#2: extra lib/router/index.js:trim_prefix
- `express-edit-normalize-type` A#2: missing lib/utils.js:normalizeTypes
- `express-edit-normalize-type` A#3: missing lib/utils.js:normalizeTypes
- `express-edit-normalize-type` C#1: missing lib/utils.js:normalizeTypes
- `express-edit-normalize-type` C#2: missing lib/utils.js:normalizeTypes
- `express-edit-normalize-type` D#1: missing lib/utils.js:normalizeTypes
- `express-edit-normalize-type` D#2: missing lib/utils.js:normalizeTypes
- `express-edit-normalize-type` D#3: missing lib/utils.js:normalizeTypes
- `express-trap-req-get` B#3: missing lib/request.js:range, lib/request.js:hostname
- `express-trap-req-get` C#2: missing lib/request.js:range, lib/request.js:protocol, lib/request.js:hostname, lib/request.js:xhr
- `express-trap-req-get` C#3: missing lib/request.js:range, lib/request.js:protocol, lib/request.js:hostname, lib/request.js:xhr
- `express-trap-res-send` C#2: missing lib/response.js:render
- `express-trap-sendfile-helper` A#1: missing lib/response.js:sendFile, lib/response.js:sendfile
- `express-trap-sendfile-helper` A#2: missing lib/response.js:sendFile, lib/response.js:sendfile
- `express-trap-sendfile-helper` A#3: missing lib/response.js:sendFile, lib/response.js:sendfile
- `express-trap-sendfile-helper` B#1: missing lib/response.js:sendFile, lib/response.js:sendfile
- `express-trap-sendfile-helper` B#3: missing lib/response.js:sendFile, lib/response.js:sendfile
- `express-trap-sendfile-helper` C#1: missing lib/response.js:sendFile, lib/response.js:sendfile
- `express-trap-sendfile-helper` C#2: missing lib/response.js:sendFile, lib/response.js:sendfile
- `express-trap-sendfile-helper` C#3: missing lib/response.js:sendFile, lib/response.js:sendfile
- `express-trap-sendfile-helper` D#1: missing lib/response.js:sendFile, lib/response.js:sendfile
- `flask-files-globals` A#3: missing src/flask/json/__init__.py
- `flask-files-globals` B#1: missing src/flask/json/__init__.py
- `flask-files-globals` B#2: missing src/flask/json/__init__.py
- `flask-files-globals` C#1: missing src/flask/json/__init__.py
- `flask-files-globals` C#2: missing src/flask/json/__init__.py
- `flask-files-globals` D#1: missing src/flask/json/__init__.py
- `flask-files-globals` D#2: missing src/flask/json/__init__.py
- `flask-locate-app-string` C#3: missing src/flask/cli.py:find_app_by_string; extra src/flask/cli.py:locate_app
- `flask-trap-load-dotenv` D#3: missing src/flask/app.py:run
- `flask-types-scaffold` D#1: missing src/flask/blueprints.py:Blueprint
- `flask-types-scaffold` D#2: missing src/flask/blueprints.py:Blueprint
- `flask-types-scaffold` D#3: missing src/flask/blueprints.py:Blueprint
