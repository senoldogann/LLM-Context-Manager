# Release v0.1.0 - "Genesis"

We are proud to announce the first release of **Cognitive Codebase Matrix (CCM)**. This release marks the completion of the core architecture, enabling AI agents to biologically understand codebases.

## 🚀 Key Features

*   **Universal Context Provider (MCP):** Full MCP Server implementation compatible with Claude Desktop and other MCP clients.
*   **Hybrid Retrieval Engine:**
    *   **Graph:** Structural code analysis using Code Property Graphs (CPG).
    *   **Vector:** Semantic search powered by LanceDB.
*   **Flexible AI Core:**
    *   **Local:** Native support for **Ollama** (`nomic-embed-text`, `mxbai-embed-large`).
    *   **Cloud:** Support for **OpenAI** compatible APIs.
*   **Tools:**
    *   `read_graph`: Inspect deep code relationships.
    *   `search_code`: Find code by meaning, not just keywords.
    *   `get_context`: Cursor-aware context prediction.

## 🔧 Fixes & Improvements

*   **Dependency Resolution:** Removed problematic `ort` dependency in favor of a clean, HTTP-based `RemoteEmbedder`.
*   **Ollama Compatibility:** Fixed generic API pathing (`/api/embed`) and added batching support.
*   **gRPC Foundation:** initial scaffolding for high-performance IPC.

## 📦 Usage

See `README.md` for detailed installation instructions.

```bash
# Quick Start (Local)
ollama pull nomic-embed-text
cargo run -p ccm-cli -- query --text "Hello World"
```
