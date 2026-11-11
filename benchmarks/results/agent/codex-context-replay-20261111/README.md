# Codex's extra turns on Tokio and "which files" tasks: root causes and offline replay

Tuned, not held out. These transcripts (Codex D runs of `heldout17-20261111/` and
`heldout18-20261111/`) informed the fix. No agent was run.

## What the transcripts show

**Tokio, `tokio-callers-globals` (D/A 2.13 in amendment 17).** The first `context` packet was
complete and correct: six callers, six call sites. Two of its caller entries, however, showed
the same line twice and then "+2 more":

```
- Function: signal_enable · tokio/src/signal/unix.rs:256-293 · calls
  268: let globals = globals();
  268: let globals = globals();
  +2 more
```

`signal_enable` binds a local variable named `globals`. The packet printed one line per
occurrence of the name, so the call and the variable each appeared, and the variable's later
uses showed up as two hidden "more" sites. Codex then asked for "complete caller source and
every globals() call" and for each caller separately: six extra `context` calls in D#1 and
four in D#2. Each extra turn re-sends the whole context.

**Tokio, the trap `Registration::new_with_interest_and_handle` (1.34).** `PollEvented` has a
method with the same name. The query `Registration::new_with_interest_and_handle` was answered
with "is not in the index … the index has no symbol there (for example a definition inside a
macro body)", although both definitions are indexed. The `Registration::` qualifier was not used
to choose between them, so the query fell through to a text search with a false message. In
D#3, Codex then asked about each call site by `path:line`: five calls instead of one.

**"Which files call X" (Codex D/A 1.30–1.70 in amendment 18, 0.88–0.99 in amendment 17).**
The packet was complete and correct, but it answers "which functions call X". The file list
had to be derived from caller lines that mix calls, a non-call reference (`unordered_list`
stores `conditional_escape`) and "+ N files that import it". After the packet, Codex ran `rg`
over the package in 7 of the 12 impact D runs: all 6 in amendment 18, 1 of 6 in amendment 17.
It answered from that. The packet and the `rg`
output together cost more than A's single `rg`.

## The fix (`context`, so D only; A, B and C unchanged)

1. A caller entry lists each call-site line once. Call hits come first; non-call uses appear
   only when the caller has no call. "+N more" counts only further distinct call lines.
2. `Type::name` picks the definition whose enclosing type, or whose file module, is `Type` when
   several definitions share the name. A name that still matches several definitions gets the
   candidate list instead of "not in the index".
3. When the call list is complete, the packet adds one line: `Files with calls (N): a, b, …`.
   It lists the files of the proven calls, without references, imports or `may call`.

New e2e test `context_lists_each_call_site_line_once_and_resolves_type_qualified_rust_names`.
It fails on the first assertion without the fix: the line is printed twice.

## Replay

`scripts/context_replay.py` re-asked all 38 recorded `context` queries of these runs (the three
Tokio tasks of amendment 17 and the four impact tasks of both sets). It used the measured
product (`ad29ffc`) and this branch, on graph-only indexes.

| | `ad29ffc` | this branch |
|---|---|---|
| Packets with a repeated call-site line | 3 (the `globals` first packets) | 0 |
| Queries answered "not in the index" | 1 (`Registration::new_with_interest_and_handle`) | 0 |
| Impact queries whose `Files with calls` line equals the gold file list | – | 15 of 15 |

- **Trap query:** `Registration::new_with_interest_and_handle` now returns the `registration.rs`
  definition with the four gold callers. Its check is conditional ("complete if the graph is
  right about 1 line"), so it gets no files line.
- **Packet size:** packets grow by 30–380 characters (the files line).
- **Unchanged:** no other query changed its kind.

## Held-out outcome (amendment 19)

The held-out run (`heldout19-20261111/`) found that the files line left out files whose only
calls are at module level (`UserModel = get_user_model()`). That made all three D runs of
`django-impact-get-user-model` fail. The line was removed after the run. Changes 1 and 2 are
kept.

## What this does not show

The replay shows the packets. It cannot show whether Codex stops asking or stops running `rg`.
That is measured on new held-out tasks (amendment 19). With the current task rules, Tokio's
selection pools are empty (amendment 18). So the Rust part of the fix is supported only by the
e2e test and this tuned replay, not by a held-out run.
