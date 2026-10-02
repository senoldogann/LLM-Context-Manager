# Project status

The current state of CCM and the next steps, for the owner and for any agent that continues the
work. Keep it current: replace lines that stop being true instead of appending history (git log is
the history).

Last updated: 2026-10-02. Where work stopped: the Claude pilot ran (see "Claude pilot result")
and showed that the model ignored CCM while still paying its fixed tool-schema cost. The accepted
product direction is now a single obvious `context` entry point: the agent states what it needs,
and CCM chooses graph/search operations and composes the smallest useful task packet server-side.
The first implementation slice is implemented and verified locally: `context` owns exact-vs-hybrid
routing, default MCP discovery is `context + index_project`, and the legacy surface is opt-in but
still directly callable. The harness has a separate `context-first-pilot` product profile without
changing the preregistered final profile; Ruff, strict mypy, profile assertions and all 24 task
anchors pass. Model-free audits on the three pilot tasks now show that one `context` call contains
the expected answer set for Flask (12/12), the Express receiver trap (5/5, with no `app.get`
false positive), and the edited Serde task (2/2, with the removed caller absent). These are packet
quality checks, not agent-token evidence. A Codex smoke attempt stopped with exit 4 before any run
record because the subscription usage limit was already exhausted; the CLI reported a reset around
2026-10-07 02:17. Claude cannot be rerun from this environment until the owner supplies
`CLAUDE_CODE_OAUTH_TOKEN`. The final product head for this preflight is `5615c30`; the optimized
MCP binary was rebuilt from that source, and the reproducible `preflight-context` command passes
all three pilot packets at recall=1.000 and precision=1.000 without an agent, API call or embedding
model.

**Owner's direction (2026-10-02):** optimize for end-to-end agent token use without sacrificing
answer quality, independent of model/provider/effort. Move retrieval/tool-selection decisions from
the model into CCM where possible, keep claims measurement-backed, and do not expand the public
surface unless an experiment justifies the fixed context cost.

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
| Release | v0.4.0 on npm and the MCP registry; `main` has unreleased work since | `RELEASE_NOTES.md` |
| Python | calls, imports and inheritance resolved from the syntax tree | `core/tests/` |
| Rust | resolved through modules, `use` paths, workspace crates and impl blocks | `benchmarks/README.md`, "Rust edges (M3)" |
| Other languages | matched by name and labelled `calls (inferred …)` or `may call` | `core/tests/lexical_labels_test.rs` |
| Freshness after an edit (L1) | measured | `benchmarks/README.md`, "Level 1" |
| Token cost of answers (M2) | measured; compact answers cut scripted response bytes 74% | `benchmarks/README.md`, "Token cost of answers" |
| Agent interface | default discovery: `context + index_project`; legacy structural tools remain callable and are discoverable with `CCM_MCP_LEGACY_TOOLS=1` | `mcp/src/server.rs`, `mcp/tests/token_budget_e2e_test.rs` |
| Agent experiment (L3) | Claude 3-task pilot complete: B/C used no CCM and paid more input tokens; Codex pilot pending | `benchmarks/results/agent/pilot-claude/`, `benchmarks/agent/PREREGISTRATION.md` |

## What the measurements cover

| Level | Question | Measures | Does not cover |
|---|---|---|---|
| L1 freshness | After an edit, does a tool answer from the new code? | each tool's answers after scripted edits | agent behaviour |
| M2 answer cost | How many tokens does a CCM answer cost? | response bytes and calls per question, before and after M2 | whether an agent reads less overall |
| L3 agents | Does the same agent finish real tasks better or with less quota with CCM? | success, recall and precision, tokens, turns, time, cost (Claude), stale answers after edits; arms A/B/C for Claude Code and Codex | large repositories (Django is not in L3), open-ended feature work and debugging, long sessions, languages other than Python, JavaScript and Rust |
| L2 edge accuracy | Are the graph's edges right against a language server? | not run | — |

L3 has 24 tasks × 3 repetitions per arm and agent: the report gives intervals, and differences
inside them are not evidence either way.

## Claude pilot result (2026-10-02)

`benchmarks/results/agent/pilot-claude/`: 3 tasks × 3 arms, Claude Code with Opus 5.5 at effort
high on a Claude subscription (run records, diffs and report in git; transcripts stay local). The
harness worked on real runs: authentication, isolation checks, scoring and records. What it
showed:

