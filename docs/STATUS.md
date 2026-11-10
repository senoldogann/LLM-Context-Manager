# Project status

The current state of CCM and the next steps, for the owner and for any agent that continues the
work. Keep it current: replace lines that stop being true instead of appending history (git log and
`benchmarks/results/` are the history).

Last updated: 2026-11-10. Where work stopped: **three measured product iterations on branch
`feat/context-complete-signal`; Codex now saves tokens with CCM on the large repository and on
single-lookup tasks, not yet on the held-out set as a whole; Claude Code still never chooses CCM on
its own.** Across the three iterations the effect follows repository size: on Django (155 k lines)
CCM cut input tokens in every Codex run (B/A 0.30–0.80) and in Claude's forced runs (F/A 0.49,
0.54 in iteration 3); on the 17–41 k-line repositories and on edit tasks it is neutral or more
expensive. All checks pass on PR #12.

## Goal

CCM is a code graph served over MCP, so that coding agents answer structural questions (who calls
X, what breaks if Y changes, where is Z) with compact, deterministic answers instead of
grep-and-read loops. The owner's goals:

1. agents use less of their usage quota without losing answer quality, on projects of any size;
2. prove or disprove that with a reproducible agent experiment. The owner plans to use the result
   in a thesis on what CCM changes for development work and for usage quotas.

Positioning: an engine that stays correct while code is edited and answers structural questions
deterministically, not "better search". Adding languages and the self-improving policy research
are parked. The owner's strategy notes are `docs/productization-plan.md`,
`docs/product-assessment.md` and `docs/agent-brief.md` (local, not in git).

## Where things stand

