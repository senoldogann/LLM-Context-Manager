# Large-repository experiment (2026-11-11)

Design and decision rule: preregistration amendment 15 (`benchmarks/agent/PREREGISTRATION.md`),
task lists frozen in `PLAN.md` before the first run. Twelve held-out tasks (the primary result)
and six tuned tasks (reported apart) on Django 5.1, Tokio 1.41.1 and scikit-learn 1.5.2; arms A
(built-ins) and D (current product: `context` + `index_project`), three repetitions, both agents.
This experiment has no B or C arm. The final run's A/B/C definitions (amendment 14) are
unchanged, and A and D here are the same arms as there.

## What was measured

- Harness and product commit: `01b2afa` (`harness_revision` and `ccm_commit` in both
  `settings.json` files; `ccm_dirty: false`). The harness refuses to start with uncommitted
  changes under `benchmarks/agent`, `pyproject.toml` or `corpus.json`.
- One release build from a clean clone at `01b2afa`: ccm-cli `5ea5c4bd122b…`, ccm-mcp
  `601fc2c0bae7…` (full SHA-256 in `settings.json`). These are byte-identical to the final run's
  binaries at `6d502c8`; no product code changed between the two commits.
- No binary was built from the uncommitted files of the old `feat-context-complete-signal`
  worktree (`mcp/src/hook.rs`, `mcp/src/tools.rs`, `benchmarks/agent/run.py`,
  `mcp/tests/token_budget_e2e_test.rs`, `docs/STATUS.md`). Those are stale copies from before
  2026-11-10; their content was ported and merged in #14, and the measured files at `01b2afa`
  are newer supersets of them.
- Agents: codex-cli 0.167.0 with `gpt-6-luna`, Claude Code 2.1.319 with `claude-opus-5-5`, both
  at effort high, on subscriptions, with local embeddings. Prebuilt indexes per repository:
  Django 1562.7 s, scikit-learn 898.4 s, Tokio 265.3 s to build once.

Ratios below are input tokens with cache, per task the D mean over repetitions divided by the A
mean; the median over tasks carries a fixed-seed bootstrap 95 % interval. Tables come from
`scripts/subset_report.py`; the full reports are `codex/report.md` and `claude/report.md`.

## Codex

All 108 runs recorded in one pass; no run stopped on a limit or an error.

### Held-out tasks (primary)

| Task | A tokens | D tokens | D/A | A ok | D ok | D runs calling CCM | Pilot rule |
|---|---:|---:|---:|---:|---:|---:|---|
| `django-callers-is-aware` | 114,032 | 60,635 | 0.53 | 0/3 | 3/3 | 3/3 | saving |
| `django-callers-make-hashable` | 79,571 | 33,027 | 0.42 | 2/3 | 1/3 | 3/3 | saving |
| `django-trap-get-connection` | 54,464 | 49,222 | 0.90 | 3/3 | 3/3 | 3/3 | – |
| `django-impact-get-resolver` | 37,587 | 60,948 | 1.62 | 3/3 | 3/3 | 3/3 | – |
| `sklearn-callers-check-cv` | 45,361 | 86,259 | 1.90 | 3/3 | 3/3 | 3/3 | – |
| `sklearn-callers-gen-batches` | 61,338 | 39,125 | 0.64 | 3/3 | 3/3 | 3/3 | saving |
| `sklearn-trap-shuffle` | 134,615 | 73,086 | 0.54 | 3/3 | 1/3 | 3/3 | saving |
| `sklearn-impact-get-config` | 38,894 | 55,670 | 1.43 | 0/3 | 0/3 | 3/3 | – |
| `tokio-callers-new-unnamed` | 41,631 | 33,018 | 0.79 | 3/3 | 3/3 | 3/3 | saving |
| `tokio-callers-unbounded-channel` | 47,753 | 70,573 | 1.48 | 3/3 | 3/3 | 3/3 | – |
| `tokio-trap-try-with` | 51,903 | 66,227 | 1.28 | 3/3 | 3/3 | 3/3 | – |
| `tokio-impact-enter-runtime` | 35,421 | 32,692 | 0.92 | 2/3 | 3/3 | 3/3 | saving |

- D/A median: **0.91 (0.59–1.45, 12 tasks)**
- Success: A 28/36 (62 %–88 %), D 29/36 (65 %–90 %)
- Pilot rule (D mean below A's and every D run below A's mean) met on 6 of 12 tasks
- By category: callers 0.72 (0.47–1.69, 6 tasks); impact 1.43 (0.92–1.62, 3); trap 0.90
  (0.54–1.28, 3)
- By repository: Django 0.72 (0.42–1.62); scikit-learn 1.03 (0.54–1.90); Tokio 1.10
  (0.79–1.48); 4 tasks each

**Decision (amendment 15): no detectable difference.** The interval contains 1.00 on both sides.

### Tuned tasks (not part of the primary result)

| Task | A tokens | D tokens | D/A | A ok | D ok | D runs calling CCM | Pilot rule |
|---|---:|---:|---:|---:|---:|---:|---|
| `django-callers-iri-to-uri` | 55,315 | 29,423 | 0.53 | 3/3 | 3/3 | 3/3 | saving |
| `django-callers-constant-time-compare` | 56,292 | 32,489 | 0.58 | 3/3 | 3/3 | 3/3 | saving |
| `django-trap-urlencode` | 130,362 | 43,555 | 0.33 | 3/3 | 3/3 | 3/3 | saving |
| `tokio-callers-sleep-until` | 51,041 | 60,806 | 1.19 | 2/3 | 3/3 | 3/3 | – |
| `tokio-callers-async-op` | 50,001 | 32,951 | 0.66 | 3/3 | 3/3 | 3/3 | saving |
| `tokio-callers-with-scheduler` | 47,526 | 36,878 | 0.78 | 2/3 | 2/3 | 3/3 | saving |

- D/A median: 0.62 (0.43–0.98, 6 tasks)
- Success: A 16/18 (67 %–97 %), D 17/18 (74 %–99 %)
- Pilot rule met on 5 of 6 tasks

Over all 18 tasks, D's median agent wall time was 22.9 s against A's 25.5 s.

### Codex failures

Eighteen runs failed: ten in A, eight in D. Every record is kept in `codex/runs/`.

| Run | What was missing or wrong |
|---|---|
| `django-callers-is-aware` A#1 | `timezone.py:make_aware` |
| `django-callers-is-aware` A#2 | 18 of the callers |
| `django-callers-is-aware` A#3 | `make_aware`, `humanize.string_for`, `serializers/json.default`, `timesince` |
| `django-callers-make-hashable` A#1, D#1, D#2 | `hashable.py:make_hashable`, the function's recursive call to itself (see note) |
| `sklearn-impact-get-config` A#1, A#2, A#3, D#2, D#3 | nearly every file: answered with the literal spelling `sklearn.get_config(` only (see note) |
| `sklearn-impact-get-config` D#1 | `utils/validation.py` (imports it as `_get_config`) |
| `sklearn-trap-shuffle` D#2, D#3 | the eight callers in `sklearn/datasets/` that import `shuffle` under another name (see product finding) |
| `tokio-callers-sleep-until` A#3 | eight callers in `tokio-test`, `tokio-util` and `time/` |
| `tokio-callers-with-scheduler` A#2 | `context.rs:defer` |
| `tokio-callers-with-scheduler` D#3 | none in substance: scorer parsing (see note) |
| `tokio-impact-enter-runtime` A#2 | `multi_thread/mod.rs` (wrote the path with a space in it) |

Definition notes, golds unchanged:

- `django-callers-make-hashable`: the `ast` gold counts `make_hashable`'s recursive call to
  itself as a caller, which follows the task's wording ("functions that call it"). Three runs
  left the function itself out; read with that definition they would have passed.
- `sklearn-impact-get-config` asks which files call "`sklearn.get_config` directly". The gold
  counts every call of the function, including `get_config()` after `from .._config import
  get_config` and the `_get_config` alias. Codex read the wording as the qualified spelling
  `sklearn.get_config(` and answered `_config.py` or nothing.

