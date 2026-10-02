# Token-Efficient Answers (M2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax. Tests are fully specified; implementation code is written during execution under TDD. The user asked for minimal token use, so this plan does not duplicate implementation code.

**Goal:** An agent that uses CCM answers code questions with the fewest tokens and tool calls, without losing correctness, at any repository size. The savings are measured and published.

**Architecture:** First, a reproducible LLM-free token benchmark records the current baseline. Then:
- All graph tools switch to a compact one-line-per-result format with a `max_tokens` budget.
- Tools accept `target` as a node ID, `path:line` or a symbol name.
- `explain` returns definition, callers, callees and tests in one call.
- `map` summarises the project by its most-used symbols. It is also available as `ccm-cli map` for session-start hooks.
- `get_context` and `read_graph` are removed, because `explain` replaces them.
- `SKILL.md` and the tool descriptions are made lean.

The benchmark is then re-run and the results published.

**Tech Stack:** Rust (`ccm-core`, `ccm-mcp`, `ccm-cli`); Python 3 + uv for `benchmarks/tokens`, which reuses `benchmarks/freshness/mcp.py`.

**Spec:** The user's goals: minimal usage-limit consumption with CCM, documented; no quality loss; fast and accurate on any project size; the agent knows the project "like the palm of its hand". The design decisions below are the spec.

## Why these levers (how an agent spends tokens)

- Every tool result stays in the context for the rest of the session and is re-read on every later turn.
- Every tool call is one more model turn.

So the levers are:
1. Fewer bytes per answer: compact format and budgets.
2. Fewer calls per question: `target`, `explain`.
3. Cheap orientation: `map`, instead of directory listings and README reads.
4. Smaller fixed overhead: tool definitions are sent with every request, and `SKILL.md` is loaded into context.

Baseline measured on 2026-10-02 (Flask, defaults):

| Item | Bytes |
|---|---|
| `find_usages` | 6,174 |
| `impact_of_change` | 8,624 |
| `find_nodes` | 2,575 |
| `read_graph` | 1,159 |
| `get_context` | 746 (1,834 with body) |
| `tools/list` | 9,333 |
| `SKILL.md` | 25,993 |

## Design decisions

1. **Result line:**
   `- {title} · {path}:{start}-{end} · {reason}`
   - `title` keeps `Kind: name`.
   - No `Score`, no `Node ID`.
   - The path loses its leading `./`.
   - The handle an agent passes back is `path:line`.
2. **Budget:** `max_tokens` (integer, 1..=20000, default 1500) on every list tool. Tokens ≈ characters / 4 (an estimate, documented as one). When results do not fit, the answer ends with `… {n} more not shown (max_tokens={m}); narrow the query or raise max_tokens.` Bodies (`include_body`) count toward the budget and are cut with `…`. The legacy `max_chars` becomes `max_tokens = max_chars / 4` when `max_tokens` is absent.
3. **Targets:** `target` takes one of three forms. `node_id` stays accepted as an alias.
   - **Node ID:** used as given.
   - **`path:line`:** the innermost Function, Method, Class, Struct or Module whose range contains the line; ties go to the smallest span.
   - **Symbol name:** a unique `name` or `Class.member`.
   - **Errors:** unknown → `isError` "no symbol named X"; ambiguous → `isError` listing up to 10 candidates as `path:line kind name`.
4. **`explain {target, include_body=true, max_tokens=1500}`:**
   - **Header:** `{Kind} \`{name}\` · {path}:{range} · {c} callers, {d} callees, {t} tests`.
   - **Body:** at most 50% of the budget.
   - **Then three lists:** `callers:` (usages outside test files), `callees:` (outgoing Calls, CallInferred, CallAmbiguous, References and Inherits, labelled with `UsageRelation`), `tests:` (usages in test files).
   - **Overflow:** each list says `… n more`.
   - **Test files:** the path contains `/tests/` or `/test/`, the file name starts with `test_`, or ends with `_test.*`, `.test.*` or `.spec.*`.
5. **`map {path?, max_tokens=1000}`** is a pure core function `graph::map::project_map(graph, prefix, max_tokens)`. It runs over code files under `prefix`:
   - A symbol's weight is its incoming reference edges from other files (all reference kinds except Contains and Defines).
   - A file's weight is the sum of its symbols' weights.
   - Each line is `{path} — {name}({uses}), …`, listing the top 5 symbols by uses (or the first 5 definitions when nothing is used).
   - Files are ordered by weight descending, then path.
   - **Header:** `{F} files, {S} symbols, {R} cross-file references; most used first.`
   - **Footer:** `… n more files`.
   - The CLI equivalent is `ccm-cli map --path <root> [--prefix <dir>] [--max-tokens N]`.
