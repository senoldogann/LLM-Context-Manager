# Contributing to Cognitive Codebase Matrix (CCM)

Thank you for your interest in contributing! We want to make it as easy as possible for you to join our mission.

## 🛠 Getting Started

1.  **Fork** the repository on GitHub.
2.  **Clone** your fork locally.
3.  **Install Dependencies:**
    *   Rust (latest stable)
    *   Ollama (optional, for local embeddings)

## 🧪 Development Workflow

We use standard Rust tooling.

```bash
# Run tests
cargo test --workspace

# Check formatting
cargo fmt --all -- --check

# Run linter
cargo clippy -- -D warnings
```

> **Note:** Our CI pipeline enforces these checks. Please run them locally before pushing to avoid failed builds.

## 🚀 Submitting a Pull Request

1.  Create a new branch for your feature/fix (`git checkout -b feature/amazing-idea`).
2.  Commit your changes using **Conventional Commits** (e.g., `feat: add new parser`, `fix: resolve crash`).
3.  Push to your fork and submit a Pull Request to `main`.
4.  Fill out the PR Template clearly.

## 🏗 Architecture

*   `core`: The brain. Contains logic for Graph, Vector Store, and Parsing.
*   `mcp`: The interface. Implements Model Context Protocol server.
*   `cli`: The tool. Command-line utilities for indexing and querying.

## 🤝 Code of Conduct

Be kind, inclusive, and respectful. We are building the future of AI-assisted coding together.
