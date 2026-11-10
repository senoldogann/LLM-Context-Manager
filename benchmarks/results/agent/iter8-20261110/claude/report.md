# L3 agent benchmark

Scope: pilot design; complete (36/36 runs).
Product profile: `context-first-pilot`. Arms: A=built-ins; B=default context+index_project with context note; C=legacy full surface with legacy navigation note; F=B plus an instruction to call `context` first (forced use, not natural use); H=B plus the opt-in Claude Code Grep hook (installed product configuration, not natural use).

Agent `claude` (2.1.319 (Claude Code), auth `subscription`), model `claude-opus-5-5` at effort `high`, embeddings `local`, per-run budget $2.0; ccm-cli 0.5.0 at `4c5f193`.

Cost is the amount the agent reported.

## By arm

| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost | Median input tokens | Median output tokens | Median turns | Median agent wall | Median index copy (prebuilt index) | Runs using CCM | Stale (edit runs) | Run failures |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| A | 8/9 (89%) | 56%–98% | 1.00 | 0.90 | $0.112 | $1.03 | 44,694 | 1,832 | 4 | 20 s | — | 0/9 | 0/0 | 0/9 |
| B | 8/9 (89%) | 56%–98% | 1.00 | 0.90 | $0.102 | $0.99 | 44,016 | 1,676 | 5 | 20 s | 0.1 s | 2/9 | 0/0 | 0/9 |
| F | 9/9 (100%) | 70%–100% | 1.00 | 0.97 | $0.096 | $0.88 | 40,819 | 1,533 | 4 | 19 s | 0.1 s | 9/9 | 0/0 | 0/9 |
| H | 9/9 (100%) | 70%–100% | 1.00 | 0.97 | $0.097 | $0.83 | 37,732 | 1,670 | 4 | 18 s | 0.1 s | 3/9 | 0/0 | 0/9 |

## Prebuilt indexes

Arms with CCM copy an index built once per repository before their runs (the cache is shared across result directories); the arm table shows the copy time. One-time build time of the index each repository's runs copied:

| Repository | Build |
|---|---|
| tokio | 239.8 s |

## Paired against A

Per task, the mean over repetitions in each arm, then the ratio to arm A; the table gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 means less than A. A task is omitted from a metric ratio if any recorded repetition in either compared arm has that metric unknown.

| Metric | B / A | F / A | H / A |
|---|---|---|---|
| Cost | 1.00 (0.86–1.02, 3 tasks) | 0.85 (0.84–0.87, 3 tasks) | 0.84 (0.75–0.90, 3 tasks) |
| Input tokens (incl. cache) | 1.02 (0.98–1.17, 3 tasks) | 0.93 (0.91–1.01, 3 tasks) | 0.86 (0.65–1.12, 3 tasks) |
| Output tokens | 1.02 (0.95–1.04, 3 tasks) | 0.86 (0.79–0.87, 3 tasks) | 0.88 (0.70–0.91, 3 tasks) |
| Turns | 1.08 (1.06–1.09, 3 tasks) | 1.00 (0.94–1.00, 3 tasks) | 1.00 (0.67–1.00, 3 tasks) |
| Wall time | 0.97 (0.95–1.06, 3 tasks) | 0.88 (0.85–1.09, 3 tasks) | 0.90 (0.70–0.92, 3 tasks) |

## Input tokens by task

Mean input tokens (cache included) over repetitions, and the ratio to A.

| Task | A | B | F | H | B/A | F/A | H/A |
|---|---|---|---|---|---|---|---|
| `tokio-callers-async-op` | 44,590 | 45,531 | 44,935 | 38,286 | 1.02 | 1.01 | 0.86 |
| `tokio-trap-catch-unwind` | 30,241 | 35,395 | 28,206 | 33,825 | 1.17 | 0.93 | 1.12 |
| `tokio-trap-yield-now` | 71,612 | 70,421 | 64,956 | 46,384 | 0.98 | 0.91 | 0.65 |

## Success by task category

| Category | A | B | F | H |
|---|---|---|---|---|
| callers | 3/3 | 3/3 | 3/3 | 3/3 |
| trap | 5/6 | 5/6 | 6/6 | 6/6 |

## By task

