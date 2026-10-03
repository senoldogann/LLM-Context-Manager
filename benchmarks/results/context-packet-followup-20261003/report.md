# Local context packet follow-up

Measured on 2026-10-03 at clean product commit
`bcff2e2c1171033bf2ef2ea4738b8920e3c3fa55`, following the packet gaps recorded in the
[Codex pilot audit](../agent/pilot-codex-context-first-20261003/audit.md). This is a
**model-free packet quality follow-up**. No external agent, embedding API or model download
was used. The existing pilot records, scorer, task ground truth and preregistration are unchanged.

Exact queries, returned packets, corpus pins, caller scores and scope checks are in
[packets.json](packets.json). Gate totals and RED/GREEN failures are in
[verification.json](verification.json).

## General fixes and regression evidence

- Plain and backtick code/path hints are matched against indexed declarations. JS/TS targets
  require assignment AST evidence; negative receiver mentions do not win merely by appearing
  first. Unrelated explicit callable targets return an ambiguity error.
- Proven assignment aliases remain available in receiver evidence. AST call inspection excludes
  comments, quoted strings and identifier-prefix lookalikes. Receiver evidence retains its
  uncertainty label; it is not full runtime receiver/type resolution.
- `context` caller lines include qualified scopes and the outermost enclosing callable from
  the graph's Contains chain. Different semantic families stay separate; branch alternatives
  within one family remain deduplicated. Legacy rendering and graph schema 11 are unchanged.
- RED `2374152`: the first five new end-to-end regressions compiled and failed for the intended
  routing/scope gaps (exit 101; 6 existing tests passed, 5 failed). GREEN `6ed2690`: 11/11 passed.
- RED `b1d7563`: the assignment-alias boundary regression compiled and failed because the alias
  caller was missing (exit 101; 0 passed, 1 failed). GREEN `bcff2e2`: 12/12 context tests passed.
  The full token-budget/context suite later passed 25/25.

No framework-specific routing rule, filesystem bypass or default tool was added. Workspaces
were temporary `git archive` copies with a temporary HOME, embeddings disabled, explicit
binary paths and the existing strict `CCM_ALLOWED_ROOTS` confinement. Source corpus repositories
remained clean and pinned. Existing benchmark output directories were not overwritten.

## Optimized binary and packet results

Product code changed, so the release pair was rebuilt once, successfully in 3m 49s:

```text
ccm-cli 99446be7c123f91dc6b7653bb80c3f90a4f488e2fc36942f1f2622e7a40cdc8e
ccm-mcp 610153e60d7418f4a70efba208a9d1d2a8ce5758f8a7386100a27a5490948b9f
```

The existing committed preflight command was run after that build, with no harness change:

```bash
UV_OFFLINE=1 uv run python -m agent preflight-context \
  --ccm-bin-dir ../target/release --corpus-dir corpus
```

| Packet | Canonical preflight | Actual pilot query replay |
|---|---|---|
| Flask | 12/12, no missing/extra caller | 12/12; all class-qualified caller lines present; `copy_current_request_context.wrapper` and `View.as_view.view` include their enclosing callable |
| Express | 5/5, no missing/extra caller | Plain query: 5/5; `location` present; `ip`, `ips`, `subdomains`, `app.get` and `res.get` excluded |
| Edited Serde | 2/2, no stale caller | Canonical post-edit query: `de::build_generics` and `ser::with_serialize_bound`; removed `ser::build_generics` absent |

All caller sets have recall=precision=1.000. The additional replay reused functions from
`agent.context_preflight` for workspace preparation, indexing, MCP confinement and scoring;
it did not add another preflight implementation. Each saved packet used one `context` call
with `max_tokens=1500`. The Serde edit was applied only to a copy, followed by graph indexing;
this does not measure an agent's post-edit tool choice or live watcher latency.

## Local gates

| Gate | Result |
|---|---|
| Full `ccm-core` package | 223 passed, 0 failed, 3 opt-in local-model tests ignored |
| Full `ccm-mcp` package | 94 passed, 0 failed, 2 opt-in tests ignored |
| Full `ccm-cli` package | 9 passed, 0 failed, 1 opt-in local-model test ignored |
| Workspace clippy, all targets, `-D warnings` | Exit 0 |
| `cargo fmt --all -- --check` | Exit 0 |
| `git diff --check` | Exit 0 |
| Structural retrieval, tracked export, phase3 baseline | 50/50; 0 failed/skipped; minimum pass rate 100%, maximum regression 0 |
| Offline synthetic fixture retrieval | 180/180; 0 failed/skipped; minimum pass rate 100% |
| Ruff format/lint | Both passed |
| Strict mypy | 28 files passed |
| Agent task anchors | 24 tasks passed |

Rust packages were run sequentially without repeating the completed package suites. Cargo/uv
were offline; ONNX Runtime came from the already cached library. Debug linking still emitted
the existing macOS large `__eh_frame` warning. An initial tracked-export command used the
GNU-only tar `--one-top-level` flag and exited 1; the portable retry succeeded before retrieval
measurement. No final product gate failed. Instrumented code coverage was not measured.

## Remaining limits and next step

These results prove the recorded packet gaps are covered by local regression tests and the
new optimized binary's packets. They do not establish lower total agent tokens, agent quality
equivalence, universal query coverage or model independence. The original pilot's mixed token
results remain valid for its measured commit. Serde B's pre-edit-only CCM use remains an agent
behavior gap.

The next concrete measurement is a matched Codex follow-up: the same three tasks, three arms,
one repetition and recorded model/effort, with this verified release pair and a fresh output
directory. It requires separate explicit owner approval for new subscription quota use. No
follow-up AI run, push, tag, release or publication was started by this local step.
