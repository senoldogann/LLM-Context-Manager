# Rescoring under amendment 16 (generic owners)

The recorded runs in `claude/runs/` and `codex/runs/` are unchanged. This table rescores their
stored answers with the corrected scorer (amendment 16: balanced `<...>` groups are part of the
owner, not the name). Produced with
`PYTHONPATH=. uv run python scripts/rescore_report.py results/agent/large-repo-20261111/{claude,codex}`.

| Agent | Arm | Recorded success | Rescored success |
|---|---|---:|---:|
| Claude Code | A | 53/54 | 54/54 |
| Claude Code | D | 54/54 | 54/54 |
| Codex | A | 44/54 | 44/54 |
| Codex | D | 46/54 | 47/54 |

Runs whose success changes: Claude `tokio-callers-with-scheduler` A#2 and Codex
`tokio-callers-with-scheduler` D#3, both fail → pass. Both answered
`current_thread/mod.rs:Arc<Handle>.schedule` and `…Arc<Handle>.unhandled_panic`.

The task is one of the six tuned tasks, so the held-out (primary) success counts do not change:
Codex A 28/36, D 29/36; Claude 36/36 in both arms. On the tuned tasks, Claude A goes from 17/18
to 18/18 and Codex D from 17/18 to 18/18. Input-token ratios do not depend on scoring, and the
decisions of amendment 15 (no detectable difference for either agent) are unchanged.

Rescoring every other published result set under `results/agent/` changes no run.
