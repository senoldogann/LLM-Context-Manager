# Release Notes

## v0.1.31 - Incremental Index Idempotency & Embedder Fallback (2026-04-16)

### 🐛 Bug Fixes

- **Idempotent `index_project`**: A second call to `index_project` on an already-indexed project now
  correctly reports "No changes detected" instead of reporting a spurious refresh. Two root causes
  were fixed:
  - CCM's own data files (`data/ccm_graph.json`, `data/ccm_manifest.json`, `data/ccm_db/`) are now
    filtered out of the git untracked-file list before the incremental indexer runs.
  - `nodes_created` in `IndexStats` now tracks the delta of new nodes added during a pass, not the
    total node count of the graph — preventing the "refreshed" response even when zero files changed.

- **`search_code` with embedder disabled**: When `CCM_DISABLE_EMBEDDER=1` is set (or no embedding
  provider is configured), `search_code` and the hybrid search path no longer return an "Internal
  error". They now gracefully fall through to graph-based keyword search.

- **`diff_context` absolute path**: `diff_context` no longer misreports file paths when the project
  root and CWD differ. Relative paths are now resolved against the project root, not the process CWD.

### 🧪 Testing
- Added a real-world end-to-end integration test (`guardian_e2e_test`) that exercises all 9 MCP tools
  against a complex Tauri + TypeScript project. Marked `#[ignore]` so it runs locally on demand and
  does not block CI.

---

## v0.1.27 - MCP Tooling UX & Incremental Index Fixes (2026-04-15)

### 🔎 Better MCP Tool Chaining
- `search_code` now supports a configurable result limit and returns node IDs plus location metadata.
- Added `find_nodes` so agents can discover graph node IDs before calling `read_graph`.
- `get_context` now includes the same metadata surface, making graph follow-ups more natural.

### ♻️ Index Refresh Accuracy
- `index_project` now bypasses lazy engine bootstrapping so an explicit refresh no longer pre-indexes behind the scenes.
- Clear "already up to date" messaging is now returned when no changes are detected.
- Internal index artifacts are excluded from manifest diffing so incremental refreshes do not re-index CCM's own generated files.

---

## v0.1.26 - Publish Pipeline Hardening (2026-04-15)

### 🚀 npm Release Reliability
- Updated the npm publish workflow to match npm trusted publishing requirements with modern GitHub Actions and Node 24.
- Added a registry check so release jobs skip npm publish when the version is already live instead of failing the whole release.
- Added explicit repository metadata to the npm package manifest for trusted publisher validation.

### 📦 Wrapper Stability
- Hardened binary downloads for concurrent first-run installs by creating unique temporary files per process.
- Ensured wrapper download paths create parent directories before writing binary payloads.

---

## v0.1.25 - Trusted Publishing & Onboarding Clarity (2026-04-15)

### 🚀 Publish Flow
- Switched npm publishing to GitHub Actions trusted publishing with OIDC.
- Removed the release workflow dependency on a manually managed `NPM_TOKEN`.

### 📘 Onboarding
- Added first-run verification steps to the main README and npm README.
- Documented supported MCP hosts and release reliability expectations more clearly.

---

## v0.1.24 - Release Pipeline Reliability (2026-04-15)

### 🚀 Release Workflow
- Replaced GitHub API-backed `setup-protoc` with native package manager installation on Linux, macOS, and Windows runners.
- Updated checkout and Node setup actions to current major versions for better long-term runner compatibility.

---

## v0.1.23 - MCP Transport Hardening (2026-04-15)

### 🔒 Transport & Input Safety
- Added a JSON-RPC request size limit to protect the stdio transport from oversized payloads.
- Redacted sensitive values from MCP debug logs instead of echoing raw request and response payloads.
- Hardened MCP path normalization to reject parent-directory traversal in `get_context` and `read_graph`.

### 📦 Installer & Release Safety
- Restricted npm binary download redirects to approved GitHub release hosts only.
- Removed unused core dependencies to reduce build surface and cleanup legacy protobuf-era wiring.

---

## v0.1.22 - MCP Compatibility & Eval Reliability (2026-04-15)

### ✅ Installer Coverage
- `npm install` flow now configures Codex through `codex mcp add` when the CLI is available.
- Cursor config support now writes `~/.cursor/mcp.json`.

### 🔌 MCP Compatibility
- `initialize` now negotiates protocol versions instead of always returning `2025-06-18`.
- Latest supported protocol is now `2025-11-25`, with compatibility for older clients.

### 🧪 Evaluation Reliability
- Evaluation bootstraps missing indexes before scoring instead of silently skipping the entire suite.
- Golden tasks were refreshed to remove references to deleted files in the current repository layout.

---

## v0.1.21 - Production Hardening (2026-02-03)

### ✅ Security & Release Integrity
- GitHub Release artifacts now include `checksums.txt` for binary verification.
- MCP allowlist support with `CCM_ALLOWED_ROOTS` and optional enforcement via `CCM_REQUIRE_ALLOWED_ROOTS`.

### ⚙️ Operational Improvements
- MCP and CLI now honor `RUST_LOG` via structured logging setup.
- Data files (`.md`, `.json`, `.yaml`) can be embedded when `CCM_EMBED_DATA_FILES=1`.

### 🧪 Test Coverage
- New CLI integration test (index + file:line query).
- New MCP integration tests (index flow + allowlist rejection).

---

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

---

---

## v0.1.8 - Multi-Language Support & Robustness (2026-01-10)

This release significantly expands CCM's capabilities to include non-code files and improves the reliability of the installation process.

### ✨ Multi-Language Support
- **Full File Indexing:** Added support for `.md`, `.json`, `.yaml`, `.yml`, and `.toml`.
- **Intelligent Data Parsing:** These formats bypass AST extraction and are indexed as whole-file nodes, making them semantically searchable.
- **Project Context:** AI agents now have full visibility into configuration and documentation files.

### 🛠️ Robustness & Fixes
- **Atomic Downloads:** The `npm` wrapper now uses `.tmp` files for binary downloads to prevent corrupted installations.
- **Guaranteed Permissions:** Explicit `chmod` calls ensure binaries always have execute permissions on Unix-like systems.
- **Simultaneous Binary Install:** Running `install` now proactively downloads both `ccm-cli` and `ccm-mcp` to ensure local availability.
- **Watch Mode:** Updated the CLI watch filter to include new supported extensions.

### 📦 Upgrading
- Run `npx @senoldogann/context-manager install` to update.
- Re-index your project to pick up new file types: `npx @senoldogann/context-manager index --path .`
