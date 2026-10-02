# Project status

The current state of CCM and the next steps, for the owner and for any agent that continues the
work. Keep it current: replace lines that stop being true instead of appending history (git log is
the history).

Last updated: 2026-10-03. Where work stopped: **local packet fixes passed their GREEN gate** after
the owner asked to continue the next steps. RED checkpoint `2374152` compiled/executed five new
MCP end-to-end regressions at base `dbaa2ec`: exit 101, 6 existing tests passed and 5 new tests
failed for the intended routing/scope bugs. The same target now passes all 11 context tests.
Plain code/path hints are checked against indexed callable assignments, alias declarations are
validated with the existing JS/TS parser, unrelated explicit targets return an ambiguity error,
and `context` caller lines include Contains-based qualified/enclosing scopes while preserving
semantic IDs and relation labels. The graph schema and legacy rendering are unchanged. Next:
review/refine the implementation, then run full local gates and rebuild/verify the changed MCP
release binary against pinned corpus packets.
No new external AI quota use is authorized by this local continuation.

The owner-approved **Codex context-first pilot is complete**, measured at clean commit `4185640`
(see "Codex context-first pilot result"). All nine
final answers passed, and B called `context` once in each task. B still used additional source
inspection and its summed input+output tokens were 29.65% above A across these three tasks.
Real-query inspection found two concrete packet gaps: unqualified/enclosing caller names in
Flask, and receiver false positives/missing evidence when the Express query omitted backticks.
The next work is local regression coverage and general server routing/composition fixes before
another agent experiment. The accepted product direction remains a single `context` entry point:
the agent states what it needs, and CCM chooses graph/search operations and composes the smallest
useful task packet server-side. Default discovery is `context + index_project`; legacy tools are
opt-in but directly callable. The separate `context-first-pilot` profile leaves the preregistered
final design intact.

The existing optimized binaries again passed the **model-free packet quality preflight** before
the Codex pilot: Flask 12/12, Express 5/5 with no extra caller, edited Serde 2/2 with the removed
caller absent; recall=precision=1.000. These canonical preflight queries do not cover every query
an agent can produce. The release pair is unchanged from the full local verification accompanying
`4185640`, and was not rebuilt. Ruff format/lint, strict mypy across 28 files and all 24 task anchors
passed after the pilot. Claude remains unavailable in this session without an owner-supplied
`CLAUDE_CODE_OAUTH_TOKEN`; the owner selected dedicated Codex only. The earlier Codex limit failure
is retained as prior evidence, but this nine-run attempt was accepted and completed. Any further
external AI run or publishing operation requires the owner's separate explicit approval.

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
| Agent experiment (L3) | Claude legacy pilot: no CCM use; Codex context-first pilot: all 9 answers correct, B/C used CCM, no aggregate token saving in this 3-task sample; final L3 pending | `benchmarks/results/agent/pilot-claude/`, `benchmarks/results/agent/pilot-codex-context-first-20261003/`, `benchmarks/agent/PREREGISTRATION.md` |

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

## Codex context-first pilot result (2026-10-03)

Measured at `4185640e67b1d6e80c82e9c27cb9d9ca6b5a36d2` on Codex CLI 0.160.0,
`gpt-6.1-sol`, effort high, dedicated ChatGPT subscription and local embeddings. The owner
approved this bounded quota use and selected Codex only. Results, settings, exact Serde patches,
the generated report and the real-query audit are recorded under
`benchmarks/results/agent/pilot-codex-context-first-20261003/`; full transcripts stay local.

- 3 tasks × 3 arms × 1 repetition completed, harness exit 0. All 9 final answers have
  recall=precision=1.000, no missing/extra/stale caller and no run failure. Each Serde edit was
  applied; the three saved edit diffs are byte-identical. Corpus Rust compilation was not part
  of the agent pilot's edit validation.
- A used built-ins only; B had `context + index_project` and the context note; C had legacy
  full discovery and its navigation note. B made one `context` call per task plus 4/3/4 shell
  calls for Flask/Express/Serde. C used CCM on all three tasks (5/4/7 CCM calls). This proves
  Codex chose CCM in this configuration; it does not isolate the surface effect on Claude.
