---
name: context-manager
description: "Cognitive Codebase Matrix (CCM) MCP skill for deep codebase intelligence. Gives AI agents a queryable knowledge graph over any project: semantic hybrid search, call chain tracing, blast-radius analysis, and cursor-aware context retrieval — all via 9 MCP tools. WHEN: understand a large codebase, find callers/callees, map blast radius, retrieve cursor context, trace a call chain, inspect a graph node, get recently changed code, or index a project before starting work."
origin: https://github.com/senoldogann/LLM-Context-Manager
terminal_state: invoke_tool_chain("index_project", then search/context/graph/impact tools based on task)
version: 1.0
license: MIT
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

## When NOT to Use

**DO NOT** activate this skill when:
- The project is **already indexed in memory** from this session and you just need context recall (context is cached; reuse `get_context` directly from memory without re-indexing)
- User is asking for **code refactoring guidance without needing graph traversal** (use code-reviewer skill instead)
- User wants **static type analysis** or **linting feedback** (use language-specific reviewer skill)
- The task is **pure documentation** (no code exploration needed)
- User says "just give me the file" without needing cross-file reasoning

<HARD-GATE>
You MUST call `index_project` before any other tool on a fresh project session.
If the project was already indexed in this conversation, you MAY skip `index_project` and call tools directly.
NEVER assume a project is indexed without explicit mention or evidence.
</HARD-GATE>

## Execution Boundaries

- **Boundary 1:** All file paths MUST be relative to project root (e.g. `src/main.rs`, not `/Users/dev/my-app/src/main.rs`)
- **Boundary 2:** Node IDs MUST be used exactly as returned by tools (never guessed or constructed)
- **Boundary 3:** If a tool returns empty results, check node_id/file format BEFORE escalating
- **Boundary 4:** `max_depth` in trace_call_chain should be 6–15; default (6) may miss distant paths

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

## Workflow Modes

CCM supports 4 distinct operational modes depending on your task. Each has a terminal state.

### Mode 1: Semantic Discovery (Entry Point for Exploration)
**When:** You have a vague question like "find auth handling" but don't know the exact file/function.

```
┌─────────────────────────────────────┐
│ index_project (if fresh session)    │
└──────────────┬──────────────────────┘
               ↓
┌──────────────────────────────────────┐
│ search_code (natural language query) │
│ Returns: ranked snippets + node_ids  │
└──────────────┬──────────────────────┘
               ↓
         [Terminal: use node_id for next step]
```

**Output artifact:** Copy top result node_id and code snippet into your current tool/editorspacing/plan.

---

### Mode 2: Cursor-Based Context (IDE Integration)
**When:** Your editor cursor is at file:line and you need surrounding context.

```
┌──────────────────────────────────────┐
│ get_context(file, line)              │
│ Returns: scope + graph neighbors     │
└──────────────┬──────────────────────┘
               ↓
       [Terminal: you have context]
```

**Output artifact:** Paste retrieved context into your refactoring notes or task description.

---

### Mode 3: Graph Traversal (Surgical Dependency Analysis)
**When:** You have a known node_id and need to drill into its callers, callees, or connections.

```
┌──────────────────────────────────────┐
│ find_nodes (locate by name)          │ (optional)
│ Returns: matching node_ids           │
└──────────────┬──────────────────────┘
               ↓
┌──────────────────────────────────────┐
│ read_graph (inspect node)            │
│ Returns: node details + edges        │
└──────────────┬──────────────────────┘
               ↓
        ╔════════════════════╗
        ║  Choose path:      ║
        ╚────┬───────────┬───╝
             ↓           ↓
    ┌──────────────┐  ┌──────────────────┐
    │ find_usages  │  │ trace_call_chain │
    │ (who calls?) │  │ (path A→B)       │
    └──────────────┘  └──────────────────┘
             ↓           ↓
      [Terminal: impact mapped]
```

**Output artifact:** Document call chains and caller lists in your refactoring impact assessment or pre-edit checklist.

---

### Mode 4: Blast Radius & Recency (Pre-Edit Risk Assessment)
**When:** Before editing a file, you MUST know what breaks and what changed recently.

```
┌──────────────────────────────────────┐
│ impact_of_change (file path)         │
│ Returns: dependents + transitive    │
└──────────────┬──────────────────────┘
               ↓
┌──────────────────────────────────────┐
│ diff_context (git-based changes)     │
│ Returns: recent edits + frequency    │
└──────────────┬──────────────────────┘
               ↓
      [Terminal: risk mapped]
```

**Output artifact:** Create a pre-edit risk matrix: "File X is depended on by Y and Z; recently touched by [commits]; estimated blast radius: N files".

---

## Recommended Starting Flow

```
START
  ↓
INDEX (if fresh session)
  ↓
CHOOSE MODE based on task:
  • "Find something?"        → Mode 1 (search_code)
  • "Context at cursor?"     → Mode 2 (get_context)
  • "Dig into node?"         → Mode 3 (find_nodes → read_graph)
  • "Before I edit X?"       → Mode 4 (impact_of_change + diff_context)
  ↓
EXTRACT OUTPUTS
  ↓
PASS TO next skill/task/plan
```

