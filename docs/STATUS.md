# Project status

The current state of CCM and the next steps, for the owner and for any agent that continues the
work. Keep it current: replace lines that stop being true instead of appending history (git log is
the history).

Last updated: 2026-10-02. Where work stopped: the Claude pilot ran (see "Claude pilot result"):
the harness works, but the agent never used CCM and CCM's tools made sessions cost more. The next
agent starts with "First: make CCM worth reaching for". The Codex pilot waits for the owner's
ChatGPT limit to reset.

**Owner's direction (2026-10-02):** keep developing CCM until it is visibly usage-friendly and
indispensable in real systems; try every approach that might get there, and measure each change
before claiming it.

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
| Token cost of answers (M2) | measured | `benchmarks/README.md`, "Token cost of answers" |
| Agent experiment (L3) | harness ready for Claude Code and Codex, checked without spending; not run | `benchmarks/agent/PREREGISTRATION.md` |

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

## First: make CCM worth reaching for

Measure each change against `pilot-claude` by re-running the same 9-run pilot into a new
`--out-dir` and comparing input tokens, CCM calls and success.

1. Cut the fixed cost: measure the tokens CCM adds to every session (tool list, descriptions,
   schemas, note) and shrink them: fewer tools by default (`map`, `explain`, `find_usages`,
   `impact_of_change`), shorter descriptions and schemas.
2. Give the agent a reason to choose CCM: tool descriptions that say when CCM beats grep (common
   names, cross-file impact, large repositories, answers after edits), then check what the agent
   does with them.
3. Test where grep gets expensive: a Django pilot (500 thousand lines) and multi-file changes.
   Add index reuse first; one index per run already takes 114 s on serde.
4. Keep the preregistered design for the final run, and run it only after B uses CCM where CCM
   should help; record every product change with its commit.

## Next steps, in order

