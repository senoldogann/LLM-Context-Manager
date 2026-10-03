# Search wrapper pilot: pilot-codex-search-shim-20261003

Settings: `{"product_commit": "d925d309bc5ce293795c5bf5aff7200628eb3698", "agent_version": "codex-cli 0.160.0", "model": "gpt-6.1-sol", "effort": "high", "tasks": ["flask-callers-ensure-sync", "flask-trap-load-dotenv", "express-trap-req-get", "express-callers-res-set", "serde-callers-expr-is-missing", "serde-edit-with-bound"], "arms": ["A", "S"], "repetitions": 1, "timeout_s": 1800, "shim_python": "/Users/dogan/Desktop/LLM-Context-Manager/benchmarks/.venv/bin/python3"}`

| Arm | Runs | Success | Total tokens | Uncached input | Cached input | Output | Shell | rg | Reads |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| A | 4 | 4/4 | 230,266 | 88,395 | 139,776 | 2,095 | 18 | 12 | 5 |
| S | 4 | 4/4 | 250,141 | 96,811 | 151,040 | 2,290 | 18 | 14 | 5 |

| Task | A tokens | S tokens | S/A | A success | S success |
|---|---:|---:|---:|---:|---:|
| express-callers-res-set | 69,039 | 61,792 | 0.90 | 1/1 | 1/1 |
| express-trap-req-get | 67,621 | 59,876 | 0.89 | 1/1 | 1/1 |
| flask-callers-ensure-sync | 45,074 | 69,884 | 1.55 | 1/1 | 1/1 |
| flask-trap-load-dotenv | 48,532 | 58,589 | 1.21 | 1/1 | 1/1 |

Median per-task S/A total-token ratio: 1.05 (4 tasks).

Wrapper modes in S: {}

Failures: none.
