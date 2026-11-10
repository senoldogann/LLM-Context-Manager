# Large-repository held-out tasks, three repetitions (written before any run)

Date: 2026-11-10, 12:55Z. Harness commit `516ed4c` (PR #16: preregistration amendment 9 and
the two tasks; no product code changes since `9bb6564`). Product binaries: the iteration-5 release
build of `9bb6564` (`~/.ccm-bench/bin-9bb6564`), the same Django index cache, the same arms and notes.

Held out (preregistration amendment 9; no CCM packet for them looked at):
- django-callers-constant-time-compare
- django-trap-urlencode

Three repetitions; Codex A and B, Claude A, B, F and H (F and H reported apart).

Reading rule, fixed now: per agent and task, the saving "holds" if the B arm's mean input tokens
are below the A arm's mean and every B run is below the A mean; a task where it does not hold is
reported as a limit, with its transcripts read only after the results are recorded.
