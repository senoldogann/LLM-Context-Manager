# Level 1 freshness benchmark: pre-registration

Fixed on 2026-10-01, before the first measured run. Anything changed after the
first run is reported under [Deviations](#deviations), never edited in place.

## Conflict of interest

This benchmark was written for the CCM maintainer, and CCM is one of the
systems under test. The safeguards: the scenarios, oracles, classification
rules and the H1 threshold are committed before any run; every oracle follows
from the scripted edit, not from any system's output; competitor adapters call
only public CLI/MCP interfaces; every probe is published raw; a "no" is
published like a "yes".

## Question and hypothesis

After a file is saved, how often does a code-graph MCP server answer from the
pre-edit code **without saying so**?

**H1.** On scenario S1 (real repositories, all edits, all probe times), CCM's
`STALE_SILENT` rate is at least **5 percentage points** lower than that of the
best competitor; or it is within 5 points and CCM is the only system whose
staleness label reaches the model-visible channel (`content` text). Otherwise
H1 does not hold. If no competitor has been run (approval gate G1), H1 is
reported as *not measurable* and only CCM's own numbers are published.

The threshold was proposed in the task brief and fixed here unchanged.

## Systems

| System | Version | Mode |
|---|---|---|
| CCM | `origin/main` at `136b106`, release build | graph only (`CCM_DISABLE_EMBEDDER=1`), auto-refresh on unless a scenario says otherwise |
| competitors | pinned when approved (G1) | each system's documented default watcher/refresh mode |

Each run records the binary path, its SHA-256 and the reported server version.

## System-neutral queries

Adapters map three questions to each system's own tools:

- `callers(symbol)`: names of the functions/methods the system reports as
  calling `symbol`, plus whether `symbol` itself was found.
- `exists(symbol)`: whether the system reports a function/method named `symbol`.
- `node_at(file, line)`: name and line range of the innermost function/method
  the system reports at that location.

**CCM mapping.** `callers`: `find_nodes(query=symbol, limit=50)`, take the
function/method block whose name equals `symbol`, then
`find_usages(node_id, limit=200)` and collect the block names. `exists`:
`find_nodes(query=symbol, limit=50)` with an exact name match. `node_at`:
`get_context(file, line)`, the `## Current:` block. Competitor mappings are
added to this file before those systems run.

**Staleness label.** A probe is *labeled* if any response the probe received
carries the system's own staleness signal in a model-visible `content` text
item. For CCM that is the first text line `_Index: …_` containing `stale`,
`pending`, `refresh running`, `refresh failed`, `auto-refresh off` or
`auto-refresh unavailable`; `fresh` alone is not a label. A label found only in
`structuredContent` is recorded as `structured_only`.

## Scenarios

Injected symbols use unique `ccmb_*` names, so no name-resolution ambiguity is
involved (edge accuracy is Level 2). Python sources. A uniform 1.0 s settle
delay follows each system's ready signal before the edit.

| Id | Repository | Edit | Post-edit oracle (CORRECT) | Pre-edit state (STALE) |
|---|---|---|---|---|
| S1-add | Flask 3.0.3, Django 5.1 | append `ccmb_caller_b` (imports and calls `ccmb_target`) to a third module | `ccmb_caller_b ∈ callers(ccmb_target)` | target found, `ccmb_caller_a` present, `ccmb_caller_b` absent |
| S1-remove | same | replace the body of `ccmb_caller_a` (its import and its call) with `return 0` | target found, `ccmb_caller_a ∉ callers` | `ccmb_caller_a ∈ callers` |
| S1-rename | same | rename `ccmb_target` → `ccmb_target_renamed` in its module and in `ccmb_caller_a` | renamed found, `ccmb_caller_a ∈ callers(renamed)`, old name absent | old name found with `ccmb_caller_a`, renamed absent |
| S2 | generated | auto-refresh off where the system allows it; add a caller; run the system's own re-index out of process; probe the session opened before the re-index | as S1-add | as S1-add |
| S3-branch | generated, two branches | `git checkout feature` (on `feature`, caller a neither imports nor calls the target; caller b added) | `b ∈ callers`, `a ∉ callers` | `a ∈ callers`, `b ∉ callers` |
| S3-bulk | generated | 20 files each gain a caller in one pass | all 20 present | none present |
| S4-nested | generated, nested git repo under `libs/inner` | add a caller inside the nested repo | as S1-add | as S1-add |
| S4-nogit | generated, no `.git` | add a caller | as S1-add | as S1-add |
| S5 | generated | insert 5 comment lines at the top of `pkg/shift.py` (`shifted_fn` at 10–12, `occupant_fn` at 15–17 before the edit) | `node_at(pkg/shift.py, 16)` is `shifted_fn`, range 15–17 | `node_at(pkg/shift.py, 16)` is the pre-edit occupant `occupant_fn`, range 15–17 |
| S6 | generated, 50 files | all 50 files gain a caller within ≤1 s | 50/50 callers present | 0/50 present |
| S7 | generated | phase 1: save a syntactically broken version of `pkg/caller_a.py`; phase 2: save a valid version of it that also defines `ccmb_caller_c`, a second caller | phase 1: `ccmb_caller_a ∈ callers` (last good state kept); phase 2: `ccmb_caller_c ∈ callers` | phase 2: target found, `ccmb_caller_a` present, `ccmb_caller_c` absent |
| S8-delete | generated | delete the file holding `ccmb_caller_b` | `b ∉ callers`, `b` not found | `b ∈ callers`, `b` found |
| S8-move | generated | move that file to `pkg/relocated/caller_b.py` (one rename) | `b` found under the new path only | `b` found under the old path only |
| S9 | all runs | — | where labels appear: `content` text, `structuredContent` only, or never | — |

S2–S8 use small purpose-built repositories. They test behaviour in a known
situation; they are not evidence of value on real code.

**Working copies.** S1 exports the pinned commit (`git archive`), appends
`ccmb_target` to a target module and `ccmb_caller_a` (which imports and calls
it inside its body) to a second module, and commits that as one baseline
commit; the added caller goes into a third module. Flask: `src/flask/helpers.py`,
`src/flask/app.py`, `src/flask/cli.py`. Django: `django/utils/text.py`,
`django/utils/html.py`, `django/utils/functional.py`. The generated repositories
hold a package `pkg/` with `target.py`, `caller_a.py` and 10 filler modules; S8
adds `caller_b.py` at baseline, S3-bulk 20 modules under `bulk/`, S6 50 under
`storm/`. The nested repository avoids `vendor/` because CCM excludes that
directory name (`core/src/lib.rs`), which would measure the exclusion list
rather than nested-repository handling. Commits use a fixed author and date.

"Exactly the pre-edit state" means every listed pre-edit fact holds: the target
is found (or not) as listed, the listed callers are present and the callers the
edit adds are absent. Callers not named in the table are ignored.

## Probes and classification

Probes run at t = 0, 0.25, 0.5, 1, 2, 5 and 30 s after the edit's last write
returns (for S2: after the re-index process exits), in one session, in order.
The actual send time of every probe is recorded. Earlier probes may affect
later ones; that is part of what an agent experiences.

Each probe gets exactly one class:

- `CORRECT`: the post-edit oracle holds.
- `STALE_SILENT`: the answer is exactly the pre-edit state and no label reached
  `content` text.
- `STALE_LABELED`: the answer is exactly the pre-edit state and a label reached
  `content` text.
- `STALE_STRUCTURED_ONLY`: the pre-edit state with a label only in
  `structuredContent`; counted as silent for H1.
- `ERROR_EMPTY`: an error, a timeout, a missing target, or a state that is
  neither pre- nor post-edit (a partial update). Partial states are flagged in
  the raw record.

A probe is flagged `partial` when some but not all of the caller changes the
edit makes are visible, or when one question of a multi-question probe (S1-rename,
S8-delete) matches the post-edit state and another does not.

S6 additionally records the count of reflected callers per probe and whether
it ever decreases. S7 phase 1 is classified `PRESERVED_LABELED` (last good state
kept and a label in `content` text), `PRESERVED_SILENT` (kept, no such label),
`LOST` (not kept) or `ERROR_EMPTY`.

## Run protocol

1. Each run gets a fresh, empty `HOME` and a fresh working copy; both are
   deleted afterwards.
2. The initial index is built out of process with the system's own CLI (CCM:
   `ccm-cli index --path <repo>`); a failed build is retried once with a warning.