| Area | State | Evidence |
|---|---|---|
| Release | v0.5.0 on npm and the MCP registry; the branch has unreleased fixes, including one for the npm launcher of 0.5.0 (MCP mode overrode the client's project roots) | `RELEASE_NOTES.md`, `npm/bin/ccm.js` |
| Python | calls, imports and inheritance resolved from the syntax tree | `core/tests/` |
| Rust | resolved through modules, `use` paths, workspace crates and impl blocks | `benchmarks/README.md`, "Rust edges (M3)" |
| Other languages | matched by name and labelled `calls (inferred …)` or `may call`; JS/TS members defined by assignment get receiver-aware evidence | `core/tests/lexical_labels_test.rs`, `mcp/tests/token_budget_e2e_test.rs` |
| Freshness after an edit (L1) | first-read validation since `fb0ecd4`; a failed refresh now retries with backoff, `context` waits up to 10 s for a pending refresh | `mcp/tests/auto_refresh_test.rs` |
| Agent interface | default discovery `context + index_project` (1,134-byte `tools/list`, 266 o200k tokens, plus 65-token server instructions); legacy tools callable, discoverable with `CCM_MCP_LEGACY_TOOLS=1` | `mcp/src/server.rs` |
| Agent experiment (L3) | product pilots on 2026-11-10 with tuned and held-out tasks, both agents; final preregistered L3 not run | "Agent pilots" below |

## What `context` returns

One call with a symbol, `path:line`, `receiver.member` or a question. For a function or method the
packet starts with a **grep-equivalent check**: the server reads the indexed files, classifies every
textual match of the target (call, other use, import, external module, string or docstring, call
resolved to another definition with the same name, recursion, Rust method of another type) and says
either "call list complete" or which lines lie outside the listed callers. Each caller carries its
call-site lines (at most 2, then `+n more`). The target body is capped at 40 lines and callees at
10. Python dotted module paths (`django.utils.html.escape`) and unique code-like names route without
embeddings. The check covers indexed non-test files; calls that never name the target in source
(getattr, runtime aliases) are out of reach of both text and graph, and the packet says so.
Implementation: `mcp/src/call_sites.rs`, `context` in `mcp/src/tools.rs`.

An independent code review after iteration 3 found ways the check could claim completeness wrongly
(calls inside f-strings and template literals or after a `#`/`//` inside a string read as text;
`exports.f = function` targets lost their real callers; aliased imports and lines the graph
attributes to another same-named definition did not soften the headline; a weak stale-line guard).
The fixes make the check stricter and are tested (unit and end-to-end, release preflight packets
unchanged), but they came after the iteration-3 runs and have no agent measurement yet.

## What the measurements cover

| Level | Question | Measures | Does not cover |
|---|---|---|---|
| L1 freshness | After an edit, does a tool answer from the new code? | each tool's answers after scripted edits | agent behaviour |
| M2 answer cost | How many tokens does a CCM answer cost? | response bytes and calls per question, before and after M2 | whether an agent reads less overall |
| L3 agents | Does the same agent finish real tasks better or with less quota with CCM? | success, recall and precision, tokens, turns, time, cost (Claude), stale answers after edits; arms A/B/C (and the forced-use arm F in product pilots) for Claude Code and Codex | open-ended feature work and debugging, long sessions, languages other than Python, JavaScript and Rust |
| L2 edge accuracy | Are the graph's edges right against a language server? | not run | — |

L3 has 24 tasks × 3 repetitions per arm and agent: the report gives intervals, and differences
inside them are not evidence either way.

## Agent pilots

All product pilots: `--profile context-first-pilot`, one repetition, subscriptions, local
embeddings. A = built-in tools; B = `context` + `index_project` + a short note (natural use);
C = legacy full surface; F = B plus an instruction to call `context` first (forced use, reported
apart). Ratios are input tokens including cache, per task (arm mean over repetitions / A), median
over tasks with a fixed-seed 95% bootstrap interval. Three or four tasks per cell: intervals are
wide and one repetition cannot separate small effects from run-to-run noise.

| Set (commit) | Agent | Tasks | Answers correct | B/A median (interval) | per task | F/A |
|---|---|---|---|---|---|---|
| 2026-11-09 (`be48997`) | Codex `gpt-6-luna` | 3 tuned | 9/9 | 1.11 (0.98–1.14) | | |
| 2026-11-09 (`be48997`) | Claude `opus-5-5` | 3 tuned | 9/9 | 0.96 (0.77–1.02), B never called CCM | | |
| iteration 1 held-out (`b6d055d`) | Codex | 3 held-out | 9/9 | **0.89 (0.71–1.01)** | load-dotenv 0.71, expr-is-missing 0.89, res-set 1.01 | |
| iteration 1 held-out (`b6d055d`) | Claude | 3 held-out | 12/12 | 1.04 (1.03–1.05), B never called CCM | | 0.67 (0.60–1.53) |
| iteration 1 tuned (`b6d055d`) | Codex | 3 of 4 (Django stopped) | 10/10 | 1.20 (0.64–1.21) | ensure_sync 0.64, req-get 1.21, with-bound 1.20 | |
| iteration 1 tuned (`b6d055d`) | Claude | 3 of 4 (Django stopped) | 12/12 | 1.05 (0.80–1.69), B never called CCM | | 1.42 (0.50–1.45) |
| iteration 2 tuned (`c0228ee`) | Codex | 4 tuned | 8/8 | **0.69 (0.30–0.87)** | django escape 0.30, req-get 0.55, with-bound 0.83, ensure_sync 0.87 | |
| iteration 2 held-out (`c0228ee`) | Codex | 4 held-out | 6/8 | 1.28 (0.80–1.49) | django make_aware 0.80, deserialize-struct 1.16, sendfile 1.40, error-handler 1.49 | |
| iteration 2 tuned (`c0228ee`) | Claude | 4 tuned | 12/12 | 1.07 (0.61–1.28), B never called CCM | | 1.00 (0.72–1.47) |
| iteration 2 held-out (`c0228ee`) | Claude | 4 held-out | 12/12 | 1.10 (1.05–2.42), B never called CCM | | 1.00 (0.74–1.42) |
| iteration 3 held-out (`85252fb`) | Codex | 4 held-out | 7/8 | 1.07 (0.53–1.42) | django get_random_string 0.53, effective-style 1.01, debug-flag 1.13, res-send 1.42 | |
| iteration 3 tuned (`85252fb`) | Codex | 4 tuned | 7/8 | 1.01 (0.51–1.73) | django make_aware 0.51, deserialize-struct 0.64, error-handler 1.38, sendfile 1.73 | |
| iteration 3 held-out (`85252fb`) | Claude | 4 held-out | 11/12 | 1.19 (1.04–1.35), B never called CCM | | 1.17 (0.49–1.55): get_random_string 0.49, res-send 0.86, effective-style 1.48, debug-flag 1.55 |
| iteration 3 tuned (`85252fb`) | Claude | 4 tuned | 12/12 | 1.09 (0.59–1.48), B never called CCM | | 1.08 (0.54–1.53): make_aware 0.54, error-handler 0.64, sendfile 1.52, deserialize-struct 1.53 |

Results, reports, run records and task selections are in `benchmarks/results/agent/`
(`heldout-20261110/`, `pilot-*-20261110/`, `iter2-20261110/`, `iter3-20261110/`); transcripts stay
local. Failures kept in the records:

- Iteration-1 tuned sets stopped at the Django task: the frozen harness rebuilt the index with local
  embeddings in every run (about an hour per Django run under load) and the session's 2-hour limit
  stopped it (`INCOMPLETE.md`). Since `c0228ee` the harness builds each repository's index once
  (Django: 1,349 s) and copies it into every run.
- The disk filled up during iteration 1; one Claude held-out run crashed before its agent started
  and was resumed from the frozen commit. No measured run was lost.
- `express-trap-sendfile-helper` failed in both Codex arms the same way: the agent read "not the
  methods res.sendFile or res.sendfile" as excluding the callers; the packet was correct.
- Iteration 3: Codex B missed `res.render` in `express-trap-res-send`: the call is `self.send(str)`
  through `var self = this`, which receiver evidence does not follow (the packet says aliases are
  not checked; 44 `examples/` call sites outside `lib/` buried that line). Both Claude A and B missed
  the async `_aget_new_session_key` in `django-callers-get-random-string`; F found it.
- One iteration-1 graph-only `context` query about `res.set` was seen by the supervising agent while
  the held-out runs were already running (disclosed in `heldout-20261110/SELECTION.md`).

What the transcripts show:

- **Codex chooses CCM on its own** in every B run that asks a structural question and, since the
  grep-equivalent check, often stops after one `context` call (Flask `ensure_sync`, Express
  `req.get`, Serde `expr_is_missing`, Django `make_aware`). The large-repository case is where it
  pays most: Django `escape` A read 259,771 input tokens (one `rg` returned 146 KB), B 78,475.
  Iteration 3's Rust rule turned `deserialize_struct` from 1.16 into a one-call 0.64.
- **Codex losses have generic causes, fixed in iteration 3 unless noted:** a stale first answer right
  after an edit (2 s freshness budget), Rust method calls of other types counted as missing call
  sites, very long bodies and callee lists in the packet, and a misread question (not a CCM issue).
- **Claude Code never called CCM in natural use** (0 of 25 B runs on 2026-11-09 and 2026-11-10),
  with the tool loaded, a usage note, short server instructions and `anthropic/alwaysLoad`. Its
  Grep tool caps output, so its baseline is cheaper than Codex's. When forced (F), it always
  re-checks the answer with one or more Greps; F saved tokens on some tasks and cost more on others.

## Next steps, in order

1. **Make CCM the path Claude Code takes without being told** (proposal, not built): an opt-in hook
   installed by `npx @senoldogann/context-manager install` that, for a Grep whose pattern is a
   symbol the index knows, answers with the `context` packet instead of running the search
   (PreToolUse deny with the packet as the reason), so Claude's verification grep is replaced rather
   than added. Evidence: 0/25 natural B runs; F runs re-grep on small repositories, but on Django
   they stopped after one `context` call (F/A 0.49, 0.54). Measure as its own arm (H)
   next to unchanged A/B, tuned and held-out.
2. **Reduce the remaining re-checks and misses**: follow `var self = this` aliases in JS receiver
   evidence (the iteration-3 `res.render` miss); keep the check's "outside" list to the question's
   scope so examples do not bury real gaps; qualify Rust trait-path calls (`Type::method(`); and make
   edit tasks cheaper (Codex B still costs 1.13–1.49 on Flask edits).
3. **Repetitions**: three repetitions of the held-out sets per agent before any claim beyond "pilot".
4. Release the fixes (npm launcher roots bug, refresh retry) as 0.5.1 after owner approval.
5. After the product configuration is stable, the preregistered L3 final experiment (24 tasks × 3
   arms × 3 repetitions per agent) with separate quota approval.

## Running the L3 experiment

Prerequisites: the corpus (`bash benchmarks/scripts/fetch_corpus.sh`), release binaries
(`cargo build --release -p ccm-cli -p ccm-mcp`, 20–45 minutes; use a separate
`CARGO_TARGET_DIR` if a pilot is running from `target/release`) and the built-in embedding model
(`~/.ccm-bench/models` on this host). Verify the binaries against the pilot packets first; this
disables embeddings and makes no model call:

```bash
cd benchmarks
uv run python -m agent preflight-context --ccm-bin-dir <bin dir> --corpus-dir <corpus>
```

Claude Code (subscription token from `claude setup-token`, only in the invoking environment) and
Codex (`CODEX_HOME=~/.ccm-bench/codex-home codex login` once; pass the native binary):

```bash
cd benchmarks
CLAUDE_CODE_OAUTH_TOKEN=... uv run python -m agent run --agent claude \
  --agent-bin "$(command -v claude)" --auth subscription --profile context-first-pilot \
  --model claude-opus-5-5 --effort high --embedding local --max-budget-usd 2 --max-total-usd 40 \
  --timeout-s 1800 --repetitions 1 --tasks <ids> --arms A,B,F \
  --ccm-bin-dir <bin dir> --ccm-model-dir ~/.ccm-bench/models --corpus-dir <corpus> \
  --index-cache-dir ~/.ccm-bench/index-cache --out-dir results/agent/<name>/claude
CODEX_BIN="$(dirname "$(readlink -f "$(command -v codex)")")/../node_modules/@openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/bin/codex"
uv run python -m agent run --agent codex --agent-bin "$CODEX_BIN" \
  --codex-home ~/.ccm-bench/codex-home --auth subscription --profile context-first-pilot \
  --model gpt-6-luna --effort high --embedding local --max-budget-usd 2 --max-total-usd 40 \
  --timeout-s 1800 --repetitions 1 --tasks <ids> --arms A,B \
  --ccm-bin-dir <bin dir> --ccm-model-dir ~/.ccm-bench/models --corpus-dir <corpus> \
  --index-cache-dir ~/.ccm-bench/index-cache --out-dir results/agent/<name>/codex
uv run python -m agent report --out-dir results/agent/<name>/codex --out results/agent/<name>/codex/report.md
```

Long sets outlive a 2-hour supervising session: start the run detached (`nohup … &`) or pre-build a
large repository's index cache before the run. Exit codes are listed in `run --help` (3 total cap,
4 usage or rate limit, 5 isolation, 6 authentication, 7 setup failure; nothing is recorded for
4–7). A measured run refuses to start with uncommitted changes under `benchmarks/agent`, and one
out-dir accepts one harness process at a time. For the preregistered final run omit `--profile`,
use `--tasks all --repetitions 3` and a new out-dir.

