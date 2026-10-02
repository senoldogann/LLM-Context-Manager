---
name: context-manager
description: "CCM: a code graph served over MCP. Use `context` as the default entry point for codebase questions and changes; CCM chooses structural graph retrieval or hybrid search server-side and returns a compact task packet."
origin: https://github.com/senoldogann/LLM-Context-Manager
terminal_state: invoke_tool("context", with the task or code question)
version: 3.0
---

# Cognitive Codebase Matrix (CCM)

CCM indexes the project into a code graph (tree-sitter symbols plus call, import
and inheritance edges) and a semantic index. The normal agent workflow has one
entry point:

```text
context({ query: "what you need to understand or change" })
```

Give `context` a symbol name, file/path:line, or a natural-language task. CCM
does the routing instead of asking the model to choose among navigation tools:

- exact symbol/path targets use deterministic graph explanation;
- natural-language tasks use hybrid retrieval, then enrich the best result with
  definition/body/members/callers/callees/tests in the same call;
- answers stay inside `max_tokens` (default 1500).

Read only the lines you still need to edit or verify after the returned packet.
Do not start a task with a project-wide map unless the task genuinely requires
one.

## Advanced / compatibility tools

The older structural tools remain useful as explicit diagnostics and for clients
that already depend on them:

| Need | Tool |
|---|---|
| Project overview | `map` |
| Explain one known symbol | `explain` |
| Every usage of one symbol | `find_usages` |
| File blast radius | `impact_of_change` |
| Call path between two symbols | `trace_call_chain` |
| Search without task-packet enrichment | `search_code` |
| Find symbols by name/path | `find_nodes` |
| Recently changed code | `diff_context` |
| Refresh a stale/missing index | `index_project` or `index_now` |

These are primitives, not a tool-selection workflow the model is expected to
memorize. They stay directly callable for compatibility, but the default
`tools/list` surface exposes only `context` plus index/recovery tools. Set
`CCM_MCP_LEGACY_TOOLS=1` only for advanced clients that need the full legacy
surface.

## Relations and uncertainty

Usages are labelled `calls` (resolved through imports, local definitions,
`self` or `super`), `calls (inferred …)`, `may call`, `references`,
`imports`, `may import` and `inherits`. Treat `may …` lines as candidates
to verify, not facts. Python and Rust are resolved at syntax level; other
languages still rely more heavily on name matching.

## Freshness

Every read answer starts with index state, for example
`_Index: auto-refresh on · fresh_` or `_Index: auto-refresh off · indexed 3s ago_`.
If it says `stale`, call `index_project` or retry after auto-refresh. Missing
indexes fail explicitly instead of triggering a hidden rebuild.

## Project setup

A minimal agent note is enough:

```markdown
## Code navigation
Use context-manager `context` first for codebase questions or before multi-file
changes. Give it the task in natural language; read additional source only when
the returned packet is insufficient.
```

Do not inject a project map at session start by default: the 2026-10-02 Claude
pilot showed that unused CCM context has a real token cost. Any extra startup
context or broader tool surface must earn its cost in an agent benchmark.

`npx @senoldogann/context-manager install` configures the MCP server for Claude
Code, Codex, Cursor, Claude Desktop and Antigravity.
