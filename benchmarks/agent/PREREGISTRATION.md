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
17. 2026-11-11, written and committed before any run: **second large-repository held-out set,
    after D-only product fixes.** Amendment 15 found no detectable difference for either agent
    (Codex D/A 0.91, 0.59–1.45; Claude Code 0.95, 0.74–1.12). Three product changes for arm D
    followed, each from a failure seen in those runs: dotted Python import paths resolve
    through re-exports (#26); the Grep hook accepts globs that only exclude test, bench or
    example directories, and answers only call lists made of proven calls (#28); and D for
    Claude Code now includes that hook (see below). This set tests the result again on tasks
    nobody has run.
    - Tasks: thirteen new held-out tasks. Every target differs from all earlier task targets.
      They were selected from source scans only, and no CCM packet for any of them was looked
      at.
      - Django and scikit-learn: `scripts/select_heldout.py`, seed 20261111, three callers
        tasks, one impact task (which files call) and one name-collision trap per repository.
      - Tokio: `scripts/rust_callers_gold.py --select`, seed 20261111, with path-form calls of
        functions that take no `self`. Its impact pool was empty, so Tokio gives two callers
        tasks and one trap.
      - Selected: Django `log_response`, `get_default_timezone`, `get_text_list`,
        `get_docs_version` (files), `django.urls.resolve` (trap: `URLResolver.resolve` and
        other `resolve` methods); scikit-learn `indexable`, `empirical_covariance`,
        `gen_even_slices`, `check_scoring` (files), `normalize` (trap: `unicodedata.normalize`);
        Tokio `ReadBuf::uninit` (`MaybeUninit::uninit` does not count), `signal::registry::
        globals`, `Registration::new_with_interest_and_handle` (trap: the
        `PollEvented` method of the same name).
      - Full commands, excluded names, pool sizes and the intermediate draws are in
        `results/agent/heldout17-20261111/SELECTION.md`.
      - Disclosure: the selection rules were revised during selection, before any run.
        - The scikit-learn trap pool was empty, which added the method and external-library
          collision rules.
        - Tokio's impact pool was empty, which added a third callers task per Python
          repository.
        - Hand checks of the Tokio gold added `cfg(all(test, …))` modules and
          `Self::` resolution to the Rust scanner.
        - Intermediate draws were seen.
    - Gold: Python from `scripts/python_callers_gold.py`. Rust from `scripts/rust_callers_gold.py`,
      with every call site of the three Tokio tasks read by hand; the scanner also reproduces
      the gold of three amendment-15 Tokio tasks (`new_unnamed`, `unbounded_channel`, `async_op`).
      Questions name the defining file and say that calls through aliases and re-exports count,
      because Codex read "`sklearn.get_config` directly" in amendment 15 as a spelling. They
      also say that passing the function as a value does not count, and they state the test-file
      rule; for Django that includes the fact that the `django/test/` package counts. A call inside
      a nested function is accepted under either function's name (`log_response` in
      `require_http_methods`/`decorator`/`inner`; `get_text_list` in
      `get_formset`/`hand_clean_DELETE`). An independent red-team review checked ten of the
      targets against the source before this commit and found no missing or wrong caller. The
      nested-name aliases and the question clarifications come from that review. Answers are
      scored with the scorer of amendment 16.
    - Arms: A (built-ins, unchanged) and D, the product at this amendment's commit: `context` +
      `index_project` with the context note and, for Claude Code only, the PreToolUse Grep hook
      (`ccm-mcp hook claude-pre-tool-use`). Codex has no hook mechanism in the harness, so its D
      differs from amendment 15 only by the product fixes. Claude Code's D is therefore labelled
      "D (context + hook)" in the report. A saving there cannot be split between the hook and the
      other fixes, and the two agents' D arms are not compared with each other. A hookless D for
      Claude Code is not run, to save quota. B and C are not run; their definitions are
      unchanged. Three repetitions, Claude Code (`claude-opus-5-5`) and Codex (`gpt-6-luna`),
      effort high, local embeddings, one commit and one release build for the whole run, started
      through `scripts/run_until_done.py`; runs rejected by a usage limit are not records.
    - Primary outcome, per agent, on the thirteen tasks: median task ratio D/A of input tokens
      (cache included) with the fixed-seed bootstrap 95 % interval.
      - **Saving shown** if the upper bound is below 1.00 and D's success count is at most two
        runs below A's (of 39).
      - **Cost shown** if the lower bound is above 1.00.
      - Otherwise: no detectable difference.
    - Power, from amendment 15's task spread: the interval reached 0.54 above the median for
      Codex and 0.17 for Claude Code. With a similar spread, the upper bound falls below 1.00
      only if the median is about 0.45 or lower for Codex and about 0.80 or lower for Claude
      Code. A "no detectable difference" result therefore means that a saving of that size is
      not shown. It does not show that there is no effect.
    - Stopping rule set by the owner: if both agents' intervals contain 1.00 again, that is
      recorded in `docs/STATUS.md` as the boundary result of this line of work, and it ends.
    - Reported alongside, descriptively:
      - the same ratio by category and by repository;
      - the per-task pilot rule;
      - hook decisions in Claude's D runs;
      - CCM calls per run;
      - every failed run;
      - runs rejected by a usage limit, by arm and task;
      - the owner audit.
18. 2026-11-11, written and committed before any run: **third large-repository held-out set,
    after a D-only change to the hook for repeated Greps.** Amendment 17 found a saving for
    Codex (D/A 0.72, 0.59–0.89) and no detectable difference for Claude Code with the hook in
    D (0.94, 0.77–1.10). In 11 of the hook's 19 answers, Claude then ran a Grep for the same
    symbol. The hook's message had invited that, and the hook let the repeat through.
    - Change (D only): the hook's first answer no longer invites a repeat. A second Grep for
      the same symbol in a session is answered with the raw matches at the listed call sites
      (`path:line: text`) and a count of the other matching lines. A third always passes. An
      offline replay on the amendment-17 transcripts (a tuned set, not held out) answers all
      11 repeats that way (`results/agent/hook-repeat-replay-20261111/`). Arms A, B and C are
      unchanged.
    - Tasks: thirteen new held-out tasks, selected with the procedure of amendment 17 (seed
      20261112). No CCM output for any of the targets was looked at.
      - Excluded: every earlier task target, plus the names seen in amendment 17's
        intermediate draws.
      - Django: five callers tasks (`get_current_timezone`, `prefix_validation_error`,
        `make_model_tuple`, `get_default_renderer`, `resolve_url`), one impact task
        (`conditional_escape`, files) and one trap (`django.contrib.auth.load_backend`, not
        `django.db.utils.load_backend`).
      - scikit-learn: five callers tasks (`compute_sample_weight`, `softmax`,
        `label_binarize`, `device`, `supported_float_dtypes`) and one impact task
        (`check_non_negative`, files). Its trap pool was empty.
      - Tokio: no task. With the corrected Rust scanner its pools were empty (see the
        disclosure below).
      - Rule set before the final draw: since Tokio gives no task, each Python repository
        gives five callers tasks instead of four.
      - Full commands and every draw: `results/agent/heldout18-20261111/SELECTION.md`.
      - Disclosure: a hand check of the first Tokio draw (`coop::Budget::unconstrained`) found
        a scanner bug. A bare call was accepted whenever the file imported the same name from
        any module, here `task::unconstrained`. A bare call now needs a `use` path through the
        definition's module. The amendment-17 Tokio gold is unchanged by the fix.
    - Gold from `scripts/python_callers_gold.py`; the `softmax` and `device` text matches it
      leaves out were read by hand (docstrings, comments; `array_device` is counted as an
      alias). Questions keep the wording rules of amendment 17 and add that a function calling
      itself counts as its own caller (`prefix_validation_error` does). Nested-function
      aliases: `user_passes_test`/`decorator`/`_redirect_to_login`. Scorer of amendment 16.
    - Arms A and D, three repetitions, both agents, settings as in amendment 17. Claude Code's D
      is "D (context + hook)" with the changed hook. Codex's D has no hook, so its run is a
      replication check of amendment 17 on new tasks.
    - Primary outcome, per agent: the median task ratio D/A of input tokens (cache included)
      over the thirteen tasks, with the fixed-seed bootstrap 95 % interval.
      - **Saving shown** if the upper bound is below 1.00 and D's success count is at most two
        runs below A's (of 39).
      - **Cost shown** if the lower bound is above 1.00.
      - Otherwise: no detectable difference.
    - Power: amendment 17's Claude interval reached 0.16 above its median. With a similar
      spread, a saving is shown only if Claude's median is about 0.84 or lower.
    - Stopping rule: if Claude Code's interval contains 1.00 again, STATUS records that the
      D-only route (context, hook, and repeat handling) gave no measurable saving for Claude
      Code on two held-out sets, and work on Claude adoption stops. If it does not contain
      1.00, the result is recorded as shown. Codex is reported either way.
    - Reported alongside, descriptively:
      - the ratio by category and by repository;
      - the per-task pilot rule;
      - hook decisions, by first answer and repeat answer;
      - how often Claude repeated a Grep after a hook answer;
      - CCM calls per run;
      - every failed run;
      - runs rejected by a usage limit;
      - the owner audit.
19. 2026-11-11, written and committed before any run: **fourth held-out set, Codex, after
    `context` fixes for its extra turns.** In amendments 17 and 18 Codex's D saved tokens
    overall, but three task kinds cost more, and the transcripts show the causes
    (`results/agent/codex-context-replay-20261111/`):
    - a caller entry repeated a call-site line and showed a phantom "+N more" when a local
      variable shared the target's name (`tokio-callers-globals`, 2.13);
    - `Type::name` was answered "not in the index" when two definitions share the name (the
      Tokio trap, 1.34);
    - "which files call X" got a function-level packet, and Codex re-derived the files with
      `rg` (1.30–1.70).
    - Change (in `context`, so arm D only; A, B and C unchanged):
      - each call-site line is listed once;
      - `Type::name` is resolved by the enclosing type or file module, and ambiguous names get
        the candidate list;
      - a complete packet adds `Files with calls (N): …`.
      - A replay of the 38 recorded Codex queries of those runs (tuned, not held out) shows 0
        repeated lines, 0 "not in the index" and a files line equal to the gold in 15 of 15
        impact queries.
    - Tasks: twelve new held-out tasks with the amendment-17 procedure (seed 20261113;
      `scripts/select_heldout.py` now takes the number of impact tasks). No CCM output for any
      target was looked at.
      - Django: two callers tasks (`flatten_fieldsets`, `call_command`), three impact tasks
        (`get_user_model`, `safe_join`, `get_current_site`) and one trap (`select_template`,
        not the engines' `select_template` methods).
      - scikit-learn: two callers tasks (`compute_class_weight`, `as_float_array`) and four
        impact tasks (`get_scorer_names`, `svd_flip`, `pairwise_distances`,
        `accuracy_score`). Its trap pool was empty.
      - Tokio: no task, because its pools are empty under the task rules (amendment 18). The
        Rust part of the fix is supported only by the e2e test and the tuned replay.
      - Commands and pools: `results/agent/heldout19-20261111/SELECTION.md`.
      - The amendment-17 and amendment-18 commands are rewritten with an impact count of 1,
        which reproduces their selections.
    - Arms A and D, three repetitions, **Codex only** (`gpt-6-luna`, effort high, local
      embeddings), one commit and one release build, started through
      `scripts/run_until_done.py`. Claude Code is not run: its D route ended under amendment
      18's stopping rule. Its next step is a separate arm with its own preregistration.
    - Primary outcome: Codex's median task ratio D/A of input tokens (cache included) over the
      twelve tasks, with the fixed-seed bootstrap 95 % interval, and the decision rule of
      amendment 17 (saving, cost or no detectable difference; success at most two runs below
      A, of 36).
    - Secondary outcome, preregistered: the same ratio over the seven impact tasks.
      - **Impact saving shown** if its upper bound is below 1.00.
      - **Impact cost shown** if its lower bound is above 1.00.
      - Otherwise: no detectable difference.
    - Stopping rule: if the impact interval does not show a saving, STATUS records that the
      files line did not turn "which files" questions into a saving for Codex, and that line of
      work stops.
    - Reported alongside:
      - per-task ratios;
      - whether Codex ran `rg` or `grep` after `context` in impact D runs (amendments 17 and
        18: 7 of 12);
      - CCM calls per run;
      - every failed run;
      - runs rejected by a usage limit;
      - the owner audit.
20. 2026-11-11, written and committed before any run: **arm E for Claude Code, a hook on Bash
    `grep`/`rg` searches.** Amendment 18 ended Claude Code adoption through D-only changes:
    after the Grep hook answered, Claude re-checked with Bash `grep -rn` every time. Arm E
    tests the route proposed there as a separate arm. A, B, C and D are unchanged.
    - Hypothesis: if the hook also answers Claude's Bash symbol searches, Claude Code uses
      fewer input tokens than with built-ins alone.
    - Arm E: arm D (`context` + `index_project` + the context note + the PreToolUse hook), with
      the hook also matched on `Bash`.
      - The hook answers a single search: an optional leading `cd`, `grep -r…` or `rg …`, and
        only fixed-string `grep -v` filters. It uses the Grep hook's rules: the caller packet
        first, then the call-site lines for a repeat.
      - Anything else runs unchanged.
      - E exists only for Claude Code in the preregistered profile.
    - Prediction recorded before the run (tuned replay, `results/agent/arm-e-replay-20261111/`):
      on the 78 amendment-17 and amendment-18 D transcripts, E's hook answers 7 of 76 recorded
      Bash searches. Most re-checks are compound commands. A saving is therefore not expected.
      The run measures it.
    - Tasks: thirteen new held-out tasks with the amendment-17 procedure (seed 20261114). No CCM
      output for any target was looked at.
      - Django: `make_naive`, `number_format`, `get_version_tuple`, `strip_quotes`,
        `validate_file_name` (callers); `is_naive` (files); `resolve_relation` from
        `django/db/migrations/utils.py` (trap: the `fields/related.py` function of the same
        name).
      - scikit-learn: `check_X_y`, `check_scalar`, `pairwise_distances_chunked`,
        `pairwise_kernels`, `has_fit_parameter` (callers); `randomized_svd` (files).
      - Rule set before the final draw: callers and trap targets must have no module-level
        call. A "which functions call X" question has no function to name for one, and
        `path:<module>` would not score. The draw takes seven callers per Python repository and
        skips such targets in draw order. In Django that skipped `deconstructible` (a decorator)
        and `double_output` (module-level assignments).
      - Every draw: `results/agent/heldout20-20261111/SELECTION.md`.
    - Gold from `scripts/python_callers_gold.py`, with nested-function aliases
      (`filesizeformat`/`filesize_number_format`, `__call__`/`f`). Scorer of amendment 16.
    - Arms A, D and E, three repetitions, **Claude Code only** (`claude-opus-5-5`, effort high,
      local embeddings). One commit and one release build, started through
      `scripts/run_until_done.py`.
    - Primary outcome: the median task ratio E/A of input tokens (cache included) over the
      thirteen tasks, with the fixed-seed bootstrap 95 % interval.
      - **Saving shown** if the upper bound is below 1.00 and E's success count is at most two
        runs below A's (of 39).
      - **Cost shown** if the lower bound is above 1.00.
      - Otherwise: no detectable difference.
    - Secondary, descriptive:
      - E/D and D/A with intervals;
      - hook decisions by tool (Grep or Bash) and answer kind;
      - how often Claude searched again after a hook answer.
    - Power: amendment 18's Claude spread (0.16 above the median) means a saving is shown only if
      the median is about 0.84 or lower.
    - Stopping rule: if the E/A interval does not show a saving, STATUS records that neither
      arm D nor arm E, hook routes that leave Claude's own behaviour unchanged, gave Claude Code
      a measurable saving. Work on Claude adoption then stops.
    - Reported alongside:
      - per-task ratios;
      - every failed run;
      - runs rejected by a usage limit;
      - the owner audit.
