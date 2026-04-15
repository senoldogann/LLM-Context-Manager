# Cognitive Codebase Matrix (CCM)

> **🧠 The Neural Backbone for Autonomous AI Agents**

> Bridge the gap between your codebase and your AI editor. CCM transforms static source code into a dynamic, queryable Knowledge Graph, enabling AI agents to navigate, understand, and reason about your project with surgical precision.

[![Rust](https://img.shields.io/badge/Built%20With-Rust-orange.svg?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![MCP Ready](https://img.shields.io/badge/MCP-Compatible-blue.svg?style=flat-square&logo=google-cloud)](https://modelcontextprotocol.io/)
[![Graph-RAG](https://img.shields.io/badge/Engine-Graph--RAG-purple.svg?style=flat-square)](https://github.com/senoldogann/LLM-Context-Manager)
[![License](https://img.shields.io/badge/License-MIT-green.svg?style=flat-square)](LICENSE)

---

## Why CCM?

Modern AI coding assistants (Claude, Cursor, Windsurf) are powerful but suffer from **blindness**:

| Problem | Impact |
|---------|--------|
| **Context Limits** | Can't "see" your entire 100,000-line project |
| **Hallucination** | Guesses dependencies without structure |
| **Lost Context** | Vector search finds *similar words*, not *connected logic* |

CCM turns your AI from a *text predictor* into a **Senior Architect**.

### The "Agent-First" Difference

Unlike tools that dump raw code, CCM injects **AI-Optimized Context**:

- **Logical Reasoning** - Explains *why* code was retrieved
- **Relational Edges** - Maps how files talk to each other
- **Confidence Scores** - Shows certainty in results

---

## Key Features

### 🧠 Connected Intelligence (Graph Navigator)
- **Two-Pass Indexing** - Links function definitions to call sites
- **Deep Traversal** - Ask "Who calls this?" and get accurate answers

### ⚡ High-Performance Core
- **Rust-Powered** - Blazing fast indexing and queries
- **Batch Embedding** - Thousands of lines in seconds
- **LanceDB** - Millisecond-latency vector storage
- **Tree-sitter** - Robust AST for Rust, Python, TypeScript

### 🔒 Production Hardening
- **Binary Checksums** - Release artifacts include `checksums.txt` for integrity
- **MCP Allowlist** - Restrict project access with `CCM_ALLOWED_ROOTS`
- **Safe Defaults** - Configurable timeouts and file-size limits

### 🔌 Universal Compatibility (MCP)
- **Plug & Play** - Installer configures Codex, Cursor, Claude Desktop, and Antigravity
- **Lazy Indexing** - Auto-indexes on first query
- **Zero-Config** - Auto-detects project root

---

## Installation

### ⚡ Automatic (Recommended)

```bash
# 1. Configure MCP for your AI editor
npx @senoldogann/context-manager install

# 2. Index your project
npx @senoldogann/context-manager index --path .
```

### 🔧 Manual Build (Rust)

```bash
# If local source builds fail, install protoc first.
# macOS: brew install protobuf

git clone https://github.com/senoldogann/LLM-Context-Manager.git
cd LLM-Context-Manager
cargo build --release

# Binary location: target/release/ccm-cli
```

---

## Configuration

Create `~/.ccm/.env`:

```ini
# Option A: Local (Recommended - Privacy)
EMBEDDING_PROVIDER=ollama
EMBEDDING_HOST=http://127.0.0.1:11434
EMBEDDING_MODEL=mxbai-embed-large

# Option B: Cloud (OpenAI)
EMBEDDING_PROVIDER=openai
EMBEDDING_API_KEY=sk-your-key
EMBEDDING_MODEL=text-embedding-3-small

# Networking & Limits
EMBEDDING_TIMEOUT_SECS=30
CCM_MAX_FILE_BYTES=2097152

# MCP Security
CCM_ALLOWED_ROOTS=/Users/you/projects:/Users/you/sandbox
CCM_REQUIRE_ALLOWED_ROOTS=0

# MCP Runtime
CCM_MCP_ENGINE_CACHE_SIZE=8
CCM_MCP_DEBUG=0

# Optional: disable embeddings entirely (semantic search disabled)
CCM_DISABLE_EMBEDDER=0

# Optional: embed data files (md/json/yaml) into vector search
CCM_EMBED_DATA_FILES=0

# npm wrapper security (0 = enforce checksum, 1 = bypass)
CCM_ALLOW_UNVERIFIED_BINARIES=0
```

**Note:** Requires Ollama running (`ollama serve`) with model pulled (`ollama pull mxbai-embed-large`).
**Production Tip:** Set `CCM_ALLOWED_ROOTS` and enable `CCM_REQUIRE_ALLOWED_ROOTS=1` to prevent unintended project access.

---

## Usage

### CLI Commands

```bash
# Index a project
ccm-cli index --path .

# Search semantically
ccm-cli query --text "authentication logic"

# Cursor prediction (file:line format)
ccm-cli query --text "src/main.rs:50"

# Watch mode - auto-reindex
ccm-cli index --path . --watch

# Evaluate retrieval quality
ccm-cli eval --tasks eval/golden_tasks.json
```

### MCP Tools

| Tool | Purpose | Example |
|------|---------|---------|
| `search_code` | Semantic search | "Find auth handling" |
| `read_graph` | Structural navigation | "Who calls this function?" |
| `get_context` | Cursor-based retrieval | Context at file:line |

---

## Architecture

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│ AI Agent    │────▶│ MCP Server  │────▶│ Core Engine │
│ (Claude)    │◀────│ (ccm-mcp)   │◀────│ (Rust)      │
└─────────────┘     └─────────────┘     └─────────────┘
                                                  │
                    ┌────────────────────────────┼────────────────────────────┐
                    ▼                            ▼                            ▼
             ┌─────────────┐            ┌─────────────┐            ┌─────────────┐
             │ Code Graph  │            │  Vector DB  │            │  Parser     │
             │ (Petgraph)  │            │  (LanceDB)  │            │(Tree-sitter)│
             └─────────────┘            └─────────────┘            └─────────────┘
```

---

## Supported Languages

| Language | Extensions | Analysis |
|----------|------------|----------|
| Rust | `.rs` | Full AST |
| Python | `.py` | Full AST |
| TypeScript | `.ts`, `.tsx` | Full AST |
| JavaScript | `.js`, `.jsx` | Full AST |
| Config/Data | `.md`, `.json`, `.yaml` | Full File |

---

## Evaluation

CCM includes a golden task evaluation framework:

```bash
# Run evaluation
ccm-cli eval --tasks eval/golden_tasks.v3.ccm.json

# Compare structural vs hybrid scoring
ccm-cli eval --tasks eval/golden_tasks.json --compare
```

If the evaluation index is missing, CCM bootstraps it automatically before scoring.
Semantic `search_code` tasks still require a configured embedder.

**Latest Recorded Results:** See the checked-in reports under [`eval/`](./eval).

---

## Troubleshooting

### "No context found"
1. Run `ccm-cli index --path .` first
2. Check CCM_PROJECT_ROOT matches indexed directory
3. Ensure Ollama is running

### Slow indexing
- First run downloads embedding model (~1.5GB)
- Subsequent runs are fast (incremental)

### "Checksum manifest not found" / "Checksum mismatch"
1. Ensure the GitHub Release includes `checksums.txt`
2. Re-run the install once
3. As a last resort, set `CCM_ALLOW_UNVERIFIED_BINARIES=1` to bypass verification

### "Project path is not allowed"
- Set `CCM_ALLOWED_ROOTS` to include the project root
- Or disable strict mode with `CCM_REQUIRE_ALLOWED_ROOTS=0`

### Large/binary files are skipped
- Increase `CCM_MAX_FILE_BYTES` if you need larger text files indexed

### Data files not showing in search
- By default, data files (`.md`, `.json`, `.yaml`) are indexed but not embedded.
- Enable `CCM_EMBED_DATA_FILES=1` to include them in semantic search.

---

## Resources

- **NPM Package:** [@senoldogann/context-manager](https://www.npmjs.com/package/@senoldogann/context-manager)
- **Getting Started:** [GETTING_STARTED.md](GETTING_STARTED.md)
- **Contributing:** [CONTRIBUTING.md](CONTRIBUTING.md)

---

## License

MIT License - Open source and free to use.

SENOL DOGAN ❤️ 
