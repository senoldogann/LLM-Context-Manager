# Immediate post-edit freshness follow-up

Measured on 2026-10-03 at clean product/harness commit
`fb0ecd4a504201dd998154dcec5ddcbd7d80e278`. The owner asked to continue local steps while away
from the computer. This is a **model-free packet quality and freshness check**. No new Codex or
Claude run, subscription quota use, model download or embedding API call occurred.

[verification.json](verification.json) records commits, test totals, failures, corpus pins,
binary hashes, command outputs and hashes of the local full logs. Earlier agent measurements,
their failures, scorer, task ground truth and preregistration are unchanged.

## Reproduction and general fix

The [matched Codex audit](../agent/pilot-codex-context-first-followup-20261003/audit.md) retained
a Serde C response that returned the old caller after the edit and labelled it fresh. The
general MCP regression at `41bb558` reproduced this on its first save: `find_usages` returned
`caller_before`, although `caller_after_0` was already on disk. The intended RED result was
exit 101, 0 passed/1 failed; its exact failure is in [regression-red.txt](regression-red.txt).

Previously, retrieval waited only for refresh work already reported by the filesystem watcher.
A saved change could therefore precede event delivery while the refresh queue still appeared
empty. Active auto-refresh now compares disk sources with the live manifest before a fresh
read. A difference schedules the existing refresh path. Validation and refresh share the
existing two-second read budget; unfinished work remains stale, and disk-validation errors are
explicit. Unchanged files use the existing stat/hash cache. Disabled auto-refresh retains
manual semantics. Strict root confinement, generation/index validation, schema 11 and legacy
tool compatibility are preserved; no framework-specific rule or timing workaround was added.

The integration suite passes **24/24**. Its new caller regression performs 12 immediate edits
through alternating `find_usages` and `context`, using ordinary and atomic saves and same-size
rewrites. Another regression queries immediately after file creation and removal. There is no
post-edit polling in these regressions. Existing budget/stale-label, ignore/artifact exclusion,
migration, semantic-upgrade, concurrent generation and failure-recovery checks also pass.

An intermediate uncommitted candidate ran 21 passed/2 failed. The fixture's prose query did not
route to its generic symbol, so the freshness regression now uses an exact path:line target.
The existing targeted failure test assumed reads were passive; it now verifies all three
injected failures in the server log before reading. That candidate diff was not archived, and
these failures are retained as intermediate failures, not attributed to a clean GREEN commit.
The final full workspace run at `fb0ecd4` passes.

## Pinned live corpus and optimized binaries

The existing `preflight-context` command gained an opt-in `--live-edits` check. It indexes the
unedited temporary copy, opens MCP, verifies the exact pre-edit caller set and fresh label,
applies the existing reference edit, then checks the **first** post-edit `context` response.
No post-edit sleep, polling or re-index occurs. The default preflight remains unchanged.

The previous verified release pair from `bcff2e2` reproduced the Serde failure with this check:
first read 0.001 s, recall=precision=0.500, old `ser::build_generics` extra and new
`ser::with_serialize_bound` missing. See [prior-release-live-preflight.txt](prior-release-live-preflight.txt).
This establishes a local event-delivery admission gap; it does not independently reconstruct
every factor in the historical agent subprocess. The fixed debug pair passed 2/2, first read
0.388 s.

Product code changed, so exactly one release build was necessary:

```bash
PATH="$HOME/.cargo/bin:$PATH" cargo build --release -p ccm-cli -p ccm-mcp
```

It exited **0**, `Finished release profile [optimized] target(s) in 3m 49s`. New SHA-256 values:

```text
ccm-cli 99446be7c123f91dc6b7653bb80c3f90a4f488e2fc36942f1f2622e7a40cdc8e
ccm-mcp 723900012c13d991f8b48f831b14b882b53f84415a3a1e948a3b3f7a30215575
```