3. The MCP server is started over stdio with only `PATH`, `HOME` and the
   system's own settings in its environment.
4. Ready signal: `exists("ccmb_target")` returns found; polled every 0.1 s,
   timeout 120 s.
5. Baseline: the first phase's questions are asked once and whether the
   pre-edit state holds is recorded. Runs whose baseline does not hold are
   reported, not dropped.
6. Settle 1.0 s, apply the edit, run the probes. Each tool call has a 30 s
   timeout; a timeout is `ERROR_EMPTY`.
7. Order is repetition-major: repetition 1 runs every scenario once, then
   repetition 2, then 3.
8. The harness refuses to run while `benchmarks/freshness`,
   `benchmarks/pyproject.toml` or `benchmarks/uv.lock` have uncommitted
   changes, and every results file records the harness commit.

## Metrics

- Class counts per system × scenario × t, over 3 repetitions.
- Time to correct: the first probe time after which every later probe is
  `CORRECT`; median, minimum and maximum over repetitions.
- `STALE_SILENT` rate (`STALE_SILENT` + `STALE_STRUCTURED_ONLY` over all
  probes) per scenario; H1 uses S1.
- Variance: per-repetition counts are published; no pooled significance test
  is claimed for 3 repetitions.
- Descriptive only, outside every hypothesis: index build time, time to the
  ready signal, and the duration of one probe.

