# Project status

The current state of CCM and the next steps, for the owner and for any agent that continues the
work. Keep it current: replace lines that stop being true instead of appending history (git log is
the history).

Last updated: 2026-10-03. Where work stopped: **search-output pre-study and Codex rg-wrapper
pilot measured; the direction did not save agent tokens and awaits the owner's decision.**
Rewriting the agent's own rg output into structure-aware compact form shrank real search output
(median 0.67 of RTK's tokens offline) but in the valid 6-task Codex pilot the wrapper arm used more
total tokens in all six tasks (median S/A 1.08) and read more files. See "Search output pre-study
and rg-wrapper pilot" below. The owner's thesis email draft (outside the repo) is on hold.

## Search output pre-study and rg-wrapper pilot (2026-10-03)

The owner asked for a direction with real users (RTK-like: in the agent's path, no new tool to
learn) and for real results before fixing thesis goals. Measured with `benchmarks/search/`
(commits `d925d30`–`f5f401a`):

- **Offline replay** (`benchmarks/results/search-prestudy-20261003/report.md`): 1,500 sampled real
  rg/grep commands from the owner's RTK history, 308 reproducible line-shaped ones, o200k tokens.
  CCM compact (definitions with signature, other matches as line numbers under the enclosing
  function) used a median 0.67 of RTK's tokens (p25–p75 0.46–1.00) and showed 44% more match
  locations; a structureless grouping baseline was 1.06, and the text-preserving "full" variant 1.07.
- **Agent pilot** (`benchmarks/results/agent/pilot-codex-search-shim-20261003-v2/report.md`, commit
  `3814a87`): Codex 0.160.0, `gpt-6.1-sol` high, 6 tasks × A/S × 1. All 12 answers correct. Total
  tokens A 293,585, S 334,025; S above A in every task; shell/rg/read commands 22/18/9 versus
  28/21/12. The wrapper compacted 12 calls to 0.15 of their raw bytes, and the agent read whole files
  to recover the hidden code text. Two earlier attempts never invoked the wrapper and are kept as
  failures; attempt 1 doubles as an A/A check (per-task ratios 0.89–1.55).
- **Harness finding:** with Codex's `shell_snapshot` enabled (the L3 default) a PATH-prefixed rg
  wrapper is never invoked; the L3 harness has therefore not verified which rg Codex runs or that
  the temporary HOME isolates the shell environment. The pilot disables the snapshot in both arms.
- **Related tools:** Graft, wonk and duckgrep already offer structure-aware search to agents as
  explicit tools and report agent-level savings with their own methods.

No product code changed. The open decision is the owner's: what the thesis and product should
target given this result (the compaction idea in this form is not supported).

The matched Codex follow-up remains measured at `f649a40`: three tasks × three arms × one
repetition, Codex CLI 0.160.0, `gpt-6.1-sol`, high effort and local embeddings. All nine final
answers were correct. B used 193,473 input+output tokens versus A 157,643 (+22.73%). B's previous
pilot total was 212,498 and B/A overhead +29.65%; one repetition and different agent queries,
retries/cache use prevent attributing the full difference to the product fixes. Actual B Flask
and Express packets now have 12/12 and 5/5 with scopes/receiver filtering; B still reads sources
and only queried CCM before its Serde edit. Neither pilot proves general token savings,
quality equivalence or model independence.

**The retained Serde C freshness failure is now reproduced and covered locally.** The prior
release pair reproduced a fresh-labelled old caller on the first live post-edit context read:
recall=precision=0.500. General RED `41bb558` likewise returned the old caller on the first
saved edit. The fix validates disk sources against the live manifest before a fresh active
read, using existing refresh and the shared two-second budget. The historical pilot's other
subprocess factors were not independently reconstructed. An intermediate uncommitted candidate
had 21 passed/2 failed (fixture prose routing and the targeted failure test's passive-read
assumption); both failures and the final correction are recorded. The final 24/24 auto-refresh
suite covers immediate context/usages reads, ordinary/atomic/same-size saves, creation/deletion,
disabled refresh, honest slow-refresh stale labels, ignores, migration and failure recovery.

Full verification at `fb0ecd4`: core 223 passed/3 ignored, MCP 96 passed/2 ignored (context/token
25/25), CLI 9 passed/1 ignored, no failure; workspace clippy all targets `-D warnings`, fmt and
diff-check exit 0. Isolated structural retrieval 50/50 and offline synthetic retrieval 180/180,
no failed/skipped task; Ruff format/lint, strict mypy 28 files and all 24 task anchors passed.
A single necessary release rebuild completed exit 0 in 3m 49s. Post-build canonical preflight:
Flask 12/12, Express 5/5, Serde 2/2. The opt-in existing preflight `--live-edits` verifies the first
post-edit context response in the same MCP session: Serde 2/2, fresh, 0.338 s, no post-edit
polling or re-index. Release SHA-256: CLI `99446be7…0cdc8e` unchanged, MCP
`72390001…15575` new; full hashes are in the follow-up report. Source corpus pins/worktrees,
earlier measurements, scorer and preregistration remain unchanged. Credentials were not searched,
copied or persisted. The existing macOS debug-link `__eh_frame` warning remains; instrumented
coverage and very large-project read-validation overhead were not measured. The corpus Serde
reference edit was not compiled.

**Claude pilot is waiting for environment authentication.** CLI 2.1.288 official `auth status`
previously reported loggedIn=true/authMethod=claude.ai in the normal application environment;
matching the harness's temporary HOME/CLAUDE_CONFIG_DIR reported loggedIn=false/authMethod=none,
exit 1. `CLAUDE_CODE_OAUTH_TOKEN` was absent from the supervising shell. The owner identified
Claude Code as the last authenticated application and is currently away. Do not extract
credentials from files/keychain, copy the normal configuration or relax HOME isolation.
A token must be provided in the terminal that actually invokes the benchmark; another
terminal's export does not alter the supervising process. No new Claude model/API call or
pilot output was started. The conditional approval covers the same three tasks × three arms × one
repetition in a new directory if authentication works; CLI differs from older pilot 2.1.287.
The completed Codex scope authorizes no further Codex run or final L3 experiment.

The accepted product direction remains one `context` entry point: the agent states what it
needs and CCM routes/composes the packet. Default discovery remains `context + index_project`;
legacy tools are opt-in but directly callable. Schema 11, strict confinement and index/freshness
validation are preserved. No framework-specific rule was introduced. The next local work is
measuring read-validation cost on a larger available pinned project and auditing remaining
source-read/packet-budget overhead in existing B records, without external AI quota use.

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
| Freshness after an edit (L1) | retained Codex stale packet reproduced locally; first-read fix and release check pass at `fb0ecd4` | `benchmarks/results/freshness-followup-20261003/report.md` |
| Token cost of answers (M2) | measured; compact answers cut scripted response bytes 74% | `benchmarks/README.md`, "Token cost of answers" |
| Agent interface | default discovery: `context + index_project`; legacy structural tools remain callable and are discoverable with `CCM_MCP_LEGACY_TOOLS=1` | `mcp/src/server.rs`, `mcp/tests/token_budget_e2e_test.rs` |
| Agent experiment (L3) | two Codex product pilots: all final answers correct, B/C used CCM, B still above A in total tokens; stale CCM packet in follow-up; final L3 pending | `benchmarks/results/agent/pilot-codex-context-first-followup-20261003/audit.md`, `benchmarks/agent/PREREGISTRATION.md` |

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

## Matched Codex follow-up result (2026-10-03)

Measured at `f649a4017bf6f06fc7d3689bee7a0ca81fc947fd` after separate owner approval to continue
the bounded next step. Agent/CLI/model/effort, three tasks, three arms, one repetition, local
embeddings, prompts, harness, scorer and preregistration match the first Codex pilot. Only the
verified product binary pair changed. Settings, report, nine records, exact Serde patches,
comparison values and transcript/packet audit are committed in
`benchmarks/results/agent/pilot-codex-context-first-followup-20261003/`; raw transcripts stay local.

- Harness exit 0; 9/9 final answers recall=precision=1.000, no run failure or stale final item.
  All three edits were applied and their diffs are byte-identical; corpus Rust compilation was
  not part of this pilot. Raw usage/tool/final-text consistency and tool-grant checks passed for
  all nine runs. Source corpus pins and release hashes remained unchanged.
- Input+output tokens (cached input included once):

  | Task | A | B | C | B versus A |
  |---|---:|---:|---:|---:|
  | Flask | 48,828 | 63,320 | 63,405 | +29.68% |
  | Express | 61,494 | 65,838 | 68,326 | +7.06% |
  | Serde | 47,321 | 64,315 | 95,211 | +35.91% |
  | Sum | 157,643 | 193,473 | 226,942 | +22.73% |

- B made one `context` call per task plus 3/3/4 shell calls; C made 6/9/9 CCM calls and 2/4/5
  shell calls. B's Flask/Express packets include the fixed scopes and exact caller sets, but
  source inspection still occurs. Both real B queries chose max_tokens=6000; local preflight used
  1500. Serde B again used CCM only before editing, so its final answer does not test freshness.
- Agent wall sums: A=90.6 s, B=85.2 s, C=112.8 s. Separate pre-index sums: B=76.6 s, C=76.7 s.
  Reusable indexes were not tested. Codex cost/turns remain unknown; report dollars are estimates
  from the unchanged committed API price table, not subscription charges. Dollar flags did not
  cap Codex quota. No additional repetition, model download, embedding API call or Claude run ran.
- B's absolute token total decreased 8.95% from the prior pilot; A also decreased 3.82% and C
  increased 4.93%. One repetition and different agent queries/retries/cache use prevent a causal
  attribution of the full difference to the product fix. The new paired input ratio B/A is 1.31
  (1.07–1.36 over three tasks). B remains above A in total tokens in all three tasks.
- **Retained freshness failure:** Serde C transcript line 29 completes the edit; line 31 confirms
  the actual changed source with successful diff/grep checks; line 32 starts `find_usages` and
  line 33 returns the old caller with `_Index: fresh · auto-refresh on_`. The packet has one
  correct caller, one stale extra and one missing caller: recall=precision=0.500. Correct final
  answers and the report's zero stale-final count do not validate this response. Root cause and
  a deterministic reproduction remain open.
- Recovered errors remain in counts: Flask A's unavailable `python` (exit 127), three AGENTS
  discovery no-match exits, and five explicit legacy MCP target/ambiguity errors. Ruff
  format/lint, strict mypy across 28 files and all 24 task anchors passed after the run set.

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
code hints, with or without backticks, can also route deterministically when indexed declarations
provide enough evidence (a symbol plus source scope, or a JS/TS `receiver.member` plus its definition
file). Proven callable assignment aliases are retained, unrelated explicit targets return an
ambiguity error, and `context` caller lines include qualified/enclosing scopes from Contains edges.
Python project-object/proxy receivers fall back only to bounded `may call` candidates, while
external imports do not. JS/TS callable assignment aliases and named function expressions are now
addressable graph symbols, and receiver-sensitive evidence filters same-name false positives for
lexical caller questions. These graph-semantics changes use index schema 11, so older indexes rebuild
once.

The default `tools/list` serializes to 1,105 bytes in a direct MCP measurement, versus 6,854 bytes
when `CCM_MCP_LEGACY_TOOLS=1` exposes the compatibility surface. This is a fixed-schema measurement,
not evidence of lower end-to-end agent tokens.

Verification on 2026-10-03 at product commit `bcff2e2`: sequential full packages passed
223 core tests (143 unit + 80 integration; 3 opt-in local-model tests ignored), 94 MCP tests
(including 25/25 token-budget/context tests; 2 opt-in tests ignored), and 9 CLI tests
(1 opt-in local-model test ignored). Workspace clippy with all targets and `-D warnings`,
Rust fmt check and diff-check passed. Structural retrieval on an isolated tracked export passed
50/50 against the phase3 baseline; offline synthetic-fixture retrieval passed 180/180, with no
failed or skipped task. Ruff format/lint, strict mypy across 28 files and all 24 task anchors
passed. Cargo/uv ran offline with temporary HOME and cached ONNX Runtime; the existing macOS
large `__eh_frame` debug-link warning remains. Instrumented coverage was not measured.

The release build completed in 3m 49s. SHA-256: `ccm-cli=99446be7…0cdc8e`,
`ccm-mcp=610153e6…948b9f` (full values and commands in the follow-up report).
The existing committed preflight was unchanged: clean pinned source repos, temporary `git archive`
workspaces, copy-only edits, explicit binary paths, embeddings disabled, strict root confinement,
no API/credential access, and exact missing/extra caller failure checks. Post-build canonical
packets passed 12/12, 5/5 and 2/2. Reuse of its helpers for the real-query replay passed the same
sets, plus Flask scope checks. Original corpus and agent benchmark outputs were not changed.

Regression history is preserved as separate RED/GREEN commits: `2374152` compiled and ran the
first routing/scope regressions (exit 101, 6 passed/5 failed), then `6ed2690` passed 11/11;
`b1d7563` compiled and ran the alias boundary regression (exit 101, 0 passed/1 failed), then
`bcff2e2` passed 12/12 context tests. The only export preparation failure was a GNU-only tar flag
unsupported by macOS; the portable retry succeeded before retrieval measurement. Detailed results
are in `benchmarks/results/context-packet-followup-20261003/report.md` and `verification.json`.

The current product is `fb0ecd4`, with the new immediate-read freshness validation and all
local gates/release preflights passing as summarized above. The two Codex pilots establish tool
adoption in their configuration and show earlier packet fixes in real queries. B still costs
more total tokens than A and still reads sources. The freshness fix has no new agent measurement;
CCM is not ready for the preregistered final L3 run. The next local work measures read-validation
cost and audits remaining packet/source-read overhead. Another matched Codex experiment requires
separate quota approval. Reusable indexes and Django/multi-file extensions remain later.

## Next steps, in order

1. **Measure the read-validation cost on a larger available pinned local project, without AI
   quota use.** Keep strict confinement, temporary copies and the existing bounded read budget;
   distinguish fresh no-change reads from first post-edit reads. Do not download a corpus/model
   or add speculative optimization before a measured bottleneck is established.
2. **Audit remaining B token overhead from existing records.** Examine why correct Flask/Express
   packets still led to source reads and max_tokens=6000, retaining prompts/scorer and measured
   failures. Any proposed packet/guidance change needs local quality checks before a new pilot.
3. **Run another matched Codex pilot only with separate explicit quota approval.** Use a fresh
   directory and retain packets/errors alongside final answers and total tokens. The conditional
   Claude pilot can proceed in its approved nine-run scope once the owner supplies environment
   authentication to the actual invoking terminal. The final L3 experiment is not authorized.
4. **Add reusable per-repository indexes**, then large-repository (Django) and multi-file edits.
5. **After the product configuration is stable, run the preregistered L3 final experiment:**
   24 tasks × 3 arms × 3 repetitions per agent, with separate quota approval. Publish negative or
   mixed results and keep the preregistration/failure ledger intact. A Claude run requires its
   approved scope and an owner-supplied token through the invoking environment.
6. Later, only if L3 evidence supports it: JS/TS syntax-level resolution, then L2 edge accuracy.

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
  `uv run mypy freshness tokens agent search scripts/aggregate.py`,
  `uv run python -m agent check --corpus-dir corpus`. The search pre-study also needs
  `cargo build --release --offline --manifest-path search/ccm-outline/Cargo.toml`.
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