- Input+output tokens, cached input included once:

  | Task | A | B | C | B versus A |
  |---|---:|---:|---:|---:|
  | Flask | 46,481 | 83,491 | 55,267 | +79.62% |
  | Express | 68,406 | 66,063 | 54,175 | −3.43% |
  | Serde | 49,015 | 62,944 | 106,832 | +28.42% |
  | Sum | 163,902 | 212,498 | 216,274 | +29.65% |

- The generated paired input-token ratios have wide intervals: B/A 1.29 (0.96–1.82), C/A
  1.21 (0.79–2.19). Three tasks and one repetition do not establish general token savings or
  quality equivalence. Codex reports no dollar cost or turn count; the report's cost columns
  are estimates from the committed API price table, not subscription charges. The dollar
  flags did not cap Codex; console `spent $0.00` is unknown cost, not zero quota use.
- Pre-index cost remains separate from agent wall time: B totaled 76.8 s and C 76.4 s, versus
  no pre-index for A. Raw usage, tool counts and final text match all nine saved records;
  isolation/tool-grant checks and manual command inspection passed. No credential file was
  inspected/copied by the supervising agent, and no Claude run, download or publication ran.

**Packet failures are retained despite correct final answers.** Flask's B packet had 12 callers
but omitted enclosing/class-qualified names; Codex used source reads and an AST traversal to
answer the task. Both Flask A/B initially tried unavailable `python` (exit 127), then succeeded
with `python3`; those retries remain in the measurements. Express B's plain-text query named
`req.get`, `req.header`, `app.get`, `res.get` and the definition path **without backticks**. The
server returned four correct callers, three extras (`ip`, `ips`, `subdomains` from
`this.app.get`) and missed `res.location` through `this.req.get`: that packet scored recall 4/5,
precision 4/7. Source inspection confirms the explicit receiver routing accepts backtick hints
only; the canonical preflight's query passes, while this natural agent query fell through to
hybrid retrieval/lexical graph output. The agent corrected its final answer by reading sources.

Serde B called `context` only before its edit, then answered from sources. Its final 2/2 answer
does not test post-edit CCM freshness; C did query `find_usages` after editing and got only
`de::build_generics` and `ser::with_serialize_bound`. The audit records C's unsupported
`path:symbol` attempts and their explicit errors as well. No routing, framework-specific rule,
prompt, scorer or preregistration was changed during this pilot.

The first staged whitespace check flagged the mandatory single-space empty context line in
each archived Serde unified diff. A scoped `.gitattributes` rule disables end-of-line whitespace
checks only for `benchmarks/results/agent/*/diffs/*.diff`; the exact measured patch bytes and
checks on source/docs/JSON are preserved.

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

Verification on 2026-10-03 from base commit `9c3845d`, with the Python maintenance corrections in
this status update: the full `ccm-core` package passed 223 tests (143 unit + 80 integration;
3 opt-in local-model tests ignored). The full `ccm-mcp` package passed 88 tests, including all 19
token-budget/context tests; the local-model and external Guardian tests were ignored as designed.
The remaining workspace package, `ccm-cli`, passed 9 tests with 1 opt-in local-model test ignored;
packages were tested sequentially without repeating the completed core/MCP suites.
`cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check` and
`git diff --check` pass. Retrieval gates on an isolated tracked-file export passed 50/50 structural
tasks against `eval/report.phase3_baseline.json` and 180/180 offline synthetic-fixture tasks, with
no failures or skipped tasks. Ruff format/lint, strict mypy across the 27 harness modules plus
`scripts/aggregate.py`, and all 24 task anchors pass. `ccm-cli` is unchanged at SHA-256
`72e84aea…47090703`; the optimized `ccm-mcp` rebuilt after the caller-render dedupe is
`b44d580e…ea4970a`. `python -m agent preflight-context` verifies the selected corpus repos are
clean and at the commits pinned in `benchmarks/corpus.json`, rejects task paths that escape the repo
root, rebuilds graph-only indexes from isolated temporary workspaces, and checks the three packets
against task ground truth; that exact release pair scores
Flask 12/12, Express 5/5 with no extra caller, and Serde 2/2 after the scripted edit, all at
recall=1.000 and precision=1.000. Duplicate occurrence IDs for one semantic caller are collapsed
only in the `context` rendering; graph call sites remain intact.

