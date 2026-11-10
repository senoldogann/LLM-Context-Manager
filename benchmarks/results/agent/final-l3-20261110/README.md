# Preregistered final L3 run (2026-11-10)

Design: `benchmarks/agent/PREREGISTRATION.md` with amendment 14. The 24 preregistered tasks
(Flask, Express, Serde) were run with 3 repetitions in each of four arms, for both agents:

- A, B and C exactly as preregistered (A built-ins; B the full legacy CCM surface with its
  navigation note; C `search_code` only);
- D, the current product (`context` + `index_project` with the context note), added before the
  run and reported as its own arm.

Everything ran at commit `6d502c8`, with one release build (ccm-cli `5ea5c4bd…`, ccm-mcp
`601fc2c0…`). The agents were Claude Code 2.1.319 with `claude-opus-5-5` and codex-cli 0.167.0
with `gpt-6-luna`, both at effort high, on subscriptions, with local embeddings. Each agent did
288 runs and every run has a record. The reports are `claude/report.md` and `codex/report.md`.

The Codex run set stopped once at its usage limit before run 124, which was not recorded. The
owner then upgraded the plan, and the same command resumed from the recorded runs. No record was
lost or overwritten.

## Results

Input tokens (including cache) are given as a ratio per task: the arm mean over repetitions
divided by the A mean. The median over tasks is shown with a fixed-seed bootstrap 95 % interval.

| Agent | Set | B/A | C/A | D/A |
|---|---|---|---|---|
| Claude Code | all 24 tasks | 1.24 (1.15–1.28) | 1.01 (0.94–1.05) | 1.05 (0.91–1.06) |
| Claude Code | 6 never-run tasks | 1.26 (1.06–1.38) | 1.09 (0.93–1.22) | 1.06 (0.91–1.11) |
| Claude Code | 18 pilot tasks | 1.21 (1.14–1.28) | 1.00 (0.91–1.04) | 1.03 (0.83–1.06) |
| Codex | all 24 tasks | 1.78 (1.68–2.09) | 2.09 (1.85–2.65) | 1.12 (1.06–1.30) |
| Codex | 6 never-run tasks | 2.00 (1.52–3.29) | 1.82 (1.29–2.65) | 1.32 (1.07–2.25) |
| Codex | 18 pilot tasks | 1.72 (1.58–2.05) | 2.31 (1.95–3.02) | 1.12 (0.89–1.27) |

| Agent | Arm | Success (95 % Wilson) | Runs that called CCM |
|---|---|---|---|
| Claude Code | A | 72/72 (95–100 %) | – |
| Claude Code | B | 72/72 (95–100 %) | 0/72 |
| Claude Code | C | 72/72 (95–100 %) | 0/72 |
| Claude Code | D | 72/72 (95–100 %) | 2/72 |
| Codex | A | 66/72 (83–96 %) | – |
| Codex | B | 65/72 (81–95 %) | 72/72 |
| Codex | C | 59/72 (72–89 %) | 72/72 |
| Codex | D | 61/72 (75–91 %) | 65/72 |

For Claude Code, cost, turns and wall time are close to A in every arm: cost 1.00–1.03,
turns 0.95–1.00. Wall time is 0.91 for D and 0.96–1.00 for the others. Codex's wall time is
1.43 for B, 1.50 for C and 1.16 for D. No edit run gave a stale answer in any arm.

## What this shows

On these three small and medium repositories, CCM does not lower token use or raise success for
either agent.

- **Claude Code** practically never called CCM: in no B or C run, and in 2 of 72 D runs. Its
  extra input tokens are the cost of carrying the tool definitions: +24 % for the ten legacy
  tools, about +5 % for the two-tool current product.
- **Codex** used CCM in nearly every CCM-arm run. B and C cost it 1.8–2.1 times A's input
  tokens, the current product 1.12 times. Success was not better: A 92 %, B 90 %, C 82 %, D 85 %,
  with overlapping intervals.
- Of the CCM arms, the current product (D) is the cheapest for both agents.

This agrees with the product pilots. There, the saving appeared on caller questions in large
repositories (Django, Tokio), which the preregistered set does not contain. The final run
therefore neither supports nor tests that result.

Failures are kept in the records and listed by task in each report. Codex's are spread across 10
tasks and occur in every arm. The most frequent are `express-trap-sendfile-helper`, with 9
failures across all arms, and `express-edit-normalize-type`, with 7 across A, C and D.

## Owner audit

`scripts/owner_audit.py` scores nothing. It flags 2 Codex runs that passed but name an owner the
pinned source does not support: `flask-callers-ensure-sync` C#3 (`MethodView.as_view`) and
`flask-trap-load-dotenv` A#3 (`AppGroup.make_context`).
