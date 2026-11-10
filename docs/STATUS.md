# Project status

The current state of CCM and the next steps, for the owner and for any agent that continues the
work. Keep it current: replace lines that stop being true instead of appending history (git log and
`benchmarks/results/` are the history).

Last updated: 2026-11-10. Where work stopped: **the preregistered final L3 run is complete
(`benchmarks/results/agent/final-l3-20261110/`, commit `6d502c8`, 288 runs per agent).** On the
preregistered tasks (Flask, Express, Serde) CCM lowers neither token use nor raises success for
either agent. Claude Code practically never calls it (0 of 72 runs in B and C, 2 of 72 in the
current product D) and pays only the tool-definition cost: input tokens B/A 1.24 (1.15–1.28),
D/A 1.05 (0.91–1.06), success 100 % in every arm. Codex calls it in nearly every run and pays
more: B/A 1.78 (1.68–2.09), C/A 2.09 (1.85–2.65), D/A 1.12 (1.06–1.30), with success A 92 %,
B 90 %, C 82 %, D 85 % (overlapping intervals). The product pilots found the saving on caller
questions in large repositories (Django, Tokio), which the preregistered set does not contain;
that result rests on one to three repetitions per task and is not tested by the final run.

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
| Release | v0.5.0 on npm and the MCP registry; the branch has unreleased fixes, including one for the npm launcher of 0.5.0 (MCP mode overrode the client's project roots) | `RELEASE_NOTES.md`, `npm/bin/ccm.js` |
| Python | calls, imports and inheritance resolved from the syntax tree | `core/tests/` |
| Rust | resolved through modules, `use` paths, workspace crates and impl blocks | `benchmarks/README.md`, "Rust edges (M3)" |
| Other languages | matched by name and labelled `calls (inferred …)` or `may call`; JS/TS members defined by assignment get receiver-aware evidence | `core/tests/lexical_labels_test.rs`, `mcp/tests/token_budget_e2e_test.rs` |
| Freshness after an edit (L1) | first-read validation since `fb0ecd4`; a failed refresh now retries with backoff, `context` waits up to 10 s for a pending refresh | `mcp/tests/auto_refresh_test.rs` |
| Claude Code Grep hook | opt-in `ccm-mcp hook claude-pre-tool-use` (PreToolUse); answers a single-symbol Grep with a compact answer (target, completeness check, one line per caller) only when the index is fresh, the check says the list is complete and the whole answer is shorter than a lower bound of the grep output in the requested mode; otherwise the grep runs. Offline replay of 119 recorded Claude greps: 16 answered, 35 % less text on those. Not installed by the npm launcher | `mcp/src/hook.rs`, `benchmarks/results/agent/hook-replay-20261110/` |
| Agent interface | default discovery `context + index_project` (1,134-byte `tools/list`, 266 o200k tokens, plus 65-token server instructions); legacy tools callable, discoverable with `CCM_MCP_LEGACY_TOOLS=1` | `mcp/src/server.rs` |
| Agent experiment (L3) | preregistered final run complete (24 tasks × A/B/C + current product D × 3 repetitions × 2 agents, commit `6d502c8`); product pilots on 2026-11-09 and 2026-11-10 | "Agent pilots" below |

## What `context` returns

One call with a symbol, `path:line`, `receiver.member` or a question. For a function or method the
packet starts with a **grep-equivalent check**: the server reads the indexed files, classifies every
textual match of the target (call, other use, import, external module, string or docstring, call
resolved to another definition with the same name, recursion, Rust method of another type) and says
either "call list complete" or which lines lie outside the listed callers. Each caller carries its
call-site lines (at most 2, then `+n more`). The target body is capped at 40 lines and callees at
10. Python dotted module paths (`django.utils.html.escape`) and unique code-like names route without
embeddings. The check covers indexed non-test files; calls that never name the target in source
(getattr, runtime aliases) are out of reach of both text and graph, and the packet says so.
Implementation: `mcp/src/call_sites.rs`, `context` in `mcp/src/tools.rs`.

An independent code review after iteration 3 found ways the check could claim completeness wrongly
(calls inside f-strings and template literals or after a `#`/`//` inside a string read as text;
`exports.f = function` targets lost their real callers; aliased imports and lines the graph
attributes to another same-named definition did not soften the headline; a weak stale-line guard).
The fixes make the check stricter and are tested (unit and end-to-end, release preflight packets
unchanged), but they came after the iteration-3 runs and have no agent measurement yet.

## What the measurements cover

| Level | Question | Measures | Does not cover |
|---|---|---|---|
| L1 freshness | After an edit, does a tool answer from the new code? | each tool's answers after scripted edits | agent behaviour |
| M2 answer cost | How many tokens does a CCM answer cost? | response bytes and calls per question, before and after M2 | whether an agent reads less overall |
| L3 agents | Does the same agent finish real tasks better or with less quota with CCM? | success, recall and precision, tokens, turns, time, cost (Claude), stale answers after edits; arms A/B/C (and the forced-use arm F in product pilots) for Claude Code and Codex; corpus Flask, Express, Serde, Django and (product pilots) Tokio | open-ended feature work and debugging, long sessions, languages other than Python, JavaScript and Rust |
| L2 edge accuracy | Are the graph's edges right against a language server? | not run | — |

L3 has 24 tasks × 3 repetitions per arm and agent: the report gives intervals, and differences
inside them are not evidence either way.

## Agent pilots

All product pilots: `--profile context-first-pilot`, one repetition, subscriptions, local
embeddings. A = built-in tools; B = `context` + `index_project` + a short note (natural use);
C = legacy full surface; F = B plus an instruction to call `context` first (forced use, reported
apart). Ratios are input tokens including cache, per task (arm mean over repetitions / A), median
over tasks with a fixed-seed 95% bootstrap interval. Three or four tasks per cell: intervals are
wide and one repetition cannot separate small effects from run-to-run noise.

| Set (commit) | Agent | Tasks | Answers correct | B/A median (interval) | per task | F/A |
|---|---|---|---|---|---|---|
| 2026-11-09 (`be48997`) | Codex `gpt-6-luna` | 3 tuned | 9/9 | 1.11 (0.98–1.14) | | |
| 2026-11-09 (`be48997`) | Claude `opus-5-5` | 3 tuned | 9/9 | 0.96 (0.77–1.02), B never called CCM | | |
| iteration 1 held-out (`b6d055d`) | Codex | 3 held-out | 9/9 | **0.89 (0.71–1.01)** | load-dotenv 0.71, expr-is-missing 0.89, res-set 1.01 | |
| iteration 1 held-out (`b6d055d`) | Claude | 3 held-out | 12/12 | 1.04 (1.03–1.05), B never called CCM | | 0.67 (0.60–1.53) |
| iteration 1 tuned (`b6d055d`) | Codex | 3 of 4 (Django stopped) | 10/10 | 1.20 (0.64–1.21) | ensure_sync 0.64, req-get 1.21, with-bound 1.20 | |
| iteration 1 tuned (`b6d055d`) | Claude | 3 of 4 (Django stopped) | 12/12 | 1.05 (0.80–1.69), B never called CCM | | 1.42 (0.50–1.45) |
| iteration 2 tuned (`c0228ee`) | Codex | 4 tuned | 8/8 | **0.69 (0.30–0.87)** | django escape 0.30, req-get 0.55, with-bound 0.83, ensure_sync 0.87 | |
| iteration 2 held-out (`c0228ee`) | Codex | 4 held-out | 6/8 | 1.28 (0.80–1.49) | django make_aware 0.80, deserialize-struct 1.16, sendfile 1.40, error-handler 1.49 | |
| iteration 2 tuned (`c0228ee`) | Claude | 4 tuned | 12/12 | 1.07 (0.61–1.28), B never called CCM | | 1.00 (0.72–1.47) |
| iteration 2 held-out (`c0228ee`) | Claude | 4 held-out | 12/12 | 1.10 (1.05–2.42), B never called CCM | | 1.00 (0.74–1.42) |
| iteration 3 held-out (`85252fb`) | Codex | 4 held-out | 7/8 | 1.07 (0.53–1.42) | django get_random_string 0.53, effective-style 1.01, debug-flag 1.13, res-send 1.42 | |
| iteration 3 tuned (`85252fb`) | Codex | 4 tuned | 7/8 | 1.01 (0.51–1.73) | django make_aware 0.51, deserialize-struct 0.64, error-handler 1.38, sendfile 1.73 | |
| iteration 3 held-out (`85252fb`) | Claude | 4 held-out | 11/12 | 1.19 (1.04–1.35), B never called CCM | | 1.17 (0.49–1.55): get_random_string 0.49, res-send 0.86, effective-style 1.48, debug-flag 1.55 |
| iteration 3 tuned (`85252fb`) | Claude | 4 tuned | 12/12 | 1.09 (0.59–1.48), B never called CCM | | 1.08 (0.54–1.53): make_aware 0.54, error-handler 0.64, sendfile 1.52, deserialize-struct 1.53 |
| iteration 4 held-out (`ed0f4e8`) | Codex | 4 held-out | 8/8 | 1.60 (0.65–2.24) | locate-app-string 0.65, edit-wrap-field 1.59, django capfirst 1.60, edit-normalize-type 2.24 | |
| iteration 4 tuned (`ed0f4e8`) | Codex | 4 tuned | 8/8 | 1.04 (0.49–1.49) | django make_aware 0.49, deserialize-struct 0.71, res-send 1.37, error-handler 1.49 | |
| iteration 4 held-out (`ed0f4e8`) | Claude | 4 held-out | 16/16 | 1.09 (1.05–2.56), B never called CCM | | F 1.14 (0.86–1.28); H 1.49 (0.72–1.69) |
| iteration 4 tuned (`ed0f4e8`) | Claude | 4 tuned | 16/16 | 1.31 (0.64–2.10), B never called CCM | | F 1.59 (0.60–2.04); H 0.93 (0.64–1.13) |
| iteration 5 held-out (`9bb6564`) | Codex | 4 held-out | 7/8 | 1.05 (0.70–2.11) | django iri_to_uri 0.70, express locate-trust 1.04, serde types-deserializer 1.06, flask edit-method-route 2.11 | |
| iteration 5 tuned (`9bb6564`) | Codex | 4 tuned | 8/8 | **0.76 (0.45–0.99)** | django capfirst 0.45, serde edit-wrap-field 0.61, flask locate-app-string 0.91, express edit-normalize-type 0.99 | |
| iteration 5 held-out (`9bb6564`) | Claude | 4 held-out | 16/16 | 1.08 (0.32–1.48), B called CCM in 1 run | | F 0.92 (0.32–1.36); H 1.07 (0.33–1.40) |
| iteration 5 tuned (`9bb6564`) | Claude | 4 tuned | 16/16 | 1.04 (0.75–1.64), B never called CCM | | F 0.92 (0.64–1.32); H 1.13 (0.59–1.63) |
| iteration 5 held-out ×3 (`9bb6564`) | Codex | 4 held-out, 3 reps | 24/24 | 1.43 (0.61–1.77) | django iri_to_uri 0.61, express locate-trust 1.16, flask edit-method-route 1.69, serde types-deserializer 1.77 | |
| iteration 5 held-out ×3 (`9bb6564`) | Claude | 4 held-out, 3 reps | 48/48 | 1.06 (0.53–1.08), B called CCM in 3 of 12 runs (all Django) | django iri_to_uri 0.53, express 1.06, flask 1.06, serde 1.08 | F 0.99 (0.51–1.27); H 0.95 (0.53–0.97) |
| large-repository ×3 (`516ed4c` harness, `9bb6564` product) | Codex | 2 held-out, 3 reps | 12/12 | | django constant_time_compare 0.67, django urlencode trap 0.51 | |
| large-repository ×3 (`516ed4c` harness, `9bb6564` product) | Claude | 2 held-out, 3 reps | 24/24 | B called CCM in 3 of 6 runs (all on constant_time_compare) | django constant_time_compare 0.51, urlencode trap 1.08 | F 0.58, 1.10; H 0.43, 1.34 |
| iteration 7 (`e805025`) | Codex | Tokio 2 held-out + Django urlencode tuned, 3 reps | 18/18 | | tokio sleep_until 0.65, tokio catch_unwind trap 1.36; tuned django urlencode 0.30 (was 0.51) | |
| iteration 7 (`e805025`) | Claude | Tokio 2 held-out + Django urlencode tuned, 3 reps | 36/36 | B called CCM in 1 of 9 runs (tokio sleep_until) | tokio sleep_until 0.84, catch_unwind trap 1.30; tuned urlencode 0.91 | F 0.87, 1.33, 0.66 (urlencode was 1.10); H 0.77, 1.22, 0.91 |
| iteration 8 (`4c5f193`) | Codex | Tokio 2 held-out + catch_unwind tuned, 3 reps | 18/18 | | tokio async_op 2.28, tokio yield_now trap 1.46; tuned catch_unwind 1.29 (was 1.36) | |
| iteration 8 (`4c5f193`) | Claude | Tokio 2 held-out + catch_unwind tuned, 3 reps | 34/36 | B called CCM in 2 of 9 runs | tokio async_op 1.02, yield_now trap 0.98; tuned catch_unwind 1.17 | F 1.01, 0.91, 0.93; H 0.86, 0.65, 1.12 |
| iteration 9 (`31502d6`) | Codex | Tokio 2 held-out + async_op tuned, 3 reps | 18/18 | | tokio with_scheduler 0.83, poll_proceed 1.10; tuned async_op 0.85 (was 2.28) | |
| iteration 9 (`31502d6`) | Claude | Tokio 2 held-out + async_op tuned, 3 reps | 36/36 | B called CCM in 7 of 9 runs | tokio with_scheduler 0.93, poll_proceed 1.18; tuned async_op 0.50 (was 1.02) | F 0.72, 1.58, 0.63; H 1.06, 1.25, 0.75 |

Iteration 8 (`iter8-20261110/`, reading rule in its `PLAN.md`): the saving held for no agent and
task. Tokio `async_op` is defined inside `cfg_trace! { … }` and is not in the index, so
`util/trace.rs:async_op` and `async_op` were answered with other functions' packets
(`async_trace_leaf`, which even said "call list complete", and `async_io`); Codex made six CCM calls
and then grepped (B 92–147 k against A 43–59 k). The preregistered gold of `tokio-trap-yield-now`
wrongly listed a call inside a `#[cfg(test)]` test (`signal/registry.rs:smoke`); it was corrected
after the runs (amendment 12) and the task's records were rescored from the stored answers with
`scripts/rescore.py`. With the preregistered gold, Codex failed all six runs of that task (each
correctly leaving the test out) and Claude 6 of 12; with the corrected gold, Codex passes all six
and Claude fails 2 (answers that included test functions). The original records and a report on
them are kept in `runs-preregistered-gold/` and `report-preregistered-gold.md`.

Iteration 9 (`iter9-20261110/`, reading rule in its `PLAN.md`): `async_op` is now indexed (#22)
and both agents answered it with one `context` call (Codex B 32 k ×3 against A 36–39 k, Claude B
19 k ×3 against A 31–44 k). On `with_scheduler` B runs exceeded the A mean (Codex 65 k against
52 k; Claude 38.6 k twice, without CCM, against 37.5 k); on `poll_proceed`, whose eleven callers are short trait methods
that a single grep already shows, B was above A for both agents. The hook answered 3 of 13 greps
(all on `async_op`).

Scoring caveat (all pilots): answers are scored per file and function name. Two methods with the
same name in one file count as one expected item (`RequestFactory.generic` and
`AsyncRequestFactory.generic` in the `urlencode` trap), and the class name in an answer is not
checked (`BaseModelAdmin.get_preserved_filters` passes for `ModelAdmin.get_preserved_filters`).
The preregistered scorer is kept so results stay comparable. An owner audit of all 2026-11-10
result sets (`owner-audit-20261110/`, `scripts/owner_audit.py`, edit tasks excluded) found 6 runs
that passed but name a wrong owner or miss a same-named copy: 4 Claude runs of the Django
`urlencode` trap (iterations 6 and 7) and 2 Codex runs of Tokio `async_op` (iteration 8). Success
rates for those cells would be lower under owner-aware scoring; token ratios are unaffected.

Per-run input tokens for iteration 7 (`iter7-20261110/`, reading rule in its `PLAN.md`): Tokio
`sleep_until` Codex A 51 k, 80 k, 53 k and B 44 k, 43 k, 33 k; Claude A 26–38 k and B 19 k (with
CCM), 28 k, 39 k (without). Tokio `catch_unwind` trap Codex A 47–60 k and B 70–71 k: its first query
(`tokio/src/sync/task/atomic_waker.rs:catch_unwind`) routed to an unrelated test function, and the
check called ten `panic::catch_unwind(` / `std::panic::catch_unwind(` calls outside call sites, so
Codex grepped and read anyway. The hook answered none of 11 greps (4 packets not complete, 4 greps
with `glob`/`type`/`-i`, 2 on a single file, 1 not a single symbol).

Per-run input tokens for the large-repository set (`iter6-20261110/`, reading rule in its `PLAN.md`):
`constant_time_compare` Codex A 48 k, 46 k, 48 k and B 32 k ×3; Claude A 34–44 k and B 17 k, 17 k,
28 k. `urlencode` trap Codex A 90 k, 113 k, 142 k and B 47 k, 60 k, 69 k; Claude A 27 k, 49 k, 54 k
and B 40 k, 40 k, 60 k without any CCM call. In the trap the packet says "NOT complete" because two
`querystring.urlencode(...)` / `query_dict.urlencode()` method calls on local objects count as
outside call sites; Claude F then greps and reads anyway.

Per-run input tokens for the repeated set (`iter5-reps-20261110/`, decision rule written in its
`PLAN.md` before the runs): Django `iri_to_uri` Codex A 47 k, 50 k, 64 k and B 32 k, 33 k, 32 k;
Claude A 35 k ×3 and B 18 k ×3 (one `context` call each). Codex B on Flask `edit-method-route`
110 k, 94 k, 73 k against A 43–64 k, and on Serde `types-deserializer` 54–57 k against A 31–32 k.

Iteration 5, Claude per task (input tokens / A). Held-out: django iri_to_uri B 0.32, F 0.32,
H 0.33; express locate-trust B 1.08, F 0.72, H 1.06; flask edit-method-route B 1.09, F 1.36,
H 1.08; serde types-deserializer B 1.48, F 1.13, H 1.40. Tuned: django capfirst B 0.75, F 0.64,
H 0.59; express edit-normalize-type B 0.87, F 0.76, H 0.68; flask locate-app-string B 1.64,
F 1.09, H 1.63; serde edit-wrap-field B 1.22, F 1.32, H 1.58.

Iteration 4, Claude per task (input tokens / A). Held-out: django capfirst B 2.56, F 1.28, H 1.64;
express edit-normalize-type B 1.05, F 1.15, H 1.34; flask locate-app-string B 1.08, F 1.12, H 1.69;
serde edit-wrap-field B 1.11, F 0.86, H 0.72. Tuned: django make_aware B 1.09, F 0.60, H 0.78;
express res-send B 1.53, F 2.04, H 1.07; flask error-handler B 0.64, F 1.21, H 0.64; serde
deserialize-struct B 2.10, F 1.97, H 1.13.

**Arm H (opt-in Grep hook, iteration 4).** The hook ran on 9 greps in 8 H runs, answered none
and let all 9 through: 4 patterns were not a single symbol or were definition searches, 3 packets
were longer than the grep output they would replace (e.g. Django `make_aware`: packet 6,065 chars,
grep 2,455), 2 packets did not claim a complete list. Its latency on Django was 0.64–1.26 s
(median 0.88 s, 5 calls on the copied embedding index; one cold call in a run took 6.2 s). H
therefore measured B plus noise; its numbers are not evidence about the hook.

**Arm H, iteration 5 (compact hook answers, PR #14).** The hook ran on 11 greps in 8 H runs and
answered 1 (Django `iri_to_uri`, 948 chars, 680 ms); it let 10 through (4 patterns not a single
symbol, 1 definition search, 3 answers not shorter than the grep, 1 packet not complete, 1 index
with pending changes after an edit). In that H run Claude had already called `context` itself and
stopped after the hook's answer. One answered grep is not enough to measure the hook; H in
iteration 5 is again mostly B plus noise. In the three repetitions the hook saw 21 greps and
answered the 3 Django `iri_to_uri` greps (≤ 671 ms); 9 patterns were alternations or regexes, 5
answers would not have been shorter, 2 were definition searches, 2 came right after an edit. Offline, on all 119 recorded greps, the hook answers 16
with 35 % less text (`hook-replay-20261110/`).

Results, reports, run records and task selections are in `benchmarks/results/agent/`
(`heldout-20261110/`, `pilot-*-20261110/`, `iter2-20261110/`, `iter3-20261110/`,
`iter4-20261110/`, `iter5-20261110/`, `iter5-reps-20261110/`, `iter6-20261110/`, `iter7-20261110/`, `iter8-20261110/`,
`iter9-20261110/`, `hook-replay-20261110/`, `owner-audit-20261110/`); transcripts stay
local. Failures kept in the records:

- Iteration-1 tuned sets stopped at the Django task: the frozen harness rebuilt the index with local
  embeddings in every run (about an hour per Django run under load) and the session's 2-hour limit
  stopped it (`INCOMPLETE.md`). Since `c0228ee` the harness builds each repository's index once
  (Django: 1,349 s) and copies it into every run.
- The disk filled up during iteration 1; one Claude held-out run crashed before its agent started
  and was resumed from the frozen commit. No measured run was lost.
- `express-trap-sendfile-helper` failed in both Codex arms the same way: the agent read "not the
  methods res.sendFile or res.sendfile" as excluding the callers; the packet was correct.
- Iteration 3: Codex B missed `res.render` in `express-trap-res-send`: the call is `self.send(str)`
  through `var self = this`, which receiver evidence does not follow (the packet says aliases are
  not checked; 44 `examples/` call sites outside `lib/` buried that line). Both Claude A and B missed
  the async `_aget_new_session_key` in `django-callers-get-random-string`; F found it.
- Iteration 4 had no failed runs (48/48 correct). The release build and the index-cache build were
  killed twice by the supervising session's background limits and once the host was saturated by
  other work (load average up to 255; the Django index took 4,500 s instead of 1,349 s), so wall
  times in iteration 4 are not comparable; token counts are unaffected.
- Iteration 5: Codex B in `express-edit-normalize-type` left out `normalizeTypes` on purpose
  ("excluding the normalizeTypes call chain") while using only `rg`, no CCM; a misread question.
  Iteration 5 ran from a fresh clone in `~/Documents/ccm-work` with the corpus fetched again at
  the same pinned commits, because the original checkout under `~/Desktop` became unreadable to
  the session.
- After an edit the first `context` answer can still be stale: in iteration 4 the single-file
  refresh (which re-embeds the changed file's nodes) exceeded the 10 s wait under a host load of
  90–255, and Codex then called `index_project` and `context` again. On an idle host the same edit
  refreshes in 0.92 s and the first answer is fresh. Publishing the graph before the embeddings
  would remove this, but it is a core change and was not made.
- One iteration-1 graph-only `context` query about `res.set` was seen by the supervising agent while
  the held-out runs were already running (disclosed in `heldout-20261110/SELECTION.md`).

What the transcripts show:

- **Codex chooses CCM on its own** in every B run that asks a structural question and, since the
  grep-equivalent check, often stops after one `context` call (Flask `ensure_sync`, Express
  `req.get`, Serde `expr_is_missing`, Django `make_aware`). The large-repository case is where it
  pays most: Django `escape` A read 259,771 input tokens (one `rg` returned 146 KB), B 78,475.
  Iteration 3's Rust rule turned `deserialize_struct` from 1.16 into a one-call 0.64.
- **Codex losses have generic causes, fixed in iteration 3 unless noted:** a stale first answer right
  after an edit (2 s freshness budget), Rust method calls of other types counted as missing call
  sites, very long bodies and callee lists in the packet, and a misread question (not a CCM issue).
- **Iteration 4 (review fixes, `self` aliases):** Codex now finds `res.render` through
  `var self = this`, but the packet still says "NOT complete" because 44 `res.send(` calls in
  anonymous callbacks under `examples/` lie outside a question that asks about `lib/`. In the held-out
  Django trap (`capfirst`) the review fix made the headline conditional ("complete if the graph is
  right about 2 lines"), although those two lines sit in a file that imports `capfirst` from the
  other module; Codex re-checked with `rg` and a natural-language query with the dotted path inside
  routed to an unrelated 12 KB `Engine` packet. Edit tasks remain more expensive for Codex B
  (1.49–2.24): it edits first and then asks CCM to confirm.
- **Iteration 5 (proved imports, Python `test/` packages, several-target queries):** the Django
  `capfirst` packet now says "call list complete" and matches the gold; Codex B answered the
  tuned trap with `context` alone (B/A 0.45, was 1.60). Its remaining losses are edit tasks it
  solves with `rg` and `sed` (flask edit-method-route 2.11).
- **Claude Code calls CCM on its own only on Django caller questions** (7 of 59 B runs on
  2026-11-09 and 2026-11-10, all on held-out Django `iri_to_uri` and `constant_time_compare`: one
  `context` call and no grep, 17–28 k input tokens against 34–57 k for A), with the tool loaded, a usage note, short server instructions and
  `anthropic/alwaysLoad`. Its
  Grep tool caps output, so its baseline is cheaper than Codex's. When forced (F), it always
  re-checks the answer with one or more Greps; F saved tokens on some tasks and cost more on others.

## Next steps, in order

1. **The final result is in** (`final-l3-20261110/README.md`). What remains open is the
   large-repository result of the pilots. Testing it needs a preregistered set of large-repository
   caller tasks (the Django and Tokio pilot tasks are tuned) with several repetitions; it is not
   started without the owner's decision and quota approval.
2. **Unknown names and macro bodies: done in #21 and #22.** Tokio `async_op` went from B/A 2.28 to
   0.85 (Codex) and 1.02 to 0.50 (Claude); all 441 functions in Tokio's item-level macro bodies
   are now indexed.
3. **Rust path targets and standard-library calls: done in #19.** Not yet shown to help: on the
   tuned Tokio trap Codex went from 1.36 to 1.29 and Claude B stayed above A.
4. **Python method calls on other objects: done in #17.** Calls `x.name(` on objects that no import
   binds are listed under their own headline instead of making the check "NOT complete"; on the
   tuned `urlencode` trap Codex B/A went from 0.51 to 0.30 and Claude F from 1.10 to 0.66.
5. **Small repositories: documented limit.** In the repeated Codex runs B costs more than A on
   Flask edits (1.69) and Serde trait implementors (1.77) in all three repetitions. The transcripts
   show why: A answers with one or two `rg` calls; B makes the same calls plus a CCM call (a
   natural-language query that missed on Serde, a confirmation after the edit on Flask), and each
   extra turn re-sends the whole context (about 15–20 k input tokens with cache). CCM can only save
   where its answer replaces at least one search turn, which on these repositories it rarely does.
6. **Hook alternations: not pursued.** Of the 49 recorded greps the hook could not parse, 10 are one
   tuned task's `_find_error_handler|def handle_exception`; the rest are varied regexes (alias and
   import checks, receivers, look-arounds). Supporting them would fit one task, not a pattern.
7. Release the fixes (npm launcher roots bug, refresh retry) as 0.5.1 after owner approval.
8. The preregistered L3 final run: done (`final-l3-20261110/`).

## Running the L3 experiment

Prerequisites: the corpus (`bash benchmarks/scripts/fetch_corpus.sh`), release binaries
(`cargo build --release -p ccm-cli -p ccm-mcp`, 20–45 minutes; use a separate
`CARGO_TARGET_DIR` if a pilot is running from `target/release`) and the built-in embedding model
(`~/.ccm-bench/models` on this host). Verify the binaries against the pilot packets first; this
disables embeddings and makes no model call:

```bash
cd benchmarks
uv run python -m agent preflight-context --ccm-bin-dir <bin dir> --corpus-dir <corpus>
```

Claude Code (subscription token from `claude setup-token`, only in the invoking environment) and
Codex (`CODEX_HOME=~/.ccm-bench/codex-home codex login` once; pass the native binary):

```bash
cd benchmarks
CLAUDE_CODE_OAUTH_TOKEN=... uv run python -m agent run --agent claude \
  --agent-bin "$(command -v claude)" --auth subscription --profile context-first-pilot \
  --model claude-opus-5-5 --effort high --embedding local --max-budget-usd 2 --max-total-usd 40 \
  --timeout-s 1800 --repetitions 1 --tasks <ids> --arms A,B,F \
  --ccm-bin-dir <bin dir> --ccm-model-dir ~/.ccm-bench/models --corpus-dir <corpus> \
  --index-cache-dir ~/.ccm-bench/index-cache --out-dir results/agent/<name>/claude
CODEX_BIN="$(dirname "$(readlink -f "$(command -v codex)")")/../node_modules/@openai/codex-darwin-arm64/vendor/aarch64-apple-darwin/bin/codex"
uv run python -m agent run --agent codex --agent-bin "$CODEX_BIN" \
  --codex-home ~/.ccm-bench/codex-home --auth subscription --profile context-first-pilot \
  --model gpt-6-luna --effort high --embedding local --max-budget-usd 2 --max-total-usd 40 \
  --timeout-s 1800 --repetitions 1 --tasks <ids> --arms A,B \
  --ccm-bin-dir <bin dir> --ccm-model-dir ~/.ccm-bench/models --corpus-dir <corpus> \
  --index-cache-dir ~/.ccm-bench/index-cache --out-dir results/agent/<name>/codex
uv run python -m agent report --out-dir results/agent/<name>/codex --out results/agent/<name>/codex/report.md
```

Long sets outlive a 2-hour supervising session: start the run detached (`nohup … &`) or pre-build a
large repository's index cache before the run. Exit codes are listed in `run --help` (3 total cap,
4 usage or rate limit, 5 isolation, 6 authentication, 7 setup failure; nothing is recorded for
4–7). A measured run refuses to start with uncommitted changes under `benchmarks/agent`, and one
out-dir accepts one harness process at a time. For the preregistered final run omit `--profile`,
use `--tasks all --repetitions 3` and a new out-dir.

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
- `target/` grows past 60 GB and the disk on this host is small (it filled up once during the
  2026-11-10 pilots); `cargo clean -p ccm-core -p ccm-mcp -p ccm-cli` frees most of it, and a
  pilot's release binaries can live in a separate `CARGO_TARGET_DIR` so builds do not replace them.

## Rules for agents working here

- No unmeasured claims in docs, READMEs or release notes; a measurement names its commit.
- Local commits are fine. Pushes, tags, releases, npm or registry publishing, external AI
  benchmark quota use and any contact with other projects need the owner's explicit approval
  for that piece of work (the 2026-11-10 task approved branch + PR and its pilots).
- Product pilots keep a held-out task set: tasks whose CCM packets nobody looked at before the
  run, chosen and written down (`SELECTION.md`) before it starts, reported apart from tuned tasks.
  A held-out task becomes tuned as soon as its transcript is read.
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