| Task | A | B | F | H | Cost A | Cost B | Cost F | Cost H |
|---|---|---|---|---|---|---|---|---|
| `tokio-callers-async-op` | 3/3 | 3/3 | 3/3 | 3/3 | $0.112 | $0.096 | $0.097 | $0.095 |
| `tokio-trap-catch-unwind` | 3/3 | 3/3 | 3/3 | 3/3 | $0.069 | $0.069 | $0.058 | $0.062 |
| `tokio-trap-yield-now` | 2/3 | 2/3 | 3/3 | 3/3 | $0.160 | $0.163 | $0.136 | $0.121 |

## Hook decisions (arm H)

Greps answered with the `context` packet (deny) and greps let through (pass), per run.

| Task | Run | Deny | Pass |
|---|---|---|---|
| `tokio-callers-async-op` | 1 | 0 | 1 |
| `tokio-callers-async-op` | 2 | 0 | 1 |
| `tokio-callers-async-op` | 3 | 0 | 1 |
| `tokio-trap-catch-unwind` | 1 | 0 | 2 |
| `tokio-trap-catch-unwind` | 2 | 1 | 1 |
| `tokio-trap-catch-unwind` | 3 | 0 | 1 |
| `tokio-trap-yield-now` | 1 | 0 | 1 |
| `tokio-trap-yield-now` | 2 | 0 | 1 |
| `tokio-trap-yield-now` | 3 | 0 | 1 |

## Failed runs

- `tokio-trap-yield-now` A#1: extra tokio/src/signal/registry.rs:smoke, tokio/src/runtime/tests/loom_multi_thread.rs:only_blocking_inner, tokio/src/runtime/tests/loom_multi_thread.rs:blocking_and_regular_inner, tokio/src/runtime/tests/loom_multi_thread.rs:complete_block_on_under_load, tokio/src/runtime/tests/loom_multi_thread_alt.rs:only_blocking_inner, tokio/src/runtime/tests/loom_multi_thread_alt.rs:blocking_and_regular_inner, tokio/src/runtime/tests/loom_multi_thread_alt.rs:complete_block_on_under_load, tokio/src/runtime/tests/loom_multi_thread/yield_now.rs:yield_calls_park_before_scheduling_again, tokio/src/runtime/tests/loom_multi_thread_alt/yield_now.rs:yield_calls_park_before_scheduling_again, tokio/src/runtime/tests/loom_current_thread/yield_now.rs:yield_calls_park_before_scheduling_again, tokio/src/runtime/tests/loom_current_thread.rs:assert_at_most_num_polls, tokio/src/runtime/tests/loom_current_thread.rs:assert_no_unnecessary_polls, tokio/src/runtime/tests/task.rs:schedule, tokio/src/runtime/tests/task.rs:shutdown, tokio/src/runtime/tests/task.rs:shutdown_immediately, tokio/src/runtime/tests/task.rs:spawn_during_shutdown, tokio/src/runtime/tests/task_combinations.rs:test_combination, tokio/src/runtime/tests/loom_local.rs:wake_during_shutdown, tokio/src/sync/tests/loom_mpsc.rs:try_recv
- `tokio-trap-yield-now` B#2: extra tokio/src/signal/registry.rs:smoke, tokio/src/sync/tests/loom_mpsc.rs:try_recv, tokio/src/runtime/tests/task.rs:schedule, tokio/src/runtime/tests/task.rs:shutdown, tokio/src/runtime/tests/task.rs:shutdown_immediately, tokio/src/runtime/tests/task.rs:spawn_during_shutdown, tokio/src/runtime/tests/task_combinations.rs:test_combination, tokio/src/runtime/tests/loom_local.rs:wake_during_shutdown, tokio/src/runtime/tests/loom_current_thread.rs:assert_at_most_num_polls, tokio/src/runtime/tests/loom_current_thread.rs:assert_no_unnecessary_polls, tokio/src/runtime/tests/loom_current_thread/yield_now.rs:yield_calls_park_before_scheduling_again, tokio/src/runtime/tests/loom_multi_thread/yield_now.rs:yield_calls_park_before_scheduling_again, tokio/src/runtime/tests/loom_multi_thread_alt/yield_now.rs:yield_calls_park_before_scheduling_again, tokio/src/runtime/tests/loom_multi_thread.rs:only_blocking_inner, tokio/src/runtime/tests/loom_multi_thread.rs:blocking_and_regular_inner, tokio/src/runtime/tests/loom_multi_thread.rs:complete_block_on_under_load, tokio/src/runtime/tests/loom_multi_thread_alt.rs:only_blocking_inner, tokio/src/runtime/tests/loom_multi_thread_alt.rs:blocking_and_regular_inner, tokio/src/runtime/tests/loom_multi_thread_alt.rs:complete_block_on_under_load
