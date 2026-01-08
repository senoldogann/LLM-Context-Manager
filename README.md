# Cognitive Codebase Matrix (CCM)

> **Context Provider for AI Agents & LLMs**
>
> CCM is a high-performance, Rust-based system designed to index, understand, and serve codebase context to AI agents. It effectively bridges the gap between raw source code and Large Language Models through graph-based structural analysis and semantic vector search.

## 🌟 Features

*   **Dual Intelligence Engine:** Combines **Code Property Graphs (CPG)** for structural understanding (Control flow, Data flow) with **Vector Embeddings** for semantic retrieval.
*   **Universal Context Provider (MCP):** Fully implements the **Model Context Protocol (MCP)** to serve context to any MCP-compliant client (Claude Desktop, Zed, Custom Agents).
*   **Plug-and-Play AI:** Supports both **OpenAI** (Cloud) and **Ollama** (Local) for embedding generation.
*   **High Performance:** Built with **Rust**, **LanceDB** (Vector Store), and **Tree-sitter** (Parsing), ensuring minimal latency and memory footprint.
*   **Graph Analysis:** Supports deep querying of code relationships (e.g., "Find all callers of function X", "Show class hierarchy").

## 🚀 Installation

### Prerequisites

*   **Rust Toolchain:** Ensure `cargo` is installed (`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`).
*   **Ollama (for Local AI):** Install [Ollama](https://ollama.com/) if you plan to use local embeddings.

### Steps

1.  **Clone the Repository:**
    ```bash
    git clone https://github.com/your-username/context-manager.git
    cd context-manager
    ```

2.  **Build the Project:**
    ```bash
    cargo build --release
    ```
    *This creates binaries in `target/release/` (`ccm-core`, `ccm-cli`, `ccm-mcp`).*

## ⚙️ Configuration

CCM uses a `.env` file for configuration.

1.  **Create `.env`:**
    ```bash
    cp .env.example .env  # If example exists, otherwise create new
    ```

2.  **Select Your AI Provider:**

    **Option A: Local Ollama (Recommended for Privacy/Cost)**
    *   Ensure Ollama is running (`ollama serve`).
    *   Pull the embedding model:
        ```bash
        ollama pull nomic-embed-text
        # OR if you have issues downloading models automatically:
        # Download mxbai-embed-large-v1-f16.gguf manually and create it with `ollama create`
        ```
    *   Edit `.env`:
        ```ini
        EMBEDDING_PROVIDER=ollama
        EMBEDDING_HOST=http://127.0.0.1:11434
        EMBEDDING_API_KEY=ollama
        EMBEDDING_MODEL=nomic-embed-text  # or mxbai-embed-large
        RUST_LOG=info
        ```

    **Option B: OpenAI (Cloud)**
    *   Edit `.env`:
        ```ini
        EMBEDDING_PROVIDER=openai
        EMBEDDING_HOST=https://api.openai.com/v1
        EMBEDDING_API_KEY=sk-your-openai-key-here
        EMBEDDING_MODEL=text-embedding-3-small
        RUST_LOG=info
        ```

## 🛠️ Usage

### 1. CLI Usage (Command Line Interface)

Use `ccm-cli` to interact with the system directly.

*   **Semantic Search:**
    ```bash
    cargo run -p ccm-cli -- query --text "authentication logic"
    ```
    *Finds code snippets semantically related to "authentication logic".*

*   **Index Codebase:**
    *(The system automatically indexes on startup, but CLI triggers can be added)*

### 2. MCP Server (AI Agent Integration)

Run the MCP server to expose tools to your AI editor or agent.

```bash
cargo run -p ccm-mcp
```

**Available Tools:**
*   `get_context(file_path, line)`: Smart context lookup based on cursor position.
*   `search_code(query)`: Semantic search across the codebase.
*   `read_graph(node_id)`: Retrieve detailed info about a specific code node (Function, Class) by its ID.

## 🏗️ Architecture

*   **`ccm-core`:** The brain. Handles Graph (Petgraph), Vector Store (LanceDB), and Parsing (Tree-sitter).
*   **`ccm-mcp`:** The interface. Implements JSON-RPC 2.0 based Model Context Protocol.
*   **`ccm-cli`:** The utility. Provides terminal access for testing and management.

## 📄 License

MIT License. See `LICENSE` file for details.
