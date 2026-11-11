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

## Agents

The three arms run separately for each agent, and arms are compared only within the same agent.

- **Claude Code** (`--agent claude`): normal mode with the built-in tools above; B adds the CCM
  server through `--mcp-config` and the note through `--append-system-prompt`; C allows only
  `search_code`.
- **Codex CLI** (`--agent codex`, ChatGPT subscription): `codex exec --json --ephemeral
  --ignore-user-config --ignore-rules --sandbox workspace-write` with a dedicated `CODEX_HOME`
  logged in once with `codex login`. Web search is disabled, and so are sub-agents, browser and
  computer use, apps, plugins, skill search, hooks, goals, image generation and memories, which
  leaves the shell (with Codex's bundled `rg`) and file edits. B adds the CCM server through
  `-c mcp_servers.context-manager.*` and the note through `developer_instructions`; C limits the
  server to `enabled_tools = ["search_code"]`. Codex reports neither its tool list nor a dollar
  cost, so isolation is checked from the tools each run used: an MCP tool outside the arm's set,
  web search, image generation or a sub-agent stops the measurement. Token counts are the quota
  measure; turns and cost are reported as unknown.

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
3. model cost in USD: as reported by Claude Code; for Codex, which reports none, an estimate from
   the official API price list (`agent/prices.py`; the report names the source and its date);
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
not `--bare`. Authentication comes only from the environment and is chosen with `--auth`: a
Claude subscription token (`CLAUDE_CODE_OAUTH_TOKEN`, created once with `claude setup-token`;
Claude Code reports the source as `none`) or an API key (`ANTHROPIC_API_KEY`). The harness stops
the measurement, without recording the run, if Claude Code reports another authentication source
than the chosen one, if the tools it exposes differ from the arm's tool set, or if a CLAUDE.md
file sits above the workspace. A usage-limit, rate-limit or overload error also stops it without
recording the run; the same command resumes after the limit resets.

With a subscription, `cost_usd` is Claude Code's API-price estimate for the tokens used, not money
spent; the token counts are the measure of quota use.

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
3. 2026-10-02: `--auth subscription` added at the owner's request, so the agent can run on the
   owner's Claude subscription instead of an API key; it is the setting that matches how the tool
   is used day to day, and quota use is what the owner wants to measure. Usage-limit stops are not
   recorded as runs.
4. 2026-10-02: the Codex agent was added (see Agents) at the owner's request, to measure both
   subscriptions. A zero-cost start without a login accepted every flag and override; its pilot
   checks the run-level isolation on real output before any final Codex run.
5. 2026-10-02: at the owner's request the report estimates Codex's cost from the official API
   price list, so that readers can weigh CCM in dollars as well as in tokens. Run records keep
   the raw token counts; the estimate is computed when the report is written.
6. 2026-10-02: after the first Claude pilot showed that the agent never selected CCM while paying
   the full tool-schema cost, a separate `context-first-pilot` product profile was added for
   follow-up pilots only. In that profile A remains built-ins, B exposes the new default
   `context` + `index_project` surface with a one-call context note, and C exposes the legacy full
   structural surface with the previous navigation note. The default `preregistered` profile and
   the final 24-task A/B/C design above are unchanged; results from the product pilot are not the
   preregistered final L3 result.
7. 2026-11-10: for the `context-first-pilot` product profile only, (a) the B note was shortened
   and now names what `context` returns (call sites and a completeness check); (b) an optional
   arm F (`--arms A,B,C,F`) gives B's tools plus an instruction to call `context` before any
   other tool. F measures forced use and is reported separately; A, B and C keep their
   definitions, so natural-use results stay comparable across pilots; (c) one Django task
   (`django-trap-html-escape`, outside the preregistered 24) was added for a larger repository.
   `--tasks all` and the preregistered profile are unchanged.
8. 2026-11-10: for the `context-first-pilot` profile and Claude Code only, an optional arm H
   (`--arms ...,H`) gives B's tools and note plus the opt-in Grep hook that CCM installs
   (`ccm-mcp hook claude-pre-tool-use`, passed with `--settings`). H is an installed product
   configuration, neither natural use nor forced use, and is reported apart; each record (version 3)
   carries how many greps the hook answered and let through. A, B, C and F are unchanged.
9. 2026-11-10: before any further product change, the iteration-5 held-out set was re-run with
   three repetitions at the iteration-5 commit (`results/agent/iter5-reps-20261110/`, rule in its
   `PLAN.md`). The large-repository saving held for both agents on the Django task, so two more
   Django held-out tasks are added before they are run or any CCM packet for them is looked at:
   `django-callers-constant-time-compare` (10 call sites in 9 functions) and
   `django-trap-urlencode` (7 calls of `django.utils.http.urlencode` in 7 functions, next to the
   standard-library `urlencode`, `QueryDict.urlencode` and the template filter). Both gold answers
   come from Python `ast` over `django/`. They are run with three repetitions, arms Codex A and B and
   Claude A, B, F and H, at a commit without product changes since iteration 5.
10. 2026-11-10: to test whether the large-repository result is specific to Django (Python), a second
    large repository in another language is added to the corpus: Tokio `tokio-1.41.1` (Rust,
    about 112 k non-test lines). Two held-out tasks are written before they are run or any CCM
    packet for them is looked at; their gold answers come from `rg` over the workspace's library
    crates and reading each call site's enclosing function: `tokio-callers-sleep-until` (11 call
    sites in 10 functions across four crates) and `tokio-trap-catch-unwind` (the helper nested in
    `AtomicWaker::do_register`, called only there, next to eleven `std::panic::catch_unwind`
    calls). They run with three repetitions, arms Codex A and B and Claude A, B, F and H, together
    with a re-run of the now tuned `django-trap-urlencode` that measures PR #17.
