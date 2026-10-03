# Search wrapper pilot: pilot-codex-search-shim-20261003-check

Settings: `{"product_commit": "c0ce0a26b7ba0748ebd380da73bd7b3c4ff41bdb", "agent_version": "codex-cli 0.160.0", "model": "gpt-6.1-sol", "effort": "high", "tasks": ["flask-callers-ensure-sync"], "arms": ["S"], "repetitions": 1, "timeout_s": 1800, "shim_python": "/Users/dogan/Desktop/LLM-Context-Manager/benchmarks/.venv/bin/python3"}`

| Arm | Runs | Success | Total tokens | Uncached input | Cached input | Output | Shell | rg | Reads |
|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| S | 1 | 1/1 | 49,383 | 17,429 | 30,720 | 1,234 | 4 | 3 | 1 |

| Task | A tokens | S tokens | S/A | A success | S success |
|---|---:|---:|---:|---:|---:|

Wrapper modes in S: {}

Failures:
- flask-callers-ensure-sync__S__1: the rg wrapper was not invoked although the agent ran rg