6. **Tool surface:** remove `get_context` and `read_graph`, because `explain` covers them. Every description is ≤ 220 characters. Target `tools/list` ≤ 6,500 bytes with 10 tools.
7. **`SKILL.md` ≤ 4,500 bytes**, containing:
   - a question → tool table;
   - the workflow `map` once → `explain`/`find_usages` → `impact_of_change` before editing → read only the lines being edited;
   - budgets;
   - the freshness line;
   - a CLAUDE.md snippet;
   - a SessionStart hook example using `ccm-cli map`.
8. **Breaking change:** recorded in README ("0.4.0: `get_context` and `read_graph` were replaced by `explain`; graph tools return compact lines with `max_tokens`").

## Global Constraints

- Cargo always runs as `PATH="$HOME/.cargo/bin:$PATH" cargo …` (pinned 1.99.0).
- Each task ends with these passing:
  - `cargo fmt --all -- --check`
  - `cargo clippy --workspace --all-targets -- -D warnings`
  - the task's tests
  - `cargo test --workspace` at the end
- Rust conventions:
  - Comments in Turkish.
  - No `unwrap`/`expect` in production code.
  - Typed errors.
  - No silent fallback.
- Python (`benchmarks/tokens`):
  - `pyproject` mypy strict (add `tokens` to `files`).
  - ruff.
  - Frozen dataclasses, typed, no `Any`, no default parameters, Turkish comments, imports at top.
- No README number without a results file behind it. Estimates are labelled "estimated tokens = bytes / 4".
- Commits are Conventional, in English, and end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`. The user approved pushing `main` for this repo, but confirm before any push that publishes a release or tag.
- Ledger: `.superpowers/sdd/2026-10-02-token-efficient-answers/progress.md` (resume from it after compaction).

## Review Focus

1. An ambiguous name target (e.g. `save`) → `isError` with candidates, never a silent pick.
2. A `path:line` on a blank line or a module-level statement → the enclosing class or function, or `isError` "no symbol at path:line" if none.
3. A huge fan-in (e.g. `Flask` in Flask) → the budget holds and the footer counts the remainder.
4. `explain` on a symbol with no callers, or on a module-level variable → a valid answer with empty lists.
5. `map` on an empty or prefix-without-code project → an explicit "no indexed code under {prefix}" result.

---

### Task 1: LLM-free token benchmark and baseline

**Files:**
- Create: `benchmarks/tokens/__init__.py`, `benchmarks/tokens/__main__.py`, `benchmarks/tokens/PREREGISTRATION.md`, `benchmarks/tokens/questions.json`
- Modify: `benchmarks/pyproject.toml` (mypy `files`)
- Results: `benchmarks/results/tokens/baseline-fd9af8b.{json,md}`

**PREREGISTRATION.md (commit before running):**
- **Metric:** per question, the number of tool calls and the total response bytes; estimated tokens = bytes / 4. Fixed overhead: `tools/list` bytes and `SKILL.md` bytes.
- **Versions:**
  - v1 is commit `fd9af8b`, with today's debug binaries built from it.
  - v2 is the M2 HEAD.
- **Questions:** per repo (Flask 3.0.3, Django 5.1), all from `questions.json`:
  - `callers(symbol)` for 5 symbols.
  - `explain(symbol)` for the same 5 symbols.
  - `impact(file)` for 2 files.
  - `map` (v2 only).
- **Tool plans:**
  - **v1 `callers`:** `find_nodes(query=symbol, limit=50)`, then `find_usages(node_id)` for every result whose heading name equals the symbol.
  - **v1 `explain`:** the same `find_nodes`, then for every exact match `read_graph(node_id)` and `get_context(file, start_line, include_body=true)`.
  - **v2 `callers`:** `find_usages(target=symbol)`. On an ambiguity error, call `find_usages(target=candidate)` for each listed candidate.
  - **v2 `explain`:** likewise with `explain`.
  - **`impact`:** `impact_of_change(file)` in both versions.
  - All other arguments stay at their defaults.
- **Parity (honesty check):** for `callers`, the share of v1 caller locations (`path:start`) that also appear in the v2 answers. This is reported, not hidden. A lower share means the default budget truncated the answer.
- **Not claimed:** agent success or real model token counts. Those are L3.
- **Index:** graph only (`CCM_DISABLE_EMBEDDER=1`), HOME isolated, `CCM_AUTO_REFRESH=0`.

**questions.json:**
```json
{"schema_version": 1, "repos": {
  "flask": {"symbols": ["url_for", "Flask", "add_url_rule", "dispatch_request", "jsonify"],
            "files": ["src/flask/wrappers.py", "src/flask/helpers.py"]},
  "django": {"symbols": ["reverse", "get_object_or_404", "slugify", "Paginator", "render"],
             "files": ["django/utils/text.py", "django/shortcuts.py"]}
}}
```

**Harness behaviour** (`python -m tokens run --version v1|v2 --bin-dir … --corpus-dir corpus --skill ../SKILL.md --out …`):
1. For each repo, index with `ccm-cli index --path` (HOME isolated).
2. Start `ccm-mcp` over stdio. Reuse `freshness.mcp.McpClient` and `freshness.ccm.PROTOCOL_VERSION`.
3. Record `tools/list` bytes, then run the question plans.
4. Parse v1 headings `## {Kind}: {name} (Score: …)` and `**Node ID:** {id}`. Parse v2 candidates from `isError` texts (`path:line`).
5. Write JSON (schema v1) and a Markdown table per repo and kind: calls and bytes (median, total), est. tokens, parity.