The CLI hash is unchanged; the new read admission path is used by MCP. The MCP hash differs
from the prior `610153e6…948b9f` pair. Post-build commands selected `target/release` explicitly:

```bash
cd benchmarks
UV_OFFLINE=1 uv run python -m agent preflight-context \
  --ccm-bin-dir ../target/release --corpus-dir corpus
UV_OFFLINE=1 uv run python -m agent preflight-context \
  --ccm-bin-dir ../target/release --corpus-dir corpus --live-edits
```

| Check | Result |
|---|---|
| Canonical Flask | 12/12, recall=precision=1.000, no missing/extra/duplicate caller |
| Canonical Express | 5/5, recall=precision=1.000, no receiver extras |
| Canonical edited Serde | 2/2, recall=precision=1.000, no stale caller |
| First live post-edit Serde context | 2/2, recall=precision=1.000, fresh label, 0.338 s |

The live packet contains only `de::build_generics` and `ser::with_serialize_bound`, excluding
the old `ser::build_generics`. Exact command outputs are in [release-preflight.txt](release-preflight.txt)
and [release-live-preflight.txt](release-live-preflight.txt). Timings describe these single
local reads; they are not a large-project latency benchmark.

The source repositories were verified clean at their manifest pins before and after the checks:

| Corpus | Commit |
|---|---|
| Flask | `c12a5d874c5a014495eb2db8a73f40037bc813ac` |
| Express | `04bc62787be974874bc1467b23606c36bc9779ba` |
| Serde | `49d098debdf8b5c38bfb6868f455c6ce542c422c` |

All corpus edits and indexes stayed in temporary `git archive` copies. HOME was temporary,
embeddings were disabled and MCP required `CCM_ALLOWED_ROOTS` confinement. Credentials were not
searched, copied or persisted; earlier benchmark outputs were preserved. The corpus Serde edit
was not compiled.

## Final verification gates

| Gate at `fb0ecd4` | Result |
|---|---|
| `cargo test --workspace --no-fail-fast` | Exit 0 |
| `ccm-core` | 223 passed, 0 failed, 3 opt-in local-model tests ignored |
| `ccm-mcp` | 96 passed, 0 failed, 2 opt-in tests ignored |
| MCP context/token suite | 25/25 |
| `ccm-cli` | 9 passed, 0 failed, 1 opt-in local-model test ignored |
| `cargo clippy --workspace --all-targets -- -D warnings` | Exit 0 |
| `cargo fmt --all -- --check` | Exit 0 |
| `git diff --check` | Exit 0 |
| Structural retrieval on an isolated tracked export | 50/50; 0 failed/skipped; phase3 baseline, 100% minimum, 0 regression |
| Offline synthetic retrieval | 180/180; 0 failed/skipped; committed embedding fixture |
| Ruff format/lint | Exit 0; 28 files formatted |
| Strict mypy | 28 files passed |
| Agent task anchors | 24 tasks passed |

Cargo and uv were offline; ONNX Runtime came from the existing cached library. Long Rust gates
and the release build were sequential. The existing macOS debug-linker large `__eh_frame`
warning remains. Instrumented coverage was not measured.

## Remaining work

The latest real Codex pilot remains measured at `f649a40`: all 9 final answers correct, B total
193,473 input+output tokens versus A 157,643 (+22.73%). The earlier B total was 212,498 and its
B/A overhead +29.65%; one repetition and changed agent queries/retries/cache use do not establish
a causal or general token-saving claim. This local fix has no new end-to-end agent measurement.

The next local step is to measure the added read-validation cost on a larger available pinned
project and audit remaining B source reads/packet budgets from existing records. Full manifest
validation visits indexed source files on active reads; very large-project cost is unmeasured.
A further matched Codex run requires separate explicit quota approval. The conditionally
approved Claude pilot awaits an owner-supplied environment token in the actual invoking
terminal. No final L3 experiment, push, tag, release or publication was started.
