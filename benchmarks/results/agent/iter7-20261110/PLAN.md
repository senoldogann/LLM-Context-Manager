# Iteration 7: a second large repository and a re-run of the urlencode trap (written before any run)

Date: 2026-11-10, 13:49Z. Product and harness commit `e805025` (main `f3149fd` with #17, plus
preregistration amendment 10). Release binaries built from `e805025`: ccm-cli
a86cb4695fa8…, ccm-mcp cd1cfefdd912….

Held out (amendment 10; no CCM packet for them looked at): tokio-callers-sleep-until,
tokio-trap-catch-unwind. Tuned: django-trap-urlencode (the task #17 was made for).
Three repetitions; Codex A and B, Claude A, B, F and H (F and H reported apart).

Reading rule, fixed now (as for iteration 6): per agent and task, the saving holds if the B arm's
mean input tokens are below the A arm's mean and every B run is below the A mean. The urlencode
re-run is reported as tuned and compared with its iteration-6 result.
