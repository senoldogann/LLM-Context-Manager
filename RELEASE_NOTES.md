# Release Notes

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