- All 9 runs succeeded. On 17–41 thousand line repositories two or three greps answer these
  tasks, so they do not separate the arms for this model.
- In B and C the agent never called a CCM tool, although the tools and the note were there; it
  used Grep, Read and Bash every time.
- Unused CCM still cost quota: B used 25%, 24% and 104% more input tokens (cache included) than A
  on the three tasks (32,082 against 25,595; 43,953 against 35,515; 70,952 against 34,790). Part
  of it is about 6,800 tokens of CCM tool definitions and the note added to every session.
- Indexing serde for one B or C run took 114 s (release build, built-in embedding model).

As configured, CCM is overhead on small repositories and the agent does not reach for it. That is
the first problem to solve.

## First: one obvious entry point

The pilot says the first product problem is not another ranking tweak: the model did not choose CCM
at all, while merely exposing the tools cost input tokens. The working design is therefore:

```text
context({ query: "what I need to understand or change", max_tokens?: 1500 })
```

CCM owns the routing. Exact symbols/paths go to structural explanation; natural-language tasks go
through hybrid retrieval and the best result is enriched with graph context in the same server
call. The legacy tools remain callable during migration, but they are primitives/diagnostics rather
than the workflow the model must learn.

The first slice is now implemented. Exact targets go directly to graph explanation; natural-language
requests use hybrid retrieval and enrich the best graph node in the same call. Explicit scoped
code hints can also route deterministically when the query gives enough evidence (for example a
backtick `ensure_sync` or `module::symbol` plus a source scope, or a JS/TS `receiver.member`
plus its definition file). Python project-object/proxy receivers fall back only to bounded `may call` candidates, while
external imports do not. JS/TS callable assignment aliases and named function expressions are now
addressable graph symbols, and receiver-sensitive evidence filters same-name false positives for
lexical caller questions. These graph-semantics changes use index schema 11, so older indexes rebuild
once.

The default `tools/list` serializes to 1,105 bytes in a direct MCP measurement, versus 6,854 bytes
when `CCM_MCP_LEGACY_TOOLS=1` exposes the compatibility surface. This is a fixed-schema measurement,
not evidence of lower end-to-end agent tokens.

Verification through product commit `5615c30`: the full `ccm-core` package passed, including 143
unit tests (3 opt-in local-model tests ignored) and all integration suites; the full `ccm-mcp`
package passed, including 19 token-budget/context tests, 22 auto-refresh tests, 26 MCP integration
tests (1 opt-in local-model test ignored), 5 protocol tests and the hermetic E2E test (external
Guardian fixture ignored as designed). `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo fmt --all -- --check` and `git diff --check` also pass. `ccm-cli` is unchanged at SHA-256
`72e84aea…47090703`; the optimized `ccm-mcp` rebuilt after the caller-render dedupe is
`37be6d90…71c094d`. `python -m agent preflight-context` rebuilds graph-only indexes from the pinned
corpus and verifies the three packets against task ground truth; that exact release pair scores
Flask 12/12, Express 5/5 with no extra caller, and Serde 2/2 after the scripted edit, all at
recall=1.000 and precision=1.000. Duplicate occurrence IDs for one semantic caller are collapsed
only in the `context` rendering; graph call sites remain intact.

The next gate is behavioral: re-run the same Claude pilot. CCM is not ready for the preregistered
final L3 run until the agent actually chooses `context` where it helps, with no success regression
and a lower or defensible total-token cost. After that, add reusable indexes and test Django and
multi-file edits where grep/read loops are expensive.

## Next steps, in order

1. **Re-run the 3-task Claude pilot** into a new output directory once
   `CLAUDE_CODE_OAUTH_TOKEN` is supplied. Run `preflight-context` first, then compare success,
   CCM calls, input/output tokens, turns and wall time with `pilot-claude`; inspect transcripts to
   see whether and why the model selects `context`.
2. **Run the Codex pilot after the subscription limit resets.** The dedicated
   `~/.ccm-bench/codex-home` login exists; the 2026-10-02 smoke attempt stopped before a run record
   with exit 4, so the first successful transcript still needs isolation/parser inspection.
3. **If agents use `context` but task packets are insufficient, improve server-side routing or
   composition from transcript evidence**, not by adding default tools back speculatively.