The first isolated/offline core attempt exited 101 at link time before running tests: `ort-sys`
could not acquire ONNX Runtime with the temporary HOME and offline mode. Setting `ORT_LIB_PATH`
to the already cached library used by the release build fixed the environment; the full retry
passed without a source change or download. Only CCM package dev-profile artifacts were cleaned
to free 32.0 GiB; release binary hashes stayed unchanged. macOS debug linking reported the large
`__eh_frame` warning; Clippy still passed with `-D warnings`.

The first Python format check found two unformatted committed files, and the lint check found
16 errors in the older `scripts/aggregate.py`. `agent/run.py` received formatting only (its AST is
unchanged); `scripts/aggregate.py` received formatting and explicit types. An intermediate mypy
check found three annotation errors, corrected before the final 28-file pass. Its stdout, stderr
and exit code match the base commit across all five existing comparison-report directories. Task
ground truth, scoring calculations, preregistration and recorded benchmark outputs were unchanged.

The Codex pilot cleared tool adoption for this configuration, but the next gate is packet
sufficiency for real agent queries and measured total-token cost. CCM is not ready for the
preregistered final L3 run: three correct final answers per arm do not establish quality
equivalence, and source inspection still compensates for packet gaps. First cover and fix those
general routing/composition gaps locally. Then, with a fresh approved quota scope, repeat a
matched agent pilot; reusable indexes and Django/multi-file extensions remain later steps.

## Next steps, in order

1. **Add local regressions from the actual Codex queries and fix general packet composition.**
   Cover plain receiver/path hints plus negative receiver mentions without requiring English
   caller prose, and include enough enclosing/class scope to identify callers from one packet.
   Preserve uncertainty labels, semantic-family dedupe, strict root confinement, freshness and
   legacy compatibility. No framework-specific rule or extra default tool is justified.
2. **Revalidate changed code and the optimized release pair** with full local gates and the
   existing pinned-corpus preflight. Rebuild only when changed product code makes it necessary.
3. **Repeat a matched product pilot only after separate explicit approval for new AI quota use.**
   Use a new output directory and compare against this Codex pilot; any Claude follow-up also
   needs the owner to supply `CLAUDE_CODE_OAUTH_TOKEN` through the invoking environment. The
   earlier Codex smoke exited 4 before records; the 2026-10-03 nine-run pilot completed, and
   real transcript isolation/parser inspection is now complete.
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
CODEX_BIN="$HOME/.local/lib/node_modules/@openai/codex/node_modules/@openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/bin/codex"
uv run python -m agent run --agent codex --agent-bin "$CODEX_BIN" \
  --codex-home ~/.ccm-bench/codex-home --auth subscription --profile context-first-pilot \
  --model gpt-6.1-sol --effort high --embedding local --max-budget-usd 2 \
  --max-total-usd 15 --timeout-s 1800 --repetitions 1 \
  --tasks flask-callers-ensure-sync,express-trap-req-get,serde-edit-with-bound \
  --ccm-bin-dir ../target/release --ccm-model-dir ~/.ccm/models --corpus-dir corpus \
  --out-dir results/agent/pilot-codex-context-first
uv run python -m agent report --out-dir results/agent/pilot-codex-context-first \
  --out results/agent/pilot-codex-context-first/report.md
```

The native path above is the active `~/.local/bin/codex` installation on this host.
`npm root -g` currently points to a different Volta installation; resolve the active launcher's
platform dependency before using that command on another installation. Do not reinstall or
change the agent version during a measured run set.

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
  `uv run mypy freshness tokens agent scripts/aggregate.py`,
  `uv run python -m agent check --corpus-dir corpus`.
- `target/` grows past 60 GB; `cargo clean -p ccm-core -p ccm-mcp -p ccm-cli` frees most of it.

## Rules for agents working here

- No unmeasured claims in docs, READMEs or release notes; a measurement names its commit.
- Local commits are fine. Pushes, tags, releases, npm or registry publishing, external AI
  benchmark quota use and any contact with other projects need the owner's separate explicit
  approval each time.
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