11. 2026-11-10: to measure #19 (path:name targets, Rust standard-library calls as external) on
    tasks it was not made for, two more Tokio held-out tasks are written before they are run or any
    CCM packet for them is looked at, gold from `rg` over the library crates and reading each
    enclosing function: `tokio-callers-async-op` (13 calls of `util/trace.rs` `async_op` in 13
    methods) and `tokio-trap-yield-now` (7 calls of `tokio::task::yield_now` in 5 files, next to
    `std::thread::yield_now`, the `loom` wrapper `crate::loom::thread::yield_now` and three
    scheduler `yield_now` methods). They run with three repetitions, arms Codex A and B and Claude
    A, B, F and H, together with a re-run of the now tuned `tokio-trap-catch-unwind`.
12. 2026-11-10: gold correction found after the iteration-8 runs. The preregistered gold of
    `tokio-trap-yield-now` listed `tokio/src/signal/registry.rs:smoke`, but that call sits in a
    `#[cfg(test)] mod tests` test, which the question excludes; four of the runs (Codex A and B,
    Claude F) left it out and were scored as failures for it. The gold now has six library calls
    in five functions. The iteration-8 records of that task were rescored with
    `scripts/rescore.py` from the stored answers; the originals are kept unchanged in
    `results/agent/iter8-20261110/<agent>/runs-preregistered-gold/` and both results are reported.
    A check of all other Tokio gold call sites found no other call inside a test module.
13. 2026-11-10: to measure #21 (unknown names answered with a text search) and #22 (items inside
    item-level brace macros indexed) on tasks they were not made for, two more Tokio held-out tasks
    whose targets are defined inside `cfg_*! { … }` blocks are written before they are run or any
    CCM packet for them is looked at, gold from `rg` over the library crates and reading each
    enclosing function (none inside a test module): `tokio-callers-with-scheduler` (8 calls in 8
    functions) and `tokio-callers-poll-proceed` (11 calls of `io/util` `poll_proceed_and_make_progress`,
    defined twice as `cfg_coop!`/`cfg_not_coop!` alternatives, in 11 methods). They run with three
    repetitions, arms Codex A and B and Claude A, B, F and H, together with a re-run of the now tuned
    `tokio-callers-async-op`.
