# L3 Agent Benchmark Preregistration

Date: 2026-10-02

## Question

Does access to CCM let the same coding agent answer real repository questions more
successfully and/or at lower model cost, while avoiding stale answers after edits?

This is an agent-level benchmark. Retrieval Recall@K, synthetic regression gates and tool
response byte counts are useful diagnostics, but they are not substitutes for this result.

## Fixed corpus

The benchmark uses the pinned real repositories in `benchmarks/corpus.json`:

- Flask 3.0.3
- Express 4.19.2
- serde 1.0.219

The benchmark does not use synthetic repositories as value evidence.

## Tasks

`agent/tasks.py` defines 24 manually checked tasks. Nine are edit tasks: the agent must
change the repository first and then answer a question whose correct answer changes because
of that edit. `python -m agent check` validates task anchors, expected items, edit markers
and stale-answer classification against the pinned corpus before a measured run.

Task categories are callers, name traps, files, types, locate and edit. The final score is
computed from an explicit JSON answer block; prose outside that block is ignored.

## Arms

Every task uses the same model, effort, task prompt, built-in tools and per-run budget.

- **A — baseline:** built-in Read/Grep/Glob/Edit/Write/Bash only.
- **B — CCM:** the same built-in tools plus the full CCM MCP surface and the short navigation
  note recommended for product use.
- **C — search-only:** the same built-in tools plus only CCM `search_code`, with a short
  search note. `search_code` itself uses hybrid semantic + graph ranking, so this is **not** a
  pure semantic arm. B versus C estimates the incremental value of CCM's explicit structural
  tools (`map`, `explain`, `find_usages`, `impact_of_change`, etc.) and their navigation note
  on top of the hybrid search surface; it does not isolate graph ranking from embeddings.

B and C are product configurations rather than a pure "tool present but never described"
ablation. The appended notes are therefore part of those arms and must not change after this
preregistration.

## Repetitions and ordering

The final run is 24 tasks × 3 arms × 3 repetitions = 216 agent runs. Arms are rotated
deterministically by task and repetition so each arm appears first once per task across the
three repetitions. Completed run records are resumable; a completed record is never silently
overwritten.

Pilot runs with fewer tasks or repetitions may be used only to validate the harness. They must
not be reported as the final L3 result.

## Primary outcomes

Report, without dropping failures:

1. task success rate and 95% Wilson interval by arm;
2. recall and precision of the structured answer;
3. actual model cost in USD;
4. input tokens, including cache creation/read tokens, and output tokens;
5. agent turns and wall-clock time;
6. stale-answer count on edit tasks;
7. task/category-level failures.

For cost, token, turns and wall time, B/A and C/A are paired by task. Repetitions are averaged
within each task first; the report then gives the median task ratio and a fixed-seed bootstrap
95% interval over tasks.

## Success definition

A run succeeds only when:

- every expected item is present;
- precision is at least 0.75;
- for edit tasks, the requested edit markers show that the edit was actually applied.

A pre-edit answer on an edit task is additionally classified as stale when it contains an item
that should have disappeared or misses an item introduced by the edit.

Budget exhaustion, timeout, malformed output, MCP connection failure and any other failed run
remain visible. They are not discarded to improve an arm's rate.

## Isolation and fairness

Each run gets a fresh exported corpus copy in a temporary directory outside the repository and the
home directory, and a fresh temporary HOME and CLAUDE_CONFIG_DIR, so no user Claude settings,
hooks, plugins, MCP servers or CLAUDE.md files are loaded. Claude Code runs in its normal mode,
not `--bare`. Authentication comes only from `ANTHROPIC_API_KEY`. The harness stops the
measurement, without recording the run, if Claude Code reports another authentication source, if
the tools it exposes differ from the arm's tool set, or if a CLAUDE.md file sits above the
workspace.

CCM artifacts are placed under the temporary repository's `.git/ccm-bench/` directory, not in
the source tree, so normal source discovery cannot benefit from or be polluted by index files.
All three arms see the same source snapshot and task prompt.

The harness records model/effort, Claude version, CCM version, CCM commit and binary hashes.
A measured run refuses to start while `benchmarks/agent`, `benchmarks/pyproject.toml` or
`benchmarks/corpus.json` has uncommitted changes.

## Claims

The result may support only what it measures. A negative or mixed result is retained and
published. No headline claim about agent success, token savings or cost savings is made before
the final run is complete.

## Amendments before the first measured run

1. 2026-10-02: `--bare` was dropped. A zero-cost smoke test (an invalid API key, so no model call
   succeeded) showed that `--bare` exposes only Bash, Edit and Read even when Grep, Glob and Write
   are requested, which would have made every arm, and the baseline above all, weaker than normal
   Claude Code use. Isolation now relies on the temporary HOME, CLAUDE_CONFIG_DIR and workspace
   location, and every run checks the reported authentication source and tool set.
2. 2026-10-02: `--max-total-usd` stops the harness before a run that could push the total cost
   over the given amount. It limits spending and does not change what is measured.