**Rule:** Most tasks combine 2–3 modes. E.g., "refactor auth" = Mode 1 (search) → Mode 4 (impact) → Mode 3 (trace callees) then handoff to code-reviewer.

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

## Anti-Patterns & Risk Catalog

| Anti-Pattern | Risk | Solution |
|--------------|------|----------|
| **Not indexing first** | All queries fail with "graph not indexed" | ALWAYS call `index_project` on fresh session (safe to repeat) |
| **Mixing absolute + relative paths** | Tool returns "path outside project root" | Use ONLY relative paths (`core/src/lib.rs`, never `/Users/.../core/src/lib.rs`) |
| **Manually constructing node_ids** | Wrong format breaks downstream tools | Use `find_nodes` output; NEVER guess the node_id format |
| **Low `max_depth` in trace_call_chain** | Misses the actual path between functions | Default is 6; increase to 10–15 for deeper chains |
| **Reading `search_code` score without understanding weights** | Over-trusting weak results | Always check the `Reason` field; score reflects hybrid rank, not absolute confidence |
| **Editing a core file without `impact_of_change`** | Silent cascade failures in dependent code | Always run blast-radius check BEFORE editing |
| **Assuming project is indexed from session history** | Stale data, missing recent edits | Re-run `index_project` after significant file changes |
| **Using tool output for architecture decisions alone** | CCM maps code, not design intent | Pair tool output with code review, tests, and conversation with domain expert |
| **Not checking diff_context before refactoring** | Duplicate work; overwriting recent changes | Always check `diff_context` with `days: 7` before any refactor |

---

## Common Output Mistakes (Decoding)

| Tool Output | What It Means | Action |
|-------------|---------------|--------|
| `No context found for src/main.rs:100` | Graph doesn't have that file indexed | Re-run `index_project` or verify path is relative |
| `Node not found with ID: ./core/src/lib.rs:function_item:310:0` | The node_id format is wrong or file changed | Use `find_nodes` to re-discover the correct node_id |
| `No graph nodes found for query: 'UserService'` | Name doesn't exist in this codebase or is indexed differently | Try partial queries (e.g. `User` or `Service` separately) |
| Empty result from `trace_call_chain` | No path exists within `max_depth` (common for distant calls) | Increase `max_depth` to 15–20 or check if path exists manually |
| `Node ID: ./path/file.ext:node_type:line:col` contains multiple colons | This is correct node_id format | Always preserve the exact string; pass it verbatim to other tools |

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

## Output Artifacts

After using CCM, you should have:

- **Search Mode:** Code snippets + node_ids saved to current task context or notebook
- **Context Mode:** Scope summary (functions, classes, dependencies) in your editor notes
- **Graph Mode:** Call chains and impact maps documented as ASCII or Markdown tables
- **Impact Mode:** Pre-edit risk checklist with affected files and change scope

**Recommended:** Save critical outputs to your task description, implementation plan, or memory file for audit trail.

---

## Handoff to Next Layer

After CCM retrieval, depending on your need:

| Your Task | Hand Off To | Why |
|-----------|-------------|-----|
| "I understand the codebase now" | code-reviewer, code-simplifier | Pair graph knowledge with lint/quality feedback |
| "Found a bug, need to fix it" | tdd-workflow, code-reviewer | Write failing test first, use graph as context |
| "Design a massive refactor" | blueprint, architecture | CCM gives you the map; blueprint gives you the plan |
| "Pre-flight check before PR" | code-reviewer, security-reviewer | CCM = "what's connected"; reviewers = "is it correct?" |
| "Performance optimization" | performance-optimizer | CCM shows hot paths; optimizer profiles them |

**Do NOT** end in CCM. Use it as input to the next skill/review/implementation layer. The terminal state is always passing structured output forward.

---

## Verification Checklist

Before considering a task complete:

- [ ] `index_project` was called (or explicitly verified to be cached from same session)
- [ ] Output node_ids are exact strings (not paraphrased)
- [ ] All file paths are relative to project root
- [ ] Tool returned non-empty results (if empty, diagnosed and resolved)
- [ ] Results were reviewed for relevance (score + reason checked)
- [ ] Output passed to another skill or documented in task/plan
- [ ] No assumptions made about file changes; `diff_context` checked if relevant

**If any item is FALSE:** Re-run the tool with corrected parameters before proceeding.

---

## Source & References

**Repository:** https://github.com/senoldogann/LLM-Context-Manager  
**npm Package:** `@senoldogann/context-manager` (v0.1.31+)  
**Built with:** Rust, Tree-sitter, LanceDB, Petgraph  
**License:** MIT  

**Further Reading:**
- [Hybrid Ranking Algorithm](https://github.com/senoldogann/LLM-Context-Manager/blob/main/docs/hybrid-ranking.md) — weight tuning details
- [.env.example](https://github.com/senoldogann/LLM-Context-Manager/blob/main/.env.example) — all configuration variables
- [README](https://github.com/senoldogann/LLM-Context-Manager/blob/main/README.md) — installation and quick start
- [MCP Protocol Spec](https://modelcontextprotocol.io/) — official MCP documentation
