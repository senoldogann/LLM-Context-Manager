# @senoldogann/context-manager

> 🧠 The Neural Backbone for Autonomous AI Agents

**Node.js wrapper for Cognitive Codebase Matrix (CCM)** - Enables AI agents to understand and navigate your codebase with surgical precision.

**v0.3.1** fixes vector deletion prefix scope, strict MCP allowlist defaults, and incremental manifest sync.

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

### First-Run Verification

Use this quick smoke test after install:

```bash
# Verify the CLI responds
npx @senoldogann/context-manager query --text "src/main.rs:1"

# Verify the MCP binary starts
npx @senoldogann/context-manager mcp
```

If the wrapper is healthy, the query command should return code context and the MCP process should stay open waiting for stdio messages.

The first index is complete; subsequent runs are incremental and process only added, changed, renamed, or deleted files. An unchanged project returns a clean no-op.

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
| `npx @senoldogann/context-manager doctor --path <dir>` | Diagnose config and index health |
| `npx @senoldogann/context-manager eval --tasks <file>` | Run evaluation tasks |

### Supported Hosts

| Host | Status |
|------|--------|
| Codex | Supported |
| Cursor | Supported |
| Claude Desktop | Supported |
| Antigravity | Supported |

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

Create `~/.ccm/.env` (or start from the repository's `.env.example`):

```ini
# Local (Recommended)
EMBEDDING_PROVIDER=ollama
EMBEDDING_HOST=http://127.0.0.1:11434
EMBEDDING_MODEL=mxbai-embed-large
EMBEDDING_API_KEY=ollama

# Cloud (Optional)
EMBEDDING_PROVIDER=openai
EMBEDDING_API_KEY=sk-your-key
EMBEDDING_MODEL=text-embedding-3-small

# Networking & Limits
EMBEDDING_TIMEOUT_SECS=30
CCM_MAX_FILE_BYTES=2097152

# MCP Security (strict allowlist is ON by default)
CCM_ALLOWED_ROOTS=/Users/you/projects:/Users/you/sandbox
CCM_REQUIRE_ALLOWED_ROOTS=1

# MCP Runtime
CCM_MCP_ENGINE_CACHE_SIZE=8
CCM_MCP_DEBUG=0

# Optional: disable embeddings entirely (semantic search disabled)
CCM_DISABLE_EMBEDDER=0

# Optional: embed data files (md/json/yaml) into vector search
CCM_EMBED_DATA_FILES=0

# Binary checksum verification (0 = enforce, 1 = bypass)
CCM_ALLOW_UNVERIFIED_BINARIES=0

# Optional binary download tuning
CCM_DOWNLOAD_TIMEOUT_MS=120000
CCM_DOWNLOAD_ATTEMPTS=3
```

Advanced overrides:
- `CCM_PROJECT_ROOT` pins the default project root used by the wrapper and MCP fallback engine.
- `CCM_DB_PATH` overrides the default MCP vector DB location.
- `.env.example` in the repository contains chunking, batch-size, hybrid-weight, and compatibility-alias settings such as `OPENAI_API_KEY`, `CCM_SKIP_CHECKSUM`, `CCM_MCP_REQUIRE_ALLOWED_ROOTS`, `CCM_EMBED_DATA`, and `EMBEDDING_DISABLED`.
- Hybrid scoring defaults and tuning notes live in [`docs/hybrid-ranking.md`](https://github.com/senoldogann/LLM-Context-Manager/blob/main/docs/hybrid-ranking.md).

**Security:** The installer writes a strict allowlist into every MCP config (`CCM_PROJECT_ROOT`, `CCM_ALLOWED_ROOTS`, `CCM_REQUIRE_ALLOWED_ROOTS=1`), and the MCP server itself defaults to strict mode. Widen `CCM_ALLOWED_ROOTS` only when you need access to more project roots.

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
2. Resumable, retrying compressed binary download from GitHub Releases
3. Global persistence in `~/.ccm`

For a manual maintainer release, run the commands from this `npm/` directory:

```bash
npm test
npm pack --dry-run
npm publish --access public --provenance
```

Running `npm publish` from the repository root fails because the root intentionally has no `package.json`.

### ✅ Release Reliability

- GitHub Releases publish platform binaries plus `checksums.txt`
- The npm wrapper verifies checksums before using downloaded binaries
- Interrupted downloads resume from their partial file and use a bounded timeout/retry policy
- Redirects are restricted to approved GitHub release hosts
- Release builds run for Linux, macOS, and Windows before assets are attached
- Codex configuration is updated directly and atomically, even when the `codex` wrapper is broken
- The same smoke path is documented here and in the main repository README

### ✅ Binary Integrity

Downloads are verified against `checksums.txt` from the GitHub Release.  
If the manifest is missing or a mismatch occurs, you can set `CCM_ALLOW_UNVERIFIED_BINARIES=1` to bypass verification (not recommended).

### 📄 Data Files in Search

By default, data files (`.md`, `.json`, `.yaml`) are indexed but not embedded.  
Enable `CCM_EMBED_DATA_FILES=1` to include them in semantic search.

**Source:** https://github.com/senoldogann/LLM-Context-Manager

**Docs:** [English README](https://github.com/senoldogann/LLM-Context-Manager/blob/main/README.md) | [Turkish README](https://github.com/senoldogann/LLM-Context-Manager/blob/main/README.tr.md)

---

## 📝 Changelog

### v0.2.1
- ✅ UTF-8-safe semantic chunking prevents panics on Turkish, Finnish, and emoji content
- ✅ Cross-file imports, constructors, and type annotations contribute to usage and impact analysis
- ✅ `get_context` returns the enclosing function/class instead of only a leaf assignment
- ✅ Incremental `diff_context` includes staged, unstaged, and untracked worktree files
- ✅ Compressed release binaries, download resume/retry/timeout, Linux ARM64 detection
- ✅ Codex MCP entries are repaired and enabled without depending on the Codex CLI binary
- ✅ Release tags run tests, lint, npm tests, indexing, and golden tasks v3 before publishing assets

### v0.1.31
- ✅ `index_project` is idempotent again and ignores CCM's own generated index files
- ✅ `search_code` falls back cleanly when embeddings are disabled
- ✅ Added a real-world Guardian MCP end-to-end test for all 9 tools

### v0.1.27
- ✅ `search_code` now returns node IDs and location metadata, with configurable limits
- ✅ New `find_nodes` MCP tool for graph node discovery before `read_graph`
- ✅ `index_project` now reports clean no-op refreshes and ignores internal CCM index files

### v0.1.26
- ✅ Trusted publishing workflow updated for modern Node and npm requirements
- ✅ Release jobs now skip npm publish when the same version is already live
- ✅ Safer wrapper binary downloads during concurrent first-run installs

### v0.1.25
- ✅ npm publish now uses GitHub Actions trusted publishing (OIDC)
- ✅ Clearer first-run verification and release reliability docs

### v0.1.24
- ✅ Native `protoc` installation in GitHub Actions release builds
- ✅ Updated release workflow actions for newer runner compatibility

### v0.1.23
- ✅ JSON-RPC request size limit for safer MCP stdio transport
- ✅ Sensitive value redaction in MCP debug logs
- ✅ Hardened path normalization for MCP graph and context tools
- ✅ GitHub release redirect allowlist for binary downloads
- ✅ Unused core dependency cleanup

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
