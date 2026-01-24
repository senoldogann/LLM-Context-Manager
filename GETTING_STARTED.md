# Getting Started with LLM-Context-Manager (CCM)

**New here? This guide will help you understand and use CCM in 5 minutes!**

---

## 🤔 What is CCM?

**Cognitive Codebase Matrix (CCM)** is an intelligent bridge between your codebase and your AI assistant. It provides:
1.  **Semantic Search:** Find code by meaning ("where is auth logic?").
2.  **Graph Navigation:** Understand code structure ("who calls this function?").
3.  **Smart Context:** Get relevant code snippets based on your cursor position.

---

## 📦 Architecture

This repo contains the complete system:

```
context-manager/
├── core/       ← Rust Core Engine (Vector DB + Graph)
├── mcp/        ← MCP Server impementation (The "Bridge")
├── cli/        ← Command Line Interface tool
└── npm/        ← Node.js wrapper for distribution
```

---

## 🚀 Quick Start

### 1. Installation

**Option A: For AI Agents (Claude Desktop, Antigravity, etc.)**
Use the `npx` command to run the MCP server directly:
```bash
npx @senoldogann/context-manager install
```

**Option B: For Developers (Rust)**
Build from source to get the CLI tool:
```bash
cargo build --release
./target/release/ccm-cli --help
```

### 2. Basic Usage (CLI)

**Index a project:**
```bash
npx @senoldogann/context-manager index --path .
```

**Query the index:**
```bash
npx @senoldogann/context-manager query --text "dependency injection usage"
```

### 3. Usage inside AI

Once installed as an MCP server, you can talk to your AI like this:

> "Search the codebase for the user authentication flow."
>
> "Read the graph for the `UserService` class and tell me who depends on it."
>
> "I'm starting a new task, please **index the project** to get fresh context."

---

## 🤝 Contributing

We welcome contributions! Please follow the **SPAP v2.2** governance model defined in `AGENTS.md`.

*   **Logic Changes:** Go to `core/src/lib.rs`
*   **MCP API:** Go to `mcp/src/server.rs`
*   **CLI:** Go to `cli/src/main.rs`

Always run `cargo test` before submitting changes.
