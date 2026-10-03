# Search wrapper pilot: pilot-codex-search-shim-20261003-v2

Settings: `{"product_commit": "3814a876d6aef350a925db95b2ddc55ad70f654a", "agent_version": "codex-cli 0.160.0", "model": "gpt-6.1-sol", "effort": "high", "tasks": ["flask-callers-ensure-sync", "flask-trap-load-dotenv", "express-trap-req-get", "express-callers-res-set", "serde-callers-expr-is-missing", "serde-edit-with-bound"], "arms": ["A", "S"], "repetitions": 1, "timeout_s": 1800, "shim_python": "/Users/dogan/Desktop/LLM-Context-Manager/benchmarks/.venv/bin/python3"}`

| Arm | Runs | Success | Total tokens | Uncached input | Cached input | Output | Shell | rg | Reads |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| A | 6 | 6/6 | 293,585 | 120,131 | 169,984 | 3,470 | 22 | 18 | 9 |
| S | 6 | 6/6 | 334,025 | 145,916 | 184,448 | 3,661 | 28 | 21 | 12 |

| Task | A tokens | S tokens | S/A | A success | S success |
|---|---:|---:|---:|---:|---:|
| express-callers-res-set | 53,851 | 62,434 | 1.16 | 1/1 | 1/1 |
| express-trap-req-get | 59,335 | 81,893 | 1.38 | 1/1 | 1/1 |
| flask-callers-ensure-sync | 48,087 | 53,700 | 1.12 | 1/1 | 1/1 |
| flask-trap-load-dotenv | 34,253 | 35,711 | 1.04 | 1/1 | 1/1 |
| serde-callers-expr-is-missing | 50,789 | 52,623 | 1.04 | 1/1 | 1/1 |
| serde-edit-with-bound | 47,270 | 47,664 | 1.01 | 1/1 | 1/1 |

Median per-task S/A total-token ratio: 1.08 (6 tasks).

Wrapper modes in S: {'delegate-shape': 11, 'compact': 12, 'raw-smaller': 1}
Compacted rg calls: 56,627 raw bytes shown as 8,235 (0.15).

Failures: none.