4. **Add reusable per-repository indexes**, then run large-repository (Django) and multi-file edit
   extensions. These are the cases most likely to separate CCM from grep/read loops.
5. **Only after the product configuration is stable, run the preregistered L3 final experiment:**
   24 tasks × 3 arms × 3 repetitions per agent. Publish negative or mixed results and keep the
   preregistration/failure ledger intact.
6. Later, only if L3 evidence supports it: TypeScript/JavaScript syntax-level resolution, then edge
   accuracy against language servers (L2).

## Running the L3 experiment

Prerequisites: the corpus (`benchmarks/scripts/fetch_corpus.sh`), release binaries
(`cargo build --release -p ccm-cli -p ccm-mcp`, about 20 minutes) and the built-in embedding model
in `~/.ccm/models` (downloaded by the first `ccm-cli` index).

Before spending any agent quota, verify the exact release binaries against the three pilot task
packets. This preflight disables embeddings and makes no model/API call:

```bash
cd benchmarks
uv run python -m agent preflight-context --ccm-bin-dir ../target/release --corpus-dir corpus
```

Claude Code, on the Claude subscription:

```bash
claude setup-token                      # once; prints a token tied to the Claude subscription
export CLAUDE_CODE_OAUTH_TOKEN=...      # in the terminal that runs the benchmark only
cd benchmarks
uv run python -m agent check --corpus-dir corpus
uv run python -m agent run --agent claude --agent-bin ~/.local/bin/claude --auth subscription \
  --profile context-first-pilot \
  --model claude-opus-5-5 --effort high --embedding local --max-budget-usd 2 \
  --max-total-usd 15 --timeout-s 1800 --repetitions 1 \
  --tasks flask-callers-ensure-sync,express-trap-req-get,serde-edit-with-bound \
  --ccm-bin-dir ../target/release --ccm-model-dir ~/.ccm/models --corpus-dir corpus \
  --out-dir results/agent/pilot-claude-context-first
uv run python -m agent report --out-dir results/agent/pilot-claude-context-first \
  --out results/agent/pilot-claude-context-first/report.md
```

Codex, on the ChatGPT subscription (pass the native binary; the npm wrapper needs Node on `PATH`):

```bash
mkdir -p ~/.ccm-bench/codex-home && CODEX_HOME=~/.ccm-bench/codex-home codex login   # once
cd benchmarks
CODEX_BIN="$(npm root -g)/@openai/codex/node_modules/@openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/bin/codex"
uv run python -m agent run --agent codex --agent-bin "$CODEX_BIN" \
  --codex-home ~/.ccm-bench/codex-home --auth subscription --profile context-first-pilot \
  --model gpt-6.1-sol --effort high --embedding local --max-budget-usd 2 \
  --max-total-usd 15 --timeout-s 1800 --repetitions 1 \
  --tasks flask-callers-ensure-sync,express-trap-req-get,serde-edit-with-bound \
  --ccm-bin-dir ../target/release --ccm-model-dir ~/.ccm/models --corpus-dir corpus \
  --out-dir results/agent/pilot-codex-context-first
```

Use the newest model each agent offers when a run set starts (`~/.codex/models_cache.json` lists
Codex's) and keep it for the whole set. The budget flags do not limit Codex, which reports no
cost; its usage limit stops the run instead. `--profile context-first-pilot` is only for the
follow-up product pilots. For the preregistered final run, omit that flag (the default profile is
`preregistered`), use `--tasks all --repetitions 3`, a new `--out-dir` and a total cap that
fits the plan. Exit codes: 3 total cap reached, 4 usage or
rate limit (rerun later), 5 isolation check failed (fix before continuing), 6 authentication
failed (export a new token; give it only through `read -s`, never on a command line). A measured run refuses
to start with uncommitted changes under `benchmarks/agent`. `--embedding openai` needs
`OPENAI_API_KEY` in the same terminal.

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
  `uv run mypy`, `uv run python -m agent check --corpus-dir corpus`.
- `target/` grows past 60 GB; `cargo clean -p ccm-core -p ccm-mcp -p ccm-cli` frees most of it.

## Rules for agents working here

- No unmeasured claims in docs, READMEs or release notes; a measurement names its commit.
- Local commits are fine. The owner approved pushing `main`; tags, releases, npm or registry
  publishing and any contact with other projects need the owner's explicit approval each time.
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
