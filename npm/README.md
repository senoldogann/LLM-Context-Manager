# @senoldogann/context-manager

> 🧠 The Neural Backbone for Autonomous AI Agents

This is the Node.js wrapper for the **Cognitive Codebase Matrix (CCM)**. It allows you to run the CCM CLI and MCP server without manually installing Rust or building from source.

## 🚀 Quick Start

> [!IMPORTANT]
> **Prerequisite:** This tool relies on local AI models. You must have **[Ollama](https://ollama.com)** installed and running.
> CCM will **automatically pull** the required embedding model (`nomic-embed-text`) if missing, but the Ollama engine itself is required.

### 🔒 Privacy by Default
CCM uses a **Local-First** architecture by default. This means:
*   Your code is **never** sent to 3rd party servers (OpenAI, Anthropic, etc.).
*   All vector operations (Embedding) happen on your local machine.
*   You can safely use it for internal or confidential projects.

The easiest way to get started. This will automatically add CCM to your Claude or Antigravity configuration:
```bash
npx @senoldogann/context-manager install
```

### 2. Index your Project
Run the indexer in your project root:
```bash
npx @senoldogann/context-manager index --path .
```

## ⚒️ Manual Configuration (Alternative)

If you prefer to configure your AI editor manually without the `install` command, add this to your `mcp_config.json`:

```json
{
  "mcpServers": {
    "context-manager": {
      "command": "npx",
      "args": ["-y", "@senoldogann/context-manager", "mcp"],
      "env": {
        "RUST_LOG": "info"
      }
    }
  }
}
```

## 💡 Pro-Tip: Enforcing CCM Usage
To ensure your AI agent (Claude, Cursor, etc.) always uses CCM for deep analysis, add this to your **Custom Instructions** or **System Prompt**:

> "You are an expert architect. For any question about the codebase, DO NOT guess. Use the `context-manager` tools to explore the Graph and Vector store. Always prioritize `search_code` to find entry points and `read_graph` to navigate dependencies before proposing any code changes."

---

## 🇹🇷 Türkçe Özet

Bu paket, **Cognitive Codebase Matrix (CCM)** için Node.js wrapper'ıdır. Rust kurulumuna gerek kalmadan CCM araçlarını kullanmanızı sağlar.

**Hızlı Kurulum:**
```bash
npx @senoldogann/context-manager install
npx @senoldogann/context-manager index --path .
```

---

## 📦 What this package does
This package is a lightweight wrapper that:
1. Detects your OS and CPU architecture.
2. Downloads the pre-built Rust binaries from GitHub Releases if not already present.
3. Automatically manages your global index and persistence in `~/.ccm`.

For more details, visit the [Main Repository](https://github.com/senoldogann/LLM-Context-Manager).

### 🆕 v0.1.8 Updates
*   **Multi-Language Support:** Full indexing for `.md`, `.json`, `.yaml`, `.toml` and more.
*   **Zero-Config:** Improved project root detection.
*   **Resilient Downloads:** Fixed binary download issues with atomic file creation.

Built with ❤️ in **Rust**.
