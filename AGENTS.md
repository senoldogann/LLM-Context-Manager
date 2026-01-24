# AGENTS.md (Project Context & Operations)

> **SYSTEM NOTICE:** This project is governed by **SPAP v2.2**.
> Active Rules: `.agent/rules/` | Active Skills: `.agent/skills/`

## 1. 🏗️ PROJECT IDENTITY
* **Name:** LLM-Context-Manager (CCM)
* **Type:** Agentic MCP Server / AI Infrastructure
* **Language:** Rust (Core), TypeScript/Node.js (Wrapper)
* **Framework:** SPAP v2.2, Model Context Protocol (MCP)

## 2. ⚡ OPERATIONAL COMMANDS (Source of Truth)
Bu projenin çalıştırılma standartları şunlardır:

| Action | Command | Note |
| :--- | :--- | :--- |
| **Install** | `cargo build` | Rust bağımlılıklarını kurar ve derler. |
| **Dev** | `cargo run` | Geliştirme modunda çalıştırır. |
| **Build** | `cargo build --release` | Optimizasyonlu production binary üretir (`target/release`). |
| **Publish** | `npm publish --access public` | NPM paketini yayınlar (npx dağıtımı için). |
| **Test** | `cargo test` | Unit ve Integration testlerini çalıştırır. |
| **Lint/Fmt** | `cargo fmt && cargo clippy` | Kod formatlama ve linter kontrolü. |

## 3. 📂 FILE STRUCTURE MAP
* **`core/`**: Rust Core Logic (Analysis Engine, Graph, Vector Store)
* **`cli/`**: CLI Interface implementation
* **`mcp/`**: MCP Server implementation
* **`.agent/`**: **BRAIN OF THE SYSTEM.**
* **`npm/`**: Node.js wrapper scripts for distribution

## 4. 🔗 CRITICAL INTEGRATIONS
* **Database:** LanceDB (Embedded Vector Store)
* **AI Engine:** Ollama (Local Embedding), OpenAI (Cloud Embedding)
* **Protocol:** Model Context Protocol (MCP) stdio transport