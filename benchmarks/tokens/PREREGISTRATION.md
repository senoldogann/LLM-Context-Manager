# Token cost of answers: pre-registration

Fixed on 2026-10-02, before the first measured run. Changes after the first run
go under [Deviations](#deviations).

## Question

How many tool calls and how many response bytes does an agent need to get the
same answer from CCM, before (v1) and after (v2) the M2 changes (compact
budgeted output, `target`, `explain`, `map`)?

## What is measured

- Per question: the number of MCP tool calls and the total bytes of their text
  responses. Estimated tokens = bytes / 4 (an estimate, not a tokenizer count).
- Fixed overhead per session: `tools/list` response bytes and `SKILL.md` bytes.
- Not measured: agent success, real model token counts or cost (that is the L3
  agent benchmark).

## Versions

- v1: commit `fd9af8b` (v0.3.13 + M1), debug binaries built from it.
- v2: the M2 head. The commit is recorded in the results file.

## Questions

From `questions.json`, for Flask 3.0.3 and Django 5.1, graph only
(`CCM_DISABLE_EMBEDDER=1`), auto-refresh off, isolated `HOME`, every argument
not named below at its default:

| Kind | v1 tool plan | v2 tool plan |
|---|---|---|
| `callers(symbol)` | `find_nodes(query=symbol, limit=50)`, then `find_usages(node_id)` for each result whose name equals the symbol | `find_usages(target=symbol)`; on an ambiguity error, `find_usages(target=candidate)` for each listed candidate |
| `explain(symbol)` | the same `find_nodes`, then `read_graph(node_id)` and `get_context(file, start_line, include_body=true)` for each exact match | `explain(target=symbol)`; on an ambiguity error, `explain(target=candidate)` for each candidate |
| `impact(file)` | `impact_of_change(file)` | `impact_of_change(file)` |
| `map` | — | `map()` |

At most `max_matches` (10) matches or candidates per symbol are followed, in
the order the tool lists them.

## Parity (honesty check)

For `callers`, the share of v1 caller locations (`path:start_line`) that also
appear in the v2 answers is reported per question and overall. A share below 1
means the v2 default budget left callers out; it is published, not hidden.

## Deviations

None yet.
