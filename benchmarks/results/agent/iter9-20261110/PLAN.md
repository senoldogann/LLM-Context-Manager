# Iteration 9: macro-defined Tokio targets (written before any run)

Date: 2026-11-10, 16:39Z. Product and harness commit `31502d6` (main `ee82565` with #21 and #22, plus
preregistration amendment 13). Release binaries built from `31502d6`: ccm-cli
5ea5c4bd122b…, ccm-mcp 601fc2c0bae7….

Held out (amendment 13; no CCM packet for them looked at): tokio-callers-with-scheduler,
tokio-callers-poll-proceed. Tuned: tokio-callers-async-op (iteration 8: Codex B/A 2.28).
Three repetitions; Codex A and B, Claude A, B, F and H (F and H apart).

Reading rule, fixed now (as for iterations 6 to 8): per agent and task, the saving holds if the B
arm's mean input tokens are below the A arm's mean and every B run is below the A mean.
