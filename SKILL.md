---
name: context-manager
description: "Cognitive Codebase Matrix (CCM) MCP skill for deep codebase intelligence. Gives AI agents a queryable knowledge graph over any project: semantic hybrid search, call chain tracing, blast-radius analysis, and cursor-aware context retrieval — all via 9 MCP tools. WHEN: understand a large codebase, find callers/callees, map blast radius, retrieve cursor context, trace a call chain, inspect a graph node, get recently changed code, or index a project before starting work."
origin: https://github.com/senoldogann/LLM-Context-Manager
---

# Cognitive Codebase Matrix (CCM)

> Transform any codebase into a queryable knowledge graph for AI agents.
> CCM combines Tree-sitter AST parsing, LanceDB vector search, and Petgraph graph traversal
> into 9 MCP tools that give agents surgical codebase intelligence.

## When to Activate

Load this skill when you need to:
- Navigate or understand a large, unfamiliar codebase
- Find all callers or callees of a function
- Assess the blast radius before editing a file
- Retrieve code context at a specific file + line position
- Trace the call path between two functions
- Inspect how graph nodes connect to each other
- See what files changed in the last N days (git-aware)
- Index or re-index a project before starting a task

Trigger phrases: "understand the codebase", "find usages of", "what calls X", "who depends on", "blast radius of", "context at line", "trace call chain", "index this project", "recently changed code", "read the graph", "find nodes named".

## MCP Setup

### One-line install (configures Codex, Cursor, Claude Desktop, Antigravity automatically)
```bash
npx @senoldogann/context-manager install
```

### Manual MCP config (VS Code Kilo / any MCP host)
```json
{
  "context-manager": {
    "command": "npx",
    "args": ["-y", "@senoldogann/context-manager", "mcp"],
    "env": { "RUST_LOG": "info" }
  }
}
```

### Minimal environment (~/.ccm/.env)
```ini
# Option A: Local inference (recommended, no API cost)
EMBEDDING_PROVIDER=ollama
EMBEDDING_HOST=http://127.0.0.1:11434
EMBEDDING_MODEL=mxbai-embed-large
EMBEDDING_API_KEY=ollama

# Option B: Cloud (OpenAI)
# EMBEDDING_PROVIDER=openai
# EMBEDDING_API_KEY=sk-your-key
# EMBEDDING_MODEL=text-embedding-3-small

# Security: restrict which projects the MCP server may access
CCM_ALLOWED_ROOTS=/Users/you/projects:/Users/you/sandbox
```

Ollama prerequisites:
```bash
ollama serve
ollama pull mxbai-embed-large
```

## Recommended Agent Flow

```
1. index_project          ← always run first on a new session / after edits
        ↓
2. search_code            ← semantic entry point — "find auth handling"
        ↓
3. get_context            ← cursor-based detail — file:line position
        ↓ (optional deep-dive)
4. find_nodes             ← locate a node by name when you know it
5. read_graph             ← inspect connections of a known node_id
6. find_usages            ← all callers of a node_id
7. trace_call_chain       ← BFS path from node A to node B
8. impact_of_change       ← blast radius before editing a file
9. diff_context           ← git-based recent changes
```

**Rule:** If the project has never been indexed in this session, call `index_project` before any other tool. It is incremental and fast on re-runs.

## Tool Reference

---

### 1. `index_project`
Index or refresh the code graph for a project. Safe to call repeatedly — performs incremental updates.

**Parameters**

| Parameter | Type | Required | Notes |
|-----------|------|----------|-------|
| `project_path` | string | yes | Absolute path to the project root |

**Example**
```json
{
  "name": "index_project",
  "arguments": {
    "project_path": "/Users/dev/my-app"
  }
}
```

**When:** Always call before the first query in a new session, or after editing files.

---

### 2. `search_code`
Hybrid semantic + graph search. Combines vector similarity and graph centrality for ranked results with explanations.

**Parameters**

| Parameter | Type | Required | Default | Notes |
|-----------|------|----------|---------|-------|
| `query` | string | yes | — | Natural language or code term |
| `project_path` | string | yes | — | Absolute path to project root |
| `limit` | integer | no | 5 | Max results to return |

