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

## 📦 Release Checklist (vX.Y.Z)

1. `cargo fmt --all -- --check` + `cargo clippy --workspace --all-targets -- -D warnings`
2. `cargo test --workspace` + `npm test --prefix npm` (hermetic MCP e2e dahil)
3. Structural golden gate: `CCM_DISABLE_EMBEDDER=1 ccm-cli eval --tasks
   eval/golden_tasks.v3.ccm.json` (search_code hariç 50/50, regresyon 0)
4. Gerçek anlamsal eval (Ollama varsa): `eval --tasks golden_tasks.v3.ccm.json
   --report eval/report.semantic.json`; sonucu README'de güncelle
5. Self-improve: `CCM_EMBEDDING_FIXTURE=... ccm-cli learn optimize --seed 42`
   (Promote veya Rejected; ikisi de geçerli)
6. Version bump: `core/cli/mcp Cargo.toml` + `Cargo.lock` + `npm/package.json`
   + `npm/package-lock.json` + README/RELEASE_NOTES
7. Tag `vX.Y.Z` push → Release workflow (quality-gate → 5 platform build →
   publish-npm) yeşil; npm `latest` doğrula
8. GitHub release notlarını `RELEASE_NOTES.md`'den üret

### 1.0 İçin Açık Maddeler (cutline)
- Gerçek repo corpus'unda anlamsal (embedder) eval CI'ya kalıcı bağlanmalı
- Trajectory feedback (açılan/düzenlenen dosya) öğrenme döngüsüne bağlanmalı
- MCP progress/cancellation (`notifications/progress`, `-32800`)
- Sembol çözümlemesi scope/qualified-name seviyesine çekilmeli (Phase 1 devamı)

## 🏗 Architecture

*   `core`: The brain. Contains logic for Graph, Vector Store, and Parsing.
*   `mcp`: The interface. Implements Model Context Protocol server.
*   `cli`: The tool. Command-line utilities for indexing and querying.

## 🤝 Code of Conduct

Be kind, inclusive, and respectful. We are building the future of AI-assisted coding together.