Scorer note, records unchanged: `tokio-callers-with-scheduler` D#3 answered
`current_thread/mod.rs:Arc<Handle>.schedule` and `…Arc<Handle>.unhandled_panic`, which are
correct (`impl Schedule for Arc<Handle>`). The scorer strips nothing from a generic owner and
reads `Arc` as the function name, so it counts both as missing and `Arc` as extra. The same
answer fails Claude's `tokio-callers-with-scheduler` A#2. This is a tuned task, so the primary
result is unaffected; fixing the parser is a separate change.

### Product finding: dotted Python paths

In 10 of 36 scikit-learn D runs, Codex's first `context` call named the public import path
(`sklearn.utils.shuffle`, `sklearn.get_config`, `sklearn.model_selection.check_cv`,
`sklearn.utils.gen_batches`). CCM answered "`shuffle` is not in the index … the index has no
symbol there (for example a definition inside a macro body)" and fell back to text search, which
cannot see calls through another name. The same function named by its bare name (`shuffle`)
resolved to `utils/_indexing.py:567` with the alias imports listed. The two failing
`sklearn-trap-shuffle` D runs are this case, and it fits D's higher token use on
`sklearn-callers-check-cv` (1.90) and `sklearn-impact-get-config` (1.43). Resolving
`package.module.name` through `__init__.py` re-exports is the next product step; it is not
measured here.

### Owner audit

`scripts/owner_audit.py` flags no Codex run.

## Claude Code

All 108 runs recorded (see the run log for the limit stop).

### Held-out tasks (primary)

| Task | A tokens | D tokens | D/A | A ok | D ok | D runs calling CCM | Pilot rule |
|---|---:|---:|---:|---:|---:|---:|---|
| `django-callers-is-aware` | 36,731 | 26,660 | 0.73 | 3/3 | 3/3 | 3/3 | – |
| `django-callers-make-hashable` | 29,262 | 39,795 | 1.36 | 3/3 | 3/3 | 0/3 | – |
| `django-trap-get-connection` | 27,684 | 32,106 | 1.16 | 3/3 | 3/3 | 0/3 | – |
| `django-impact-get-resolver` | 18,816 | 18,124 | 0.96 | 3/3 | 3/3 | 2/3 | saving |
| `sklearn-callers-check-cv` | 27,452 | 20,769 | 0.76 | 3/3 | 3/3 | 3/3 | saving |
| `sklearn-callers-gen-batches` | 38,789 | 34,351 | 0.89 | 3/3 | 3/3 | 2/3 | – |
| `sklearn-trap-shuffle` | 68,388 | 78,369 | 1.15 | 3/3 | 3/3 | 0/3 | – |
| `sklearn-impact-get-config` | 19,543 | 18,430 | 0.94 | 3/3 | 3/3 | 1/3 | – |
| `tokio-callers-new-unnamed` | 46,739 | 25,381 | 0.54 | 3/3 | 3/3 | 1/3 | saving |
| `tokio-callers-unbounded-channel` | 26,636 | 28,997 | 1.09 | 3/3 | 3/3 | 1/3 | – |
| `tokio-trap-try-with` | 41,909 | 28,831 | 0.69 | 3/3 | 3/3 | 0/3 | saving |
| `tokio-impact-enter-runtime` | 15,507 | 16,554 | 1.07 | 3/3 | 3/3 | 0/3 | – |

- D/A median: **0.95 (0.74–1.12, 12 tasks)**
- Success: A 36/36 (90 %–100 %), D 36/36 (90 %–100 %)
- D runs that called CCM: 13/36
- Pilot rule met on 4 of 12 tasks
- By category: callers 0.82 (0.63–1.22, 6 tasks); impact 0.96 (0.94–1.07, 3); trap 1.15
  (0.69–1.16, 3)
- By repository: Django 1.06 (0.73–1.36); scikit-learn 0.91 (0.76–1.15); Tokio 0.88
  (0.54–1.09); 4 tasks each

**Decision (amendment 15): no detectable difference.** The interval contains 1.00 on both sides.

### Tuned tasks (not part of the primary result)