- [ ] Write PREREGISTRATION.md and questions.json; commit (`test(benchmarks): pre-register the token benchmark`).
- [ ] Implement the harness (typed dataclasses `Question`, `CallRecord`, `QuestionResult`, `RunResult`); `uv run --frozen ruff check tokens && uv run --frozen mypy` pass.
- [ ] Run v1 on Flask and Django with `target/debug` built from fd9af8b, before any Rust change. Commit the results (`test(benchmarks): record the token baseline for v0.3.13+M1`).

### Task 2: Compact budgeted output

**Files:** `mcp/src/tools.rs` (`format_suggestions_output`, new `max_tokens_from_args`), `mcp/src/server.rs` (schemas), `mcp/tests/token_budget_e2e_test.rs` (new).

**Test fixture** (written by the test into a tempdir, indexed with `ccm_core::index_directory(path, Some(db))`, and served by spawning `ccm-mcp` with `CCM_ALLOWED_ROOTS`, the same pattern as `hermetic_e2e_test.rs`). The Python fixture is from `core/tests/python_references_test.rs` plus two files:
- `app/hub.py`: `def hub(): return 0` and 30 functions `def caller_NN(): return hub()`.
- `tests/test_core.py`: `from app.core import run` and `def test_run(): assert run() is not None`.

- [ ] **RED:** `usages_are_compact_lines_without_node_ids`. `find_usages {target:"app/models.py:2"}` (`Base.save`) must satisfy all of:
  - the text contains `- Function: save · app/models.py:7-8 · calls`;
  - it does not contain `:symbol:`;
  - it does not contain `Score:`.
- [ ] **RED:** `max_tokens_caps_output_and_reports_the_rest`. `find_usages {target:"hub", max_tokens:200}` → the text is ≤ 1,000 chars and contains `more not shown (max_tokens=200)`.
- [ ] **GREEN:** implement the line format, the budget and the footer (`format_suggestions_output(suggestions, include_body, max_tokens)`), replace `max_chars` with `max_tokens` in the schemas (keeping the legacy alias), and update the `find_usages` summary for the budget.
- [ ] Commit `feat(mcp): compact one-line results with a token budget`.

### Task 3: Flexible targets

**Files:**
- `core/src/graph/mod.rs`: `CodeGraph::symbol_at(file_id, line) -> Option<NodeIndex>`.
- `core/src/engine.rs`: `resolve_target(&self, target) -> Result<String, TargetError>` and `TargetError::{NotFound(String), Ambiguous { target: String, candidates: Vec<String> }}` with `Display`.
- `mcp/src/tools.rs`: `target_from_args(args)`, used by `find_usages` and `trace_call_chain` (`from`/`to` accept targets, keeping the `from_id`/`to_id` aliases).

