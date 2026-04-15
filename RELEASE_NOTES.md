# Release Notes

## v0.1.22 - MCP Compatibility & Eval Reliability (2026-04-15)

### ✅ Installer Coverage
- `npm install` flow now configures Codex through `codex mcp add` when the CLI is available.
- Cursor config support now writes `~/.cursor/mcp.json`.

### 🔌 MCP Compatibility
- `initialize` now negotiates protocol versions instead of always returning `2025-06-18`.
- Latest supported protocol is now `2025-11-25`, with compatibility for older clients.

### 🧪 Evaluation Reliability
- Evaluation bootstraps missing indexes before scoring instead of silently skipping the entire suite.
- Golden tasks were refreshed to remove references to deleted files in the current repository layout.

---

## v0.1.21 - Production Hardening (2026-02-03)

### ✅ Security & Release Integrity
- GitHub Release artifacts now include `checksums.txt` for binary verification.
- MCP allowlist support with `CCM_ALLOWED_ROOTS` and optional enforcement via `CCM_REQUIRE_ALLOWED_ROOTS`.

### ⚙️ Operational Improvements
- MCP and CLI now honor `RUST_LOG` via structured logging setup.
- Data files (`.md`, `.json`, `.yaml`) can be embedded when `CCM_EMBED_DATA_FILES=1`.

### 🧪 Test Coverage
- New CLI integration test (index + file:line query).
- New MCP integration tests (index flow + allowlist rejection).

---

## v0.1.0 - Initial Release (2026-01-08)

### 🎉 First Public Release

CCM (Cognitive Codebase Matrix) is now available as a fully functional MCP server for AI-powered code understanding.

---

### ✨ Features

- **MCP Server (`ccm-mcp`)**
  - Full JSON-RPC 2.0 implementation
  - Protocol version `2025-06-18` support
  - Compatible with Antigravity, Claude Desktop, and other MCP clients
  - Three built-in tools:
    - `get_context` - File/line context retrieval
    - `search_code` - Semantic vector search
    - `read_graph` - Code graph node inspection

- **Core Engine (`ccm-core`)**
  - LanceDB vector store integration
  - Tree-sitter multi-language parsing (Rust, Python, TypeScript, JavaScript)
  - Petgraph-based code graph representation
  - Hybrid retrieval (structural + semantic)

- **Embedding Support**
  - Local: Ollama (`mxbai-embed-large`, `nomic-embed-text`)
  - Cloud: OpenAI (`text-embedding-3-small`)

---

### 🔧 Technical Details

- **Protocol:** MCP 2025-06-18
- **Transport:** stdio (JSON-RPC over stdin/stdout)
- **Language:** Rust 1.70+
- **Vector DB:** LanceDB
- **Parser:** Tree-sitter

---

### 📋 Known Limitations

1. **Manual Indexing Required:** The codebase must be indexed manually via CLI before search works.
2. **Single Workspace:** Currently supports one workspace per MCP session.
3. **Embedding Dependency:** Requires Ollama or OpenAI for embeddings.

---

### 🚀 Getting Started

1. Clone and build: `cargo build --release`
2. Configure `.env` with your embedding provider
3. Add wrapper script to your MCP config
4. Restart your AI editor

See [README.md](README.md) for detailed instructions.

---

### 🔜 Roadmap (v0.2.0)

- [ ] Auto-indexing on workspace open
- [ ] Multi-workspace support
- [ ] Incremental indexing (file watchers)
- [ ] More language support (Go, Java, C++)
- [ ] LSP integration for real-time updates

---

---

## v0.1.8 - Multi-Language Support & Robustness (2026-01-10)

This release significantly expands CCM's capabilities to include non-code files and improves the reliability of the installation process.

### ✨ Multi-Language Support
- **Full File Indexing:** Added support for `.md`, `.json`, `.yaml`, `.yml`, and `.toml`.
- **Intelligent Data Parsing:** These formats bypass AST extraction and are indexed as whole-file nodes, making them semantically searchable.
- **Project Context:** AI agents now have full visibility into configuration and documentation files.

### 🛠️ Robustness & Fixes
- **Atomic Downloads:** The `npm` wrapper now uses `.tmp` files for binary downloads to prevent corrupted installations.
- **Guaranteed Permissions:** Explicit `chmod` calls ensure binaries always have execute permissions on Unix-like systems.
- **Simultaneous Binary Install:** Running `install` now proactively downloads both `ccm-cli` and `ccm-mcp` to ensure local availability.
- **Watch Mode:** Updated the CLI watch filter to include new supported extensions.

### 📦 Upgrading
- Run `npx @senoldogann/context-manager install` to update.
- Re-index your project to pick up new file types: `npx @senoldogann/context-manager index --path .`
