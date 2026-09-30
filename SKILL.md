---
name: context-manager
description: "CCM: a code graph of the project served over MCP. Answers structural questions in one call with compact `path:line` lines: project map, explain a symbol, who uses it, what breaks if a file changes. WHEN: starting work in a codebase, before reading or editing a symbol, before changing a file, or whenever you would otherwise grep and open many files."
origin: https://github.com/senoldogann/LLM-Context-Manager
terminal_state: invoke_tool_chain("map", then explain/find_usages/impact_of_change based on task)
version: 2.0
---

# Cognitive Codebase Matrix (CCM)

CCM indexes the project into a code graph (tree-sitter symbols plus call, import
and inheritance edges) and answers with one line per result:
`- Kind: name · path:start-end · relation`. Any `path:line` can be passed back
as a `target`.

## Workflow

1. `map` once per task: the most-used files and their key symbols.
2. `explain {target}` before reading a symbol: definition, body, members,
   callers, callees and tests in one call.
3. `find_usages {target}` for every user of a symbol; `impact_of_change {file}`
   before editing a file.
4. Read only the lines you will edit. Do not open whole files that `explain`
   already summarised.

## Which tool

| Question | Tool |
|---|---|
| What is in this project and what matters most? | `map` (`path` zooms into a directory) |
| What does `X` do, who calls it, what does it call, which tests cover it? | `explain {target}` |
| Who uses `X`? | `find_usages {target}` |
| What breaks if I change this file? | `impact_of_change {file}` |
| How does `a` reach `b`? | `trace_call_chain {from, to}` |
| Where is the code that does …? (no symbol name yet) | `search_code {query}` |
| Where is a symbol called …? | `find_nodes {query}` |
| What changed recently? | `diff_context {days}` |
| The index is missing or stale | `index_project` (background) or `index_now` (waits) |

`target` is a name (`run`, `Engine.start`), a file path, `path:line` (a
result's `path:start-end` works too) or a node ID. An ambiguous name returns up
to 10 candidates as `path:line Kind name`; pass one back as it is. An unknown
target is an error, never an empty result.

## Relations

Usages are labelled `calls` (resolved through imports, local definitions,
`self` or `super`), `calls (inferred …)` (the only project definition of that
name, not imported), `may call` (receiver type unknown or several candidates),
`references`, `imports`, `may import` and `inherits`. Treat `may …` lines as
candidates to verify, not facts. Python is resolved at syntax level; other
languages are matched by name.

## Budgets

Graph tools take `max_tokens` (default 1500, `map` 1000, at most 20000; about
four characters per token). A list that does not fit ends with `… n more`;
raise `max_tokens` or narrow the query only when you need the rest.
`include_body` adds code; only `explain` includes it by default, capped at half
of the budget.

## Freshness

Every answer starts with the index state, for example
`_Index: auto-refresh on · fresh_` or `_Index: auto-refresh off · indexed 3s ago_`.
If it says `stale`, the answer may predate your last edit: call `index_project`
or ask again after the refresh.

## Project setup

Add this to the project's `CLAUDE.md` or `AGENTS.md`:

```markdown
## Code navigation
Use the context-manager MCP tools before reading files: `map` once, then
`explain` or `find_usages` for symbols, and `impact_of_change` before editing a
file. Read only the lines you edit.
```

Optional Claude Code hook that puts the map in context when a session starts
(`.claude/settings.json`; `ccm-cli map` does the same when the CLI is
installed):

```json
{
  "hooks": {
    "SessionStart": [{
      "hooks": [{
        "type": "command",
        "command": "npx -y @senoldogann/context-manager map --path \"$CLAUDE_PROJECT_DIR\" --max-tokens 800"
      }]
    }]
  }
}
```

`npx @senoldogann/context-manager install` (0.4.0 or later) configures the MCP
server for Claude Code, Codex, Cursor, Claude Desktop and Antigravity.