- [ ] **RED:** `targets_accept_path_line_and_unique_names`. All of:
  - `find_usages {target:"helper"}` → `isError` true, and the text contains `app/util.py:1` and `app/other.py:1`;
  - `find_usages {target:"app/util.py:1"}` → no error;
  - `find_usages {target:"Engine.start"}` → no error;
  - `find_usages {target:"app/core.py:3"}` → the class `Engine` (a blank line inside it resolves to the class).
- [ ] **GREEN**, then commit `feat(mcp): accept path:line and symbol names as targets`.

### Task 4: `explain`

**Files:**
- `core/src/engine.rs`: `explain(&self, node_id) -> Result<Explanation, UsageError>` with `Explanation { node: CodeNode, usages: Vec<Usage>, callees: Vec<Usage> }`; callees reuse `Usage` with relation labels.
- `mcp/src/tools.rs`: `explain`, `format_explanation`.
- `mcp/src/server.rs`: definition and dispatch.

- [ ] **RED:** `explain_returns_definition_callers_callees_and_tests_in_one_call`. `explain {target:"run"}` → no error, and the text contains each of:
  - `` Function `run` · app/core.py:13-17 ``
  - `def run():`
  - `callers:` and `main · app/cli.py`
  - `callees:` and `Class: Engine`
  - `tests:` and `test_run · tests/test_core.py`
- [ ] **RED:** `explain_handles_a_symbol_without_callers`. `explain {target:"app/other.py:5"}` (`start`) → no error, and the text contains `0 callers`.
- [ ] **GREEN**, then commit `feat(mcp): explain a symbol in one call`.

### Task 5: `map`

**Files:**
- `core/src/graph/map.rs` (new, pure): `project_map`.
- `core/src/lib.rs`: `project_map_for(project_path, prefix, max_tokens) -> Result<String>`, which loads the active graph like `run_query`.
- `mcp/src/tools.rs` and `server.rs`: the `map` tool.
- `cli/src/main.rs`: the `Map` subcommand.

- [ ] **RED:** `map_lists_files_by_their_most_used_symbols_within_budget`. `map {max_tokens:300}` → no error, and:
  - the first line contains `files,`;
  - `app/hub.py — hub(` appears before `app/other.py`;
  - the text is ≤ 1,500 chars.
- [ ] **RED:** `map_with_an_unknown_prefix_says_so`. `map {path:"nope"}` → the text contains `no indexed code under nope`.
- [ ] **RED (CLI):** an `assert_cmd` test runs `ccm-cli map --path <fixture> --max-tokens 300`, expects exit 0, and the output contains `app/hub.py`.
- [ ] **GREEN**, then commit `feat: summarise a project by its most-used symbols (map)`.

### Task 6: Lean tool surface and docs

- [ ] **RED:** `tool_list_is_lean`. The `tools/list` JSON is ≤ 6,500 bytes; the names are exactly `index_project`, `index_now`, `search_code`, `find_nodes`, `find_usages`, `trace_call_chain`, `impact_of_change`, `diff_context`, `explain`, `map`; every description is ≤ 220 chars.
- [ ] **GREEN:**
  - Remove `get_context` and `read_graph` (server definitions, dispatch, `tools.rs` functions, annotations).
  - Shorten the descriptions.
  - Update `hermetic_e2e_test.rs` to use `explain` where it called `get_context` or `read_graph`.
- [ ] Rewrite `SKILL.md` (≤ 4,500 bytes, per design decision 7). Update the README tool list and count, add the 0.4.0 breaking-change note, and add the CLAUDE.md snippet and the SessionStart hook example.
- [ ] Commit `feat(mcp)!: replace get_context and read_graph with explain; lean tool list and skill`.

### Task 7: Measure and publish

- [ ] Rebuild the debug binaries at HEAD.
- [ ] Run v2 on Flask and Django.
- [ ] Commit `benchmarks/results/tokens/m2-<sha>.{json,md}`.
- [ ] Add a "Token cost of answers" section to `benchmarks/README.md`: method, the table v1 vs v2 (calls, bytes, est. tokens, parity), and limits (not an agent benchmark).
- [ ] Add one README sentence linking to it, with the measured overall reduction.
- [ ] Full verification (fmt, clippy, workspace tests); commit `docs: publish the token cost of answers`.
- [ ] Final whole-branch review (fresh reviewer, most capable model) and one fix pass. Then push `main` (approved by the user for `main`).