## Verifying changes

- Rust: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace --no-fail-fast` (the suite takes several minutes; run it in the
  background).
- Retrieval gates: copy the tracked files to a scratch directory, index it with the debug
  `ccm-cli` and `CCM_DISABLE_EMBEDDER=1`, then run `ccm-cli eval` against
  `eval/golden_tasks.v3.ccm.json` without the `search_code` tasks (baseline
  `eval/report.phase3_baseline.json`) and against `eval/fixtures/golden_tasks.synthetic.json` with
  `CCM_EMBEDDING_FIXTURE=eval/fixtures/embeddings.ndjson`. Both must report no failed tasks.
- Benchmarks (Python): in `benchmarks/`, `uv run ruff format --check`, `uv run ruff check`,
  `uv run mypy freshness tokens agent search scripts/aggregate.py`,
  `uv run python -m agent check --corpus-dir corpus`. The search pre-study also needs
  `cargo build --release --offline --manifest-path search/ccm-outline/Cargo.toml`.
- `target/` grows past 60 GB and the disk on this host is small (it filled up once during the
  2026-11-10 pilots); `cargo clean -p ccm-core -p ccm-mcp -p ccm-cli` frees most of it, and a
  pilot's release binaries can live in a separate `CARGO_TARGET_DIR` so builds do not replace them.

## Rules for agents working here

- No unmeasured claims in docs, READMEs or release notes; a measurement names its commit.
- Local commits are fine. Pushes, tags, releases, npm or registry publishing, external AI
  benchmark quota use and any contact with other projects need the owner's explicit approval
  for that piece of work (the 2026-11-10 task approved branch + PR and its pilots).
- Product pilots keep a held-out task set: tasks whose CCM packets nobody looked at before the
  run, chosen and written down (`SELECTION.md`) before it starts, reported apart from tuned tasks.
  A held-out task becomes tuned as soon as its transcript is read.
- Never read or log `~/.ccm/.env` or other credential files. Credentials reach the harness only
  through environment variables the owner sets.
- Do not copy code from competing tools into this repository (GitNexus is PolyForm
  Noncommercial). The competitor adapters live on the unmerged `bench/ccm-bench` branch.
- Code comments are Turkish; docs and commit messages are English.

## Known limits and open items

Rust resolution misses edges, but adds no wrong ones, for: Rust 2015 crate-relative paths,
`#[path]` modules, `include!` and macro-generated items, renamed dependencies (`extern crate x as
y`) and custom `[[bin]]`/`[lib]` paths. A workspace member named like an external crate captures
that crate's paths. Method calls on values of unknown type stay `may call` with at most five
candidates (`.clone()` and other std trait methods are noisy), and every impl block references its
own type.

Deferred, not yet fixed:

- `Self { .. }` and `Self(..)` expressions produce no edge.
- Enum and trait nodes are not embedded, get no symbol ranking bonus and are not counted as
  symbols, so `search_code` cannot return them.
- A comment in `core/src/vector/hybrid.rs` says ranking is unchanged; Python `calls (inferred)`
  weights moved from 0.60 to 1.00 and ambiguous mentions are now dropped, unmeasured.
