# @senoldogann/context-manager

> 🧠 The Neural Backbone for Autonomous AI Agents

This is the Node.js wrapper for the **Cognitive Codebase Matrix (CCM)**. It allows you to run the CCM CLI and MCP server without manually installing Rust or building from source.

## 🚀 Quick Start

### 1. Auto-Configure your Editor
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

## 📦 What this package does
This package is a lightweight wrapper that:
1. Detects your OS and CPU architecture.
2. Downloads the pre-built Rust binaries from GitHub Releases if not already present.
3. Automatically manages your global index and persistence in `~/.ccm`.

For more details, visit the [Main Repository](https://github.com/senoldogann/LLM-Context-Manager).

### 🆕 v0.1.7 Updates
*   **Zero-Config:** Automatically detects project root.
*   **Fixes:** Resolved context retrieval issues with absolute paths.

