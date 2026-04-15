# @senoldogann/context-manager

> 🧠 The Neural Backbone for Autonomous AI Agents

**Node.js wrapper for Cognitive Codebase Matrix (CCM)** - Enables AI agents to understand and navigate your codebase with surgical precision.

[![npm](https://img.shields.io/npm/v/@senoldogann/context-manager?color=orange)](https://www.npmjs.com/package/@senoldogann/context-manager)
[![MCP](https://img.shields.io/badge/MCP-Compatible-blue)](https://modelcontextprotocol.io/)
[![Rust](https://img.shields.io/badge/Built%20With-Rust-orange.svg)](https://www.rust-lang.org/)

---

## 🚀 Quick Start

### Prerequisites

1. **Node.js** 16+ installed
2. **Ollama** installed and running (for local embeddings)
   - Download: https://ollama.com
   - Pull required model: `ollama pull mxbai-embed-large`

### Installation

```bash
# 1. Install and configure for Codex, Cursor, Claude Desktop, Antigravity, etc.
npx @senoldogann/context-manager install

# 2. Index your project
npx @senoldogann/context-manager index --path .
```

**That's it!** Restart your AI editor and start asking questions about your code.

---

## 📖 What is CCM?

CCM transforms static source code into a dynamic, queryable Knowledge Graph:

- **🔍 Semantic Search** - Find code by meaning ("where is auth logic?")
- **🧠 Graph Navigation** - Understand relationships ("who calls this function?")
- **📍 Cursor Context** - Get relevant code based on your position

---

## 🔧 Commands

The npm wrapper downloads pre-built binaries and passes commands through:

| Command | Description |
|---------|-------------|
| `npx @senoldogann/context-manager install` | Auto-configure MCP for editors |
| `npx @senoldogann/context-manager index --path <dir>` | Index a project |
| `npx @senoldogann/context-manager query --text "..."` | Search codebase |
| `npx @senoldogann/context-manager mcp` | Run MCP server directly |
| `npx @senoldogann/context-manager eval --tasks <file>` | Run evaluation tasks |

### Options

```bash
# Watch mode - auto-reindex on file changes
npx @senoldogann/context-manager index --path . --watch

# Custom database path
npx @senoldogann/context-manager index --path . --db-path /custom/path
```

---

## 🔒 Privacy by Default

CCM uses a **Local-First** architecture:

- ✅ Your code **never** leaves your machine
- ✅ All embeddings run locally via Ollama
- ✅ No external API calls (unless you configure OpenAI)

---

## ⚙️ Configuration

### Environment Variables

Create `~/.ccm/.env`:

```ini
# Local (Recommended)
EMBEDDING_PROVIDER=ollama
EMBEDDING_HOST=http://127.0.0.1:11434
EMBEDDING_MODEL=mxbai-embed-large

# Cloud (Optional)
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

# Binary checksum verification (0 = enforce, 1 = bypass)
CCM_ALLOW_UNVERIFIED_BINARIES=0
```

**Production Tip:** Set `CCM_ALLOWED_ROOTS` and enable `CCM_REQUIRE_ALLOWED_ROOTS=1` to restrict MCP access.

---

## 🤖 Usage in AI

Once configured, ask your AI agent:

> "Search for the authentication flow in this codebase."

> "Read the graph for `UserService` and show me its callers."

> "What functions call `parse_config`?"

---

## 📦 For Developers

This package handles:

1. OS/architecture detection
2. Binary download from GitHub Releases
3. Global persistence in `~/.ccm`

### ✅ Binary Integrity

Downloads are verified against `checksums.txt` from the GitHub Release.  
If the manifest is missing or a mismatch occurs, you can set `CCM_ALLOW_UNVERIFIED_BINARIES=1` to bypass verification (not recommended).

### 📄 Data Files in Search

By default, data files (`.md`, `.json`, `.yaml`) are indexed but not embedded.  
Enable `CCM_EMBED_DATA_FILES=1` to include them in semantic search.

**Source:** https://github.com/senoldogann/LLM-Context-Manager

---

## 📝 Changelog

### v0.1.22
- ✅ Codex installer support via `codex mcp add`
- ✅ Cursor config support via `~/.cursor/mcp.json`
- ✅ Evaluation bootstraps missing indexes before scoring
- ✅ Golden tasks refreshed for the current repo layout

### v0.1.21
- ✅ Release checksums (`checksums.txt`) for binary integrity
- ✅ MCP allowlist with optional strict enforcement
- ✅ Data file embedding is optional (`CCM_EMBED_DATA_FILES`)
- ✅ CLI/MCP integration tests

### v0.1.20
- ✅ 100% evaluation pass rate
- ✅ Hybrid scoring (graph + semantic)
- ✅ Lazy indexing support
- ✅ Watch mode for auto-reindexing

---

Built with ❤️ in **Rust**
