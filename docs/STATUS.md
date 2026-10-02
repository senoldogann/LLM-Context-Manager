# Project status

The current state of CCM and the next steps, for the owner and for any agent that continues the
work. Keep it current: replace lines that stop being true instead of appending history (git log is
the history).

Last updated: 2026-10-02.

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
| Agent experiment (L3) | harness ready and checked without spending; not run | `benchmarks/agent/PREREGISTRATION.md` |

## Next steps, in order

1. **L3 pilot** (the owner starts it; it uses the subscription): 3 tasks × 3 arms × 1 repetition,
   to check the harness on real runs and measure the cost of one run. Pilot results are not
   reported as the L3 result.
2. **L3 final run:** 24 tasks × 3 arms × 3 repetitions = 216 runs, sequential, about 8–18 hours
   unattended. A usage-limit stop is expected on a subscription; rerun the same command after the
   limit resets.
3. **L3 report:** `python -m agent report`, a failure ledger written from the transcripts, then a
   data-based proposal for the README headline. Publish negative or mixed results too.
4. **Codex arm** (ChatGPT subscription): not started. Use a dedicated `CODEX_HOME` logged in once
   with `CODEX_HOME=<dir> codex login`, never a link or copy of `~/.codex/auth.json` (a token
   refresh could break the owner's login). Run `codex exec --json --ephemeral
   --ignore-user-config --ignore-rules`, disable the features that would leak into runs (memories,
   apps, browser and computer use, hooks; see `codex features list`), and verify every run's
   tools and instructions as the Claude path does. Add it to the preregistration before running.
5. Later, only if the L3 result supports it: TypeScript/JavaScript syntax-level resolution, then
   edge accuracy against language servers (L2).

## Running the L3 experiment

Prerequisites: the corpus (`benchmarks/scripts/fetch_corpus.sh`), release binaries
(`cargo build --release -p ccm-cli -p ccm-mcp`, about 20 minutes) and the built-in embedding model
in `~/.ccm/models` (downloaded by the first `ccm-cli` index).

```bash
claude setup-token                      # once; prints a token tied to the Claude subscription
export CLAUDE_CODE_OAUTH_TOKEN=...      # in the terminal that runs the benchmark only
cd benchmarks
uv run python -m agent check --corpus-dir corpus
uv run python -m agent run --auth subscription --model claude-opus-5-5 --effort high \
  --embedding local --max-budget-usd 2 --max-total-usd 15 --timeout-s 1800 --repetitions 1 \
  --tasks flask-callers-ensure-sync,express-trap-req-get,serde-edit-with-bound \
  --claude-bin ~/.local/bin/claude --ccm-bin-dir ../target/release \
  --ccm-model-dir ~/.ccm/models --corpus-dir corpus --out-dir results/agent/pilot
uv run python -m agent report --out-dir results/agent/pilot --out results/agent/pilot/report.md
```

For the final run use `--tasks all --repetitions 3`, a new `--out-dir` and a total cap that fits
the plan. Exit codes: 3 total cap reached, 4 usage or rate limit (rerun later), 5 isolation check
failed (fix before continuing). A measured run refuses to start with uncommitted changes under
`benchmarks/agent`. `--embedding openai` needs `OPENAI_API_KEY` in the same terminal.

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
