# Iteration 8: measuring #19 on new Tokio held-out tasks (written before any run)

Date: 2026-11-10, 14:55Z. Product and harness commit `4c5f193` (main `3ff38b2` with #19 and #18, plus
preregistration amendment 11). Release binaries built from `4c5f193`: ccm-cli
8d855682c522…, ccm-mcp 9351f45d0876….

Held out (amendment 11; no CCM packet for them looked at): tokio-callers-async-op,
tokio-trap-yield-now. Tuned: tokio-trap-catch-unwind (the task #19 was made from), compared with
its iteration-7 result. Three repetitions; Codex A and B, Claude A, B, F and H (F and H apart).

Reading rule, fixed now (as for iterations 6 and 7): per agent and task, the saving holds if the B
arm's mean input tokens are below the A arm's mean and every B run is below the A mean.