**Example**
```json
{
  "name": "search_code",
  "arguments": {
    "query": "authentication token validation",
    "project_path": "/Users/dev/my-app",
    "limit": 8
  }
}
```

**Output:** Ranked list of code snippets with score, reason, node metadata (file, type, line range, node_id).

**When:** Starting point for any exploration. Prefer over `get_context` when you don't know the exact file/line.

---

### 3. `get_context`
Cursor-aware context retrieval. Given a file path and line number, returns the most relevant surrounding code with graph-aware context.

**Parameters**

| Parameter | Type | Required | Notes |
|-----------|------|----------|-------|
| `file` | string | yes | Relative path from project root (e.g. `src/main.rs`) |
| `line` | integer | yes | 1-based line number |
| `project_path` | string | yes | Absolute path to project root |

**Example**
```json
{
  "name": "get_context",
  "arguments": {
    "file": "core/src/lib.rs",
    "line": 100,
    "project_path": "/Users/dev/my-app"
  }
}
```

**When:** Use when your cursor is at a specific line and you want context — perfect for editor integrations or after viewing a diff.

---

### 4. `find_nodes`
Find graph nodes by name, file path fragment, or node ID fragment. Returns matching nodes with metadata and node_ids for use in other tools.

**Parameters**

| Parameter | Type | Required | Default | Notes |
|-----------|------|----------|---------|-------|
| `query` | string | yes | — | Function name, file path, or node ID fragment |
| `project_path` | string | yes | — | Absolute path to project root |
| `limit` | integer | no | 10 | Max results |

**Example**
```json
{
  "name": "find_nodes",
  "arguments": {
    "query": "update_index",
    "project_path": "/Users/dev/my-app",
    "limit": 5
  }
}
```

**When:** You know a function/class name and want its node_id to feed into `read_graph`, `find_usages`, or `trace_call_chain`.

---

### 5. `read_graph`
Inspect a specific node's full details and its direct graph connections (calls, called_by, contains).

**Parameters**

| Parameter | Type | Required | Notes |
|-----------|------|----------|-------|
| `node_id` | string | yes | Exact node ID from `find_nodes` or `search_code` output. Format: `./path/to/file.rs:node_type:line:col` |
| `project_path` | string | yes | Absolute path to project root |

**Example**
```json
{
  "name": "read_graph",
  "arguments": {
    "node_id": "./core/src/lib.rs:function_item:310:0",
    "project_path": "/Users/dev/my-app"
  }
}
```

**Output:** Node name, type, line range, source content, plus `Calls`, `Called By`, `Contains` edge lists.

**When:** You have a node_id and want to understand exactly what it connects to in one call.

---

### 6. `find_usages`
Find all callers of a given node (reverse-edge traversal). Answers "who uses this function/class?".

**Parameters**

| Parameter | Type | Required | Notes |
|-----------|------|----------|-------|
| `node_id` | string | yes | Exact node ID |
| `project_path` | string | yes | Absolute path to project root |

**Example**
```json
{
  "name": "find_usages",
  "arguments": {
    "node_id": "./core/src/lib.rs:function_item:310:0",
    "project_path": "/Users/dev/my-app"
  }
}
```

**When:** Before removing, refactoring, or changing the signature of a function. Know every caller first.

---

### 7. `trace_call_chain`
BFS path search between two nodes. Finds how node A eventually calls node B through the call graph.

**Parameters**

| Parameter | Type | Required | Default | Notes |
|-----------|------|----------|---------|-------|
| `from_id` | string | yes | — | Starting node ID |
| `to_id` | string | yes | — | Target node ID |
| `project_path` | string | yes | — | Absolute path to project root |
| `max_depth` | integer | no | 6 | BFS traversal depth limit |

**Example**
```json
{
  "name": "trace_call_chain",
  "arguments": {
    "from_id": "./cli/src/main.rs:function_item:1:0",
    "to_id": "./core/src/engine.rs:function_item:50:0",
    "project_path": "/Users/dev/my-app",
    "max_depth": 8
  }
}
```

**When:** Debugging an execution path. "How does the CLI entry point reach the embedding engine?"  
**Note:** Returns empty if no path exists within `max_depth`. Try increasing depth or verifying both node_ids with `find_nodes`.