14. 2026-11-10, written before the final run, with the owner's approval of its quota: the final run
    keeps the preregistered design above unchanged (24 tasks; arms A, B and C with their
    preregistered tools and notes; 3 repetitions; both agents) and adds one arm, **D**, the
    current product: B's built-in tools plus only `context` and `index_project`, with the context
    note used in the product pilots. D is reported next to B and C as its own arm (D/A); it does
    not replace B. The preregistered rotation of A, B and C is kept exactly; D runs after them in
    every task and repetition. Product and harness are the commit that adds this amendment, built
    once for the whole run; agent models are those of the product pilots (Claude Code
    `claude-opus-5-5`, Codex `gpt-6-luna`, the newest Codex model listed), effort high, local
    embeddings, a per-run budget of 2 USD.
    Eighteen of the 24 tasks were used in the product pilots of 2026-11-09 and 2026-11-10 and are
    therefore not held out; six were never run (`flask-files-globals`, `flask-types-scaffold`,
    `express-edit-stringify`, `express-edit-handle-request`, `serde-locate-tag-conflict`,
    `serde-edit-deserialize-seq`). The primary result covers all 24 tasks as preregistered; the
    report also gives the same outcomes separately for the six never-run tasks and the eighteen
    pilot tasks. Answers keep the preregistered scorer; the owner audit
    (`scripts/owner_audit.py`) is reported alongside without changing scores.
15. 2026-11-11, written before the run: **large-repository experiment.** The final run (amendment
    14) found no saving on Flask, Express and Serde; the product pilots found one on caller
    questions in large repositories, but on tuned tasks with one to three repetitions. This
    experiment tests that result. It is separate from the final run, whose A/B/C definitions are
    unchanged.
    - Repositories: Django 5.1 (Python, about 155 k non-test lines), Tokio 1.41.1 (Rust, about
      112 k) and, new in the corpus, scikit-learn 1.5.2 (Python, about 186 k).
    - Held-out tasks, chosen from source scans only, with no CCM packet looked at, gold from
      `scripts/python_callers_gold.py` (Python `ast`, relative imports and re-exports resolved,
      test files excluded) and from `rg` plus reading each enclosing function (Rust): per
      repository two caller tasks, one name-collision trap and one impact task (which files call
      a function). Django `is_aware`, `make_hashable`, `get_connection` (transaction, not mail),
      `get_resolver` (files); scikit-learn `check_cv`, `gen_batches`, `shuffle` (not NumPy's),
      `get_config` (files); Tokio `SpawnMeta::new_unnamed`, `unbounded_channel`, task-local
      `try_with` (not thread-local), `enter_runtime` (files). Twelve tasks.
    - Tuned tasks, reported apart and not part of the primary result: `django-callers-iri-to-uri`,
      `django-callers-constant-time-compare`, `django-trap-urlencode`,
      `tokio-callers-sleep-until`, `tokio-callers-async-op`, `tokio-callers-with-scheduler`.
    - Arms: A (built-ins) and D (current product as in amendment 14). Three repetitions, Claude
      Code (`claude-opus-5-5`) and Codex (`gpt-6-luna`), effort high, local embeddings, one
      commit and one release build for the whole run.
    - Primary outcome, per agent, on the twelve held-out tasks: median task ratio D/A of input
      tokens (cache included) with the fixed-seed bootstrap 95 % interval. **Saving shown** if the
      interval's upper bound is below 1.00 and D's success count is at most two runs below A's
      (of 36); **cost shown** if the lower bound is above 1.00; otherwise no detectable
      difference.
    - Boundary analysis, descriptive: the same ratio by category (callers, trap, impact) and by
      repository, and per task the rule used in the pilots (D's mean below A's and every D run
      below A's mean). The owner audit is reported alongside.
16. 2026-11-11, after the large-repository run (amendment 15), written before any rescoring:
    **scorer correction for generic owners.** The scorer cut an answer's name at the first `<`,
    so `tokio/src/runtime/scheduler/current_thread/mod.rs:Arc<Handle>.schedule`, a correct
    method of `impl Schedule for Arc<Handle>`, was read as the name `Arc`. Balanced `<...>`
    groups are now removed before the name is taken (`Arc<Handle>.schedule` → `schedule`;
    `ContentDeserializer<'de, E>` stays `ContentDeserializer`); an unbalanced `<` keeps the old
    cut. Recorded runs are not changed. `scripts/rescore_report.py` rescores the stored answers
    read-only and the result is published as a separate table next to the recorded one. Every
    published result set was checked; only two runs change, both of the tuned task
    `tokio-callers-with-scheduler` in `large-repo-20261111/` (Claude A#2, Codex D#3, fail →
    pass). The primary outcome of amendment 15 is input tokens and is unaffected; its success
    condition holds before and after. Later runs are scored with the corrected scorer.