| Task | A tokens | D tokens | D/A | A ok | D ok | D runs calling CCM | Pilot rule |
|---|---:|---:|---:|---:|---:|---:|---|
| `django-callers-iri-to-uri` | 35,369 | 18,656 | 0.53 | 3/3 | 3/3 | 3/3 | saving |
| `django-callers-constant-time-compare` | 35,615 | 37,106 | 1.04 | 3/3 | 3/3 | 0/3 | – |
| `django-trap-urlencode` | 49,914 | 40,870 | 0.82 | 3/3 | 3/3 | 0/3 | saving |
| `tokio-callers-sleep-until` | 44,502 | 31,491 | 0.71 | 3/3 | 3/3 | 0/3 | saving |
| `tokio-callers-async-op` | 42,890 | 28,963 | 0.68 | 3/3 | 3/3 | 2/3 | saving |
| `tokio-callers-with-scheduler` | 41,007 | 28,535 | 0.70 | 2/3 | 3/3 | 3/3 | saving |

- D/A median: 0.70 (0.60–0.93, 6 tasks)
- Success: A 17/18 (74 %–99 %), D 18/18 (82 %–100 %)
- Pilot rule met on 5 of 6 tasks

Over all 18 tasks D/A was 0.85 (0.71–1.05) for input tokens, 1.02 (0.92–1.07) for cost and
0.82 (0.73–0.93) for wall time; median wall time D 14.9 s, A 18.2 s. Claude called CCM in 21 of
54 D runs. Where it did not, the D tokens differ from A's by the agent's own path and by the
two tool definitions.

### Claude failures

One run failed: `tokio-callers-with-scheduler` A#2, the scorer parsing case in the note above.

### Owner audit

`scripts/owner_audit.py` flags three passing Claude runs of the tuned task
`django-trap-urlencode` (A#2, A#3, D#3). They name `BaseModelAdmin.get_preserved_filters`; the
method is defined on `ModelAdmin`.

## What this shows

On twelve held-out caller, trap and impact questions in three large repositories, the current
product does not measurably change input tokens for either agent: D/A 0.91 (0.59–1.45) for
Codex and 0.95 (0.74–1.12) for Claude Code, with success equal or within one run. The tuned
pilot tasks again show a saving for both agents (0.62 and 0.70), so the pilots' result does not
carry over to new tasks. The spread is large per task: D saves on some caller tasks
(`django-callers-is-aware`, `tokio-callers-new-unnamed`) and costs more on others. For Codex,
part of the cost comes from the dotted-path finding above.

## Run log

- 2026-11-10 22:42Z: `PLAN.md` written (its header says 2026-11-11 by mistake; the file time
  is 2026-11-10 22:42Z). First run 23:23Z, both agents from the same command.
- Claude stopped once at run 55 with `OSError: [Errno 66] Directory not empty` while deleting a
  finished run's temporary workspace (a git process still held `.git/objects`). No record was
  written for that run; the same command resumed from the 54 recorded runs.
- 2026-11-11 00:14–00:15Z: Claude's subscription reached its weekly limit. Claude Code
  returned error results with HTTP status 429 (`usage_limit_reached`, "You've hit your weekly
  limit"). The harness at `01b2afa` matched quota errors by text markers, none of which this
  message contains, so it recorded 14 runs as failures instead of stopping. All 14 lie in one
  34-second block: 13 carry the 429 result and one
  (`sklearn-impact-get-config` D#3) exited after its `init` event with no result. They are agent
  non-runs, not answers, and were moved unchanged to `claude-quota-rejected/attempt-1/`, where
  they remain. The fix (the harness now stops on status 429) is a later commit and was not used
  for any run here.
- 2026-11-11 01:10Z: a first resume from a separate checkout at `01b2afa` (same binaries and
  command) met the same limit; its 14 rejected runs are in `claude-quota-rejected/attempt-2/`.
- 2026-11-11, after the owner supplied a new Claude Code OAuth token for an account with quota
  left: the same resume recorded the 14 missing runs, all from the `01b2afa` checkout with the
  same binaries, agent version and command (the harness verified `settings.json` unchanged).
  These 14 runs ran under a different subscription token from the other 94; model, effort and
  agent version are the same.