1. **L3 pilots** (the owner starts them; they use the subscriptions): 3 tasks × 3 arms × 1
   repetition per agent, to check the harness on real runs and measure what one run uses. Pilot
   results are not reported as the L3 result.
   - Claude Code: the owner starts it in a terminal where `CLAUDE_CODE_OAUTH_TOKEN` (from `claude
     setup-token`) is exported; the token never enters the repository or a chat. The measured
     subscription is the account that created the exported token, not necessarily the account
     running the agent session.
   - Codex: the dedicated login exists (`~/.ccm-bench/codex-home`, ChatGPT, done 2026-10-02), so an
     agent session can start the Codex pilot itself once the owner's ChatGPT limit has reset.
   - Model: Claude Opus 5.5 and Codex `gpt-6.1-sol` for the pilots; confirm the final-run models
     and the total budget with the owner after the pilots.
   - More than one Claude account can share the work: run `claude setup-token` while signed in to
     the account (the browser's claude.ai login decides which one), export that token and rerun
     the same command with the same `--out-dir`; completed runs are skipped, and a usage-limit
     stop (exit 4) is where the next account takes over. Records do not name the account.
2. **L3 final run:** 24 tasks × 3 arms × 3 repetitions = 216 runs, sequential, about 8–18 hours
   unattended. A usage-limit stop is expected on a subscription; rerun the same command after the
   limit resets.
3. **L3 report:** `python -m agent report` per run set, then a "Level 3: agents with and without
   CCM" section in `benchmarks/README.md` with the run's commit, the arm tables, a failure ledger
   written from the transcripts and a data-based proposal for the README headline. Raw records stay
   in `benchmarks/results/agent/<run>/`. Publish negative or mixed results too.
4. **Codex results:** the Codex agent is implemented (`benchmarks/agent/codex.py`, configuration
   in the preregistration). Its zero-cost start without a login accepted every flag; the event
   parsing and the run-level isolation check have not seen real output yet, so read the first
   pilot transcripts before the final Codex run. The dedicated `CODEX_HOME` keeps the owner's
   `~/.codex` login, memories, skills and global AGENTS.md out of the runs; never link or copy
   `~/.codex/auth.json` (a token refresh could break the owner's login).
5. **L3 extensions, after the core runs** (the owner asked to record them for later; each runs
   the same three arms on both agents). The owner's original goal is quota savings without losing
   quality on projects of any size during real development, and the core 24 tasks cover only
   small and medium repositories (17–41 thousand lines) and short tasks:
   - Large repository: Django 5.1 (2,899 files, about 500 thousand lines, already in
     `corpus.json`), about 8 tasks. Indexing it for every B/C run is too slow: add a reusable
     per-repository index (restore a prebuilt workspace and index at a fixed path) first.
   - Development tasks: about 6 multi-file changes (change a signature and update every caller,
     rename, move a function, fix a bug), judged by deterministic checks and, where they run
     offline, the repository's own tests.
   - TypeScript: one popular repository; CCM matches names only there today, so the result also
     shows whether TS syntax-level resolution is worth building.
   - Real-use diary for the owner's thesis: the owner's own tasks, alternating with and without
     CCM, with a small script that sums tokens from Claude Code and Codex session logs.
   - Codex quota share: Codex keeps 5-hour and weekly `rate_limits` (`used_percent`) in its
     internal token events, not in `exec --json`; after the Codex pilot, decide whether to keep
     session rollouts to record the quota share each run used.
6. Later, only if the L3 result supports it: TypeScript/JavaScript syntax-level resolution, then
   edge accuracy against language servers (L2).

## Running the L3 experiment

Prerequisites: the corpus (`benchmarks/scripts/fetch_corpus.sh`), release binaries
(`cargo build --release -p ccm-cli -p ccm-mcp`, about 20 minutes) and the built-in embedding model
in `~/.ccm/models` (downloaded by the first `ccm-cli` index).

Claude Code, on the Claude subscription:

```bash
claude setup-token                      # once; prints a token tied to the Claude subscription
export CLAUDE_CODE_OAUTH_TOKEN=...      # in the terminal that runs the benchmark only
cd benchmarks
uv run python -m agent check --corpus-dir corpus
uv run python -m agent run --agent claude --agent-bin ~/.local/bin/claude --auth subscription \
  --model claude-opus-5-5 --effort high --embedding local --max-budget-usd 2 \
  --max-total-usd 15 --timeout-s 1800 --repetitions 1 \
  --tasks flask-callers-ensure-sync,express-trap-req-get,serde-edit-with-bound \
  --ccm-bin-dir ../target/release --ccm-model-dir ~/.ccm/models --corpus-dir corpus \
  --out-dir results/agent/pilot-claude
uv run python -m agent report --out-dir results/agent/pilot-claude \
  --out results/agent/pilot-claude/report.md
```

Codex, on the ChatGPT subscription (pass the native binary; the npm wrapper needs Node on `PATH`):

```bash
mkdir -p ~/.ccm-bench/codex-home && CODEX_HOME=~/.ccm-bench/codex-home codex login   # once
cd benchmarks
CODEX_BIN="$(npm root -g)/@openai/codex/node_modules/@openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/bin/codex"
uv run python -m agent run --agent codex --agent-bin "$CODEX_BIN" \
  --codex-home ~/.ccm-bench/codex-home --auth subscription --model gpt-6.1-sol --effort high \
  --embedding local --max-budget-usd 2 --max-total-usd 15 --timeout-s 1800 --repetitions 1 \
  --tasks flask-callers-ensure-sync,express-trap-req-get,serde-edit-with-bound \
  --ccm-bin-dir ../target/release --ccm-model-dir ~/.ccm/models --corpus-dir corpus \
  --out-dir results/agent/pilot-codex
```

Use the newest model each agent offers when a run set starts (`~/.codex/models_cache.json` lists
Codex's) and keep it for the whole set. The budget flags do not limit Codex, which reports no
cost; its usage limit stops the run instead. For a final run use `--tasks all --repetitions 3`, a
new `--out-dir` and a total cap that fits the plan. Exit codes: 3 total cap reached, 4 usage or
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
