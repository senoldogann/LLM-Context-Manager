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

## 🎯 Good First Issues

New contributors: start here. Each item is small, self-contained, and has a
clear acceptance signal (a passing test or a visible artifact).

| Area | What to do | Signal |
|------|-----------|--------|
| `mcp/src/protocol.rs` | Expand JSON-RPC frame/notification handling tests (currently the least-covered MCP module) | `cargo test -p ccm-mcp` green + coverage rises |
| `core/src/vector/remote.rs` | Cover embedding timeout and retry branches (host validation already has tests) | `cargo test -p ccm-core vector::remote` green |
| `core/src/parser` | Prototype SCIP index import for precise cross-file symbol resolution | Sample SCIP corpus produces correct cross-file edges |
| `cli` / `mcp` | Prototype LSP integration for real-time index updates | Editor save triggers a `--watch`-style incremental update |
| Docker | Add multi-arch builds (`linux/amd64`, `linux/arm64`) to the release workflow | `docker buildx build --platform linux/amd64,linux/arm64` succeeds |
| Docs | Add a short animated demo/GIF to `GETTING_STARTED.md` (terminal + one query) | Preview renders in README |

When in doubt, open a draft PR early and tag the maintainers for direction.

## 📦 Release Checklist (vX.Y.Z)

1. `cargo fmt --all -- --check` + `cargo clippy --workspace --all-targets -- -D warnings`
2. `cargo test --workspace` + `npm test --prefix npm` (hermetic MCP e2e dahil)
3. Structural golden gate: `jq '.tasks |= map(select(.query.type !=
   "search_code"))' eval/golden_tasks.v3.ccm.json > /tmp/golden.structural.json`
   + `CCM_DISABLE_EMBEDDER=1 ccm-cli eval --tasks /tmp/golden.structural.json
   --min-pass-rate 100 --max-regression 0` (search_code hariç 50/50)
4. Gerçek anlamsal eval (Ollama varsa): `eval --tasks
   eval/golden_tasks.v3.ccm.json --report eval/report.semantic.json`; sonucu
   README'de güncelle
5. Self-improve: `CCM_EMBEDDING_FIXTURE=... ccm-cli learn optimize --seed 42`
   (Promote veya Rejected; ikisi de geçerli)
6. Version bump: `core/cli/mcp Cargo.toml` + `Cargo.lock` + `npm/package.json`
   + `npm/package-lock.json` + README/RELEASE_NOTES
7. Tag `vX.Y.Z` push → Release workflow (quality-gate → 5 platform build →
   release assets → npm tarball attach) yeşil; npm publish manuel, `latest`
   doğrula
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
