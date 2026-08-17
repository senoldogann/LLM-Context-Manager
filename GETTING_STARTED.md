# Getting Started with CCM

**New to CCM? This guide gets you running in 5 minutes.**

---

## What is CCM?

**Cognitive Codebase Matrix (CCM)** is an intelligent bridge between your codebase and AI assistants:

- 🔍 **Semantic Search** - Find code by meaning, not just keywords
- 🧠 **Graph Navigation** - Understand code relationships
- 📍 **Smart Context** - Get relevant code at your cursor position

---

## Quick Start

### Step 1: Install

**Option A: For AI Editors (Codex, Cursor, Claude Desktop, Antigravity)**

```bash
npx @senoldogann/context-manager install
```

**Option B: For CLI Development**

```bash
# If local source builds fail, install protoc first.
# macOS: brew install protobuf

cargo build --release
./target/release/ccm-cli --help
```

**Option C: Docker (no Rust toolchain)**

```bash
docker build -t ccm:local .

# Index the current directory (mounted at /workspace)
docker run --rm -v "$PWD":/workspace -w /workspace \
  -e EMBEDDING_HOST=http://host.docker.internal:11434 \
  -e EMBEDDING_MODEL=mxbai-embed-large \
  ccm:local index --path /workspace

# Query it
docker run --rm -v "$PWD":/workspace -w /workspace \
  -e EMBEDDING_HOST=http://host.docker.internal:11434 \
  ccm:local query --text "authentication flow"
```

Or use `docker compose run --rm ccm index --path /workspace`. On Linux, replace
`host.docker.internal` with your host IP if the Docker bridge cannot resolve it.

### Step 2: Configure

Create `~/.ccm/.env` with the basics below, or start from the repository's `.env.example` for the full advanced list.

Ensure [Ollama](https://ollama.com) is running:

```bash
ollama serve
ollama pull mxbai-embed-large
```

Optional production settings (recommended for server use):

```ini
# Restrict MCP access to allowed roots
CCM_ALLOWED_ROOTS=/Users/you/projects:/Users/you/sandbox
CCM_REQUIRE_ALLOWED_ROOTS=1

# Embed data files in semantic search (0 = off, 1 = on)
CCM_EMBED_DATA_FILES=0
```

Advanced overrides such as `CCM_PROJECT_ROOT`, `CCM_DB_PATH`, chunking controls, and hybrid ranking weights are documented in `.env.example` and [`docs/hybrid-ranking.md`](docs/hybrid-ranking.md).

### Step 3: Index Your Project

```bash
npx @senoldogann/context-manager index --path .
```

---

## Usage

### CLI Examples

```bash
# Search for code
ccm-cli query --text "dependency injection"

# Find context at specific location
ccm-cli query --text "src/main.rs:42"

# Auto-reindex on changes
ccm-cli index --path . --watch
```

### AI Conversation Examples

Once configured as MCP server:

> "Search for the user authentication flow."

> "Read the graph for `PaymentService` and show me all callers."

> "What does the `parseConfig` function do?"

---

## Project Structure

```
context-manager/
├── core/       # Rust engine (Vector DB + Graph)
├── mcp/        # MCP Server implementation
├── cli/        # Command-line interface
├── npm/        # Node.js wrapper for distribution
└── eval/       # Evaluation framework & golden tasks
```

---

## Development

```bash
# Build
cargo build --release

# Test
cargo test

# Lint
cargo fmt && cargo clippy

# Eval (bootstraps missing index automatically)
cargo run -p ccm-cli -- eval --tasks eval/golden_tasks.v3.ccm.json
```

---

## Learn More

- **Full Documentation:** [README.md](README.md)
- **Evaluation Framework:** [eval/README.md](eval/README.md)
- **Contributing:** [CONTRIBUTING.md](CONTRIBUTING.md)
