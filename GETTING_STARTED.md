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

# Index the current directory (mounted at /workspace). The built-in embedding
# model is downloaded once into the `ccm-models` volume.
docker run --rm -v "$PWD":/workspace -v ccm-models:/models -w /workspace \
  ccm:local index --path /workspace

# Query it
docker run --rm -v "$PWD":/workspace -v ccm-models:/models -w /workspace \
  ccm:local query --text "authentication flow"
```

The image runs Linux, so the built-in model works there on Intel Macs too. To
use Ollama on the host instead, pass the same settings to every command:

```bash
docker run --rm -v "$PWD":/workspace -w /workspace \
  -e EMBEDDING_PROVIDER=ollama \
  -e EMBEDDING_HOST=http://host.docker.internal:11434 \
  -e EMBEDDING_MODEL=mxbai-embed-large \
  ccm:local index --path /workspace
```

Or use `docker compose run --rm ccm index --path /workspace` (it mounts the
models volume; the Ollama settings are commented out in `docker-compose.yml`).
On Linux, replace `host.docker.internal` with your host IP if the Docker bridge
cannot resolve it.

To embed with OpenAI in the container, select it explicitly: there the key is a
plain environment variable, which never switches the provider on its own.
`-e OPENAI_API_KEY` passes the key through from your shell:

```bash
docker run --rm -v "$PWD":/workspace -w /workspace \
  -e EMBEDDING_PROVIDER=openai -e OPENAI_API_KEY \
  ccm:local index --path /workspace
```

### Step 2: Configure

Create `~/.ccm/.env` with the basics below, or start from the repository's `.env.example` for the full advanced list.

Embeddings need no setup: a built-in model runs inside the binary and is
downloaded once (~124 MB) on the first index. To fetch it ahead of time (or to
prepare an offline machine), run:

```bash
npx @senoldogann/context-manager models pull
```

To use OpenAI embeddings instead, add your key to `~/.ccm/.env`. Only this file
counts: a key exported in your shell does not switch providers. CCM then uses
the official endpoint and `text-embedding-3-small`, and code chunks are sent to OpenAI:

```ini
# ~/.ccm/.env
OPENAI_API_KEY=sk-your-key
```

To use [Ollama](https://ollama.com) instead:

```bash
ollama serve
ollama pull mxbai-embed-large
# ~/.ccm/.env
EMBEDDING_PROVIDER=ollama
```

On Intel Macs the built-in model is not available: until one of the providers
above is configured, CCM builds a graph-only index and `doctor` explains why.

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