---

### 8. `impact_of_change`
Compute the blast radius of changing a file. Returns all nodes and files that depend on it, transitively.

**Parameters**

| Parameter | Type | Required | Notes |
|-----------|------|----------|-------|
| `file` | string | yes | Relative path from project root |
| `project_path` | string | yes | Absolute path to project root |

**Example**
```json
{
  "name": "impact_of_change",
  "arguments": {
    "file": "core/src/lib.rs",
    "project_path": "/Users/dev/my-app"
  }
}
```

**When:** Before editing a core file. Understand the ripple effect before you touch anything.

---

### 9. `diff_context`
Returns code context for recently changed files using git history. Ranks by recency and change frequency.

**Parameters**

| Parameter | Type | Required | Default | Notes |
|-----------|------|----------|---------|-------|
| `project_path` | string | yes | — | Absolute path to project root |
| `days` | integer | no | 7 | Look-back window in days |
| `limit` | integer | no | 10 | Max results |

**Example**
```json
{
  "name": "diff_context",
  "arguments": {
    "project_path": "/Users/dev/my-app",
    "days": 14,
    "limit": 20
  }
}
```

**When:** Starting a task on code you haven't touched in a while. "What changed recently?"

---

## Output Format

All tools return markdown-formatted text:

```
## <Title> (Score: 0.87)
**Reason:** <why this result was selected>
**Node ID:** ./path/to/file.rs:function_item:42:0
**File:** path/to/file.rs
**Node Type:** function_item
**Range:** 42-68

```rust
fn example() { ... }
```

---
```

**Key fields to extract from output:**
- `Node ID` — use this as `node_id` input for `read_graph`, `find_usages`, `trace_call_chain`
- `File` + `Range` — use as `file`/`line` input for `get_context`
- `Score` — hybrid relevance (higher = better match)
- `Reason` — CCM's explanation of why this result is relevant

## Node ID Format

Node IDs follow a rigid format:
```
./relative/path/to/file.ext:node_type:start_line:column
```

Examples:
- `./core/src/lib.rs:function_item:310:0`
- `./src/auth/token.py:class_definition:15:0`
- `./api/routes.ts:function_declaration:88:2`

Always use node_ids **exactly as returned** by tools. Do not guess or construct them manually.

## Common Mistakes

| Mistake | Correct Approach |
|---------|-----------------|
| Calling `search_code` without indexing first | Always call `index_project` first in each session |
| Constructing node_ids manually | Use `find_nodes` to discover exact node_ids |
| Using absolute paths in `file` param | Use relative paths (`core/src/lib.rs`, not `/Users/dev/my-app/core/src/lib.rs`) |
| Setting `max_depth` too low in `trace_call_chain` | Default is 6; increase to 10–15 for deep chains |
| Skipping `impact_of_change` before a refactor | Always check blast radius before editing core files |

## Supported Languages

Rust, Python, TypeScript, JavaScript, Go, Java, Kotlin, C#

## Advanced Configuration

For hybrid weight tuning, chunking controls, batch size, and embedding timeout, see `.env.example` in the repository or [`docs/hybrid-ranking.md`](https://github.com/senoldogann/LLM-Context-Manager/blob/main/docs/hybrid-ranking.md).

| Env Var | Default | Effect |
|---------|---------|--------|
| `CCM_HYBRID_GRAPH_WEIGHT` | 0.55 | Graph centrality weight |
| `CCM_HYBRID_SEM_WEIGHT` | 0.35 | Semantic/vector weight |
| `CCM_HYBRID_SPATIAL_WEIGHT` | 0.08 | File proximity weight |
| `CCM_HYBRID_RECENT_WEIGHT` | 0.02 | Recency weight |
| `CCM_ALLOWED_ROOTS` | _(empty)_ | Allowlist for MCP project access |
| `CCM_MCP_ENGINE_CACHE_SIZE` | 4 | Max projects held in RAM |
| `CCM_DISABLE_EMBEDDER` | 0 | Disable vector search entirely |

## Source

Repository: https://github.com/senoldogann/LLM-Context-Manager  
npm: `@senoldogann/context-manager`  
License: MIT