## Environment

macOS on Apple M4, release builds. Each system runs with an empty `HOME` and
only `PATH`, `HOME` and the system's own settings in its environment; nothing
from the maintainer's home directory is read. Each run uses a fresh working
copy that is deleted afterwards.

## What this does not measure

Agent outcomes, token cost, edge accuracy (Level 2), semantic search quality,
and behaviour on platforms other than macOS.

## Exploration before pre-registration (disclosed)

To learn CCM's response format, one manual session ran on a two-file toy
repository before this file was written: after a caller was appended, the
query at t = 0.00 s returned the pre-edit callers labeled `fresh`; the query
at 0.32 s was correct. Nothing above was tuned to that observation; it is
disclosed because it was seen before pre-registration.

## Deviations

1. **S5 block parsing (found after the first CCM run).** `get_context` returns,
   after the `## Current:` block, an `## Active element: …` block for the leaf
   node at the cursor. The harness accepted only one-word block kinds, missed
   that heading, and attributed the leaf's `Range` (16-16) to the `Current`
   block. Every S5 probe was therefore `ERROR_EMPTY` and no S5 baseline held.
   Names were unaffected: `occupant_fn` at t = 0 and `shifted_fn` from
   t = 0.25 s on, in 3 of 3 runs. The fix accepts multi-word kinds. `find_nodes`
   and `find_usages` titles use one-word node types (`core/src/engine.rs`), so
   S1–S4 and S6–S8 are unaffected. S5 was re-run alone (3 repetitions) with
   the fixed harness. Both results files are published; the summary takes S5
   from the re-run. Rerun outcome (3/3, harness `ecf7576`): baseline met;
   `STALE_SILENT` at t = 0; `CORRECT` from t = 0.25 s on; status line
   `fresh · auto-refresh on` in every probe, so the one stale answer carried no
   label. No errors, no partial states.
2. **Competitor runs not performed (2026-10-01).** The project moved from the
   comparison to product work before any competitor was measured. Draft
   adapters, smoke runs and their notes are archived on branch
   `bench/ccm-bench` (commits `b78f708`–`50d2f7e`) and are not part of this
   harness. H1 is not measurable; the published CCM results above are unchanged.
