# Cognitive Codebase Matrix (CCM)

<p align="center">
  <img src="docs/assets/cover.png" width="400" alt="LLM Context Manager">
</p>

English | [Turkce](./README.tr.md)

> **A code graph for AI coding agents that keeps up with your edits — and tells you when it is not watching.**

> CCM parses your project with tree-sitter (13 languages), keeps a call and
> import graph plus an optional semantic index on disk, and serves them to
> agents such as Claude Code, Codex and Cursor through 10 MCP tools. On Django
> 5.1 (6,670 files) a saved change shows up in graph results after a median of
> about 0.6 s ([measurement](https://github.com/senoldogann/LLM-Context-Manager/pull/5):
> release build, embedder off). Query responses start with the index state
> (`fresh`, `stale · …` while a registered change is pending, or
> `auto-refresh off · indexed Xs ago`), and a failed re-index never replaces the
> last good graph. In the pre-registered L1 freshness benchmark (one-function
> edits in Flask 3.0.3 and Django 5.1, probes at fixed times), every graph answer
> from 0.25 s after a save on matched the edit; in the sub-250 ms window before
> the watcher applies the event, 6 of 126 probes returned the pre-edit answer
> still labeled `fresh`, and 4 more a partial or empty one
> ([benchmarks/](./benchmarks/README.md)).

> **Status:** search quality has a 35-task pilot benchmark. In an LLM-free
> benchmark of 24 fixed questions on Flask and Django, the 0.4.0 tools answer
> with 73% fewer response bytes and a third fewer calls than the 0.3 tools, and
> keep 163 of the 164 caller locations
> ([token cost](./benchmarks/README.md#token-cost-of-answers-m2)). Whether agents
> finish tasks faster or with fewer tokens is not measured yet. What is and is
> not measured: [`benchmarks/`](./benchmarks/README.md).

[![Rust](https://img.shields.io/badge/Built%20With-Rust-orange.svg?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![MCP Ready](https://img.shields.io/badge/MCP-Compatible-blue.svg?style=flat-square&logo=google-cloud)](https://modelcontextprotocol.io/)
[![Graph-RAG](https://img.shields.io/badge/Engine-Graph--RAG-purple.svg?style=flat-square)](https://github.com/senoldogann/LLM-Context-Manager)
[![License](https://img.shields.io/badge/License-MIT-green.svg?style=flat-square)](LICENSE)
[![Agent Skill](https://img.shields.io/badge/Agent-SKILL.md-blueviolet.svg?style=flat-square)](SKILL.md)

---

## Why CCM?

Coding agents find code well with grep. What grep does not give them is
structure: who calls a function, what a change can break, which calls connect
two places. CCM answers those as graph queries (`find_usages`,
`impact_of_change`, `trace_call_chain`) and keeps the graph current while you
edit:

- **Fresh after a save:** a file watcher applies changes to the index the MCP
  server already holds in memory (median ~0.6 s on Django 5.1, graph only).
- **Says when it is not watching:** with auto-refresh off, responses say
  `auto-refresh off · indexed Xs ago`. With auto-refresh on, the status line
  does not yet cover the sub-250 ms window above.
- **Never half-built:** each index is built as a new generation and activated
  atomically; a failed or interrupted run leaves the previous graph serving.

**Limits, stated up front.** In Python files, calls and other uses of a name
(arguments, attribute access such as `User.objects`, type hints) are read from
the syntax tree and resolved through the file's imports (`import`,
`from … import`, relative imports, package re-exports including
`from .x import *`), `self`/`cls`/`super()` and class names;
a call on a receiver whose type is unknown is reported as a *possible* call to
at most five same-named definitions, and imports that leave the project produce
no edge. In the other 12 languages call edges are still resolved by name: a
call binds to a definition in the same file first, otherwise to the only
definition elsewhere; several same-file definitions produce edges marked
ambiguous, and a name defined in several other files produces no edge. How often
this matches a type-aware tool has not been measured yet. Other MCP servers
also build code graphs and refresh them automatically; CCM makes no claim of
being fresher or more accurate than them until that is measured
([`benchmarks/`](./benchmarks/README.md)).

---

## Key Features

### 🧠 Connected Intelligence (Graph Navigator)
- **Two-Pass Indexing** - Links function definitions to call sites
- **Incremental Refresh** - Re-indexes only added, modified, renamed, or deleted files after the first run
- **Deep Traversal** - Ask "Who calls this?" and get the callers the graph knows (name-resolved, see limits above)

### ⚡ High-Performance Core
- **Rust-Powered** - Single-binary CLI and MCP server; index and query timings
  are published in [`benchmarks/`](./benchmarks/README.md), not claimed here
- **Built-in Embeddings** - Semantic search works out of the box: a pinned multilingual, code-trained embedding model runs inside the binary (no Ollama, no API key)
- **Deterministic Embedding** - One chunk per inference call on all physical cores, so a chunk's vector never depends on its neighbors (Ollama/OpenAI requests are batched)
- **LanceDB** - Millisecond-latency vector storage
- **Tree-sitter** - Robust AST for Rust, Python, TypeScript, JavaScript, Go, Java, Kotlin, C#, C, C++, Ruby, PHP, and Swift

### 🔒 Production Hardening
- **Binary Checksums** - Release artifacts include `checksums.txt` for integrity
- **MCP Allowlist** - Restrict project access with `CCM_ALLOWED_ROOTS`
- **Safe Defaults** - UTF-8-safe chunking, configurable timeouts, retries, and file-size limits

### 🔌 Universal Compatibility (MCP)
- **Plug & Play** - Installer configures Claude Code, Codex, Cursor, Claude Desktop, and Antigravity
- **Explicit Indexing** - Missing indexes fail fast and point to `index_project`
- **Zero-Config** - Auto-detects project root

---

## Installation

### ⚡ Automatic (Recommended)

```bash
# 1. Configure MCP for your AI editor
npx @senoldogann/context-manager install

# 2. Index your project
npx @senoldogann/context-manager index --path .
```

### First-Run Verification

After installation, verify the happy path with a clean smoke test:

```bash
# Check that the CLI responds
npx @senoldogann/context-manager query --text "src/main.rs:1"

# Start the MCP server directly
npx @senoldogann/context-manager mcp
```

Expected outcomes:
- The wrapper downloads the correct binary for your OS and architecture
- `index` creates `data/ccm_db` inside the project
- `mcp` starts without JSON-RPC parse errors and waits on stdio

### Editor Compatibility

| Host | Status | Installation Path |
|------|--------|-------------------|
| Claude Code | Supported | `claude mcp add-json --scope user` (requires the `claude` CLI) |
| Codex | Supported | Atomic update of `~/.codex/config.toml` |
| Cursor | Supported | `~/.cursor/mcp.json` |
| Claude Desktop | Supported | Native desktop config |
| Antigravity | Supported | Native host config |

If your editor is not auto-detected, use the manual MCP config printed by the installer.

### 🤖 Agent Skill

CCM ships a [`SKILL.md`](SKILL.md) (about 4 KB) in both the source repository and the npm tarball. It tells an agent which of the 10 MCP tools answers which question, the `map` → `explain` → `impact_of_change` workflow and the answer budgets, and it includes a `CLAUDE.md` snippet and a SessionStart hook that loads the project map.

Copy it into your agent's skill directory and it becomes a first-class tool reference:
```bash
# Install to your local agents skill directory
cp SKILL.md ~/.agents/skills/context-manager/SKILL.md
```

### 🔧 Manual Build (Rust)

```bash
# If local source builds fail, install protoc first.
# macOS: brew install protobuf

git clone https://github.com/senoldogann/LLM-Context-Manager.git
cd LLM-Context-Manager
cargo build --release

# Binary location: target/release/ccm-cli
```

**No Rust toolchain?** Use the Docker option in [GETTING_STARTED.md](GETTING_STARTED.md).

### Manual npm publication (maintainers)

The npm manifest is intentionally inside `npm/`, not the repository root:

```bash
cd npm
npm test
npm pack --dry-run
npm publish --access public --provenance
```

---

## Configuration

Semantic search needs no configuration: the built-in local embedding model is
the default (see [Embeddings](#embeddings)). Create `~/.ccm/.env` (or start from
the repository's `.env.example`) only to change defaults:

```ini
# Default: built-in local model, nothing to set.

# Option B: Ollama
EMBEDDING_PROVIDER=ollama
EMBEDDING_HOST=http://127.0.0.1:11434
EMBEDDING_MODEL=mxbai-embed-large
# No API key is required for local Ollama.

# Option C: OpenAI. This one line selects it; code chunks are sent to OpenAI.
OPENAI_API_KEY=sk-your-key

# Networking & Limits
EMBEDDING_TIMEOUT_SECS=30
CCM_MAX_FILE_BYTES=2097152

# MCP Security (strict allowlist is ON by default)
CCM_ALLOWED_ROOTS=/Users/you/projects:/Users/you/sandbox
CCM_REQUIRE_ALLOWED_ROOTS=1

# MCP Runtime
CCM_MCP_ENGINE_CACHE_SIZE=8
CCM_MCP_DEBUG=0
# Re-index automatically when project files change (0 = manual index_now only)
CCM_AUTO_REFRESH=1

# Optional: disable embeddings entirely (semantic search disabled)
CCM_DISABLE_EMBEDDER=0

# Optional: embed data files (md/json/yaml) into vector search
CCM_EMBED_DATA_FILES=0

# npm wrapper security (0 = enforce checksum, 1 = bypass)
CCM_ALLOW_UNVERIFIED_BINARIES=0

# Optional download tuning (milliseconds / attempts)
CCM_DOWNLOAD_TIMEOUT_MS=120000
CCM_DOWNLOAD_ATTEMPTS=3
```

Advanced overrides:
- `CCM_PROJECT_ROOT` pins the default project root and overrides the workspace reported by the host. Without it the MCP server resolves its default project in this order: the workspace the host reports via MCP `roots` → the launch directory when it lies inside `CCM_ALLOWED_ROOTS` (never `/` or your home directory) → the single `CCM_ALLOWED_ROOTS` entry.
- `CCM_DB_PATH` overrides the default MCP vector DB location.
- `.env.example` contains the full advanced tuning surface for chunking, batch size, hybrid ranking weights, and compatibility aliases such as `OPENAI_API_KEY`, `CCM_SKIP_CHECKSUM`, `CCM_MCP_REQUIRE_ALLOWED_ROOTS`, `CCM_EMBED_DATA`, and `EMBEDDING_DISABLED`.
- Hybrid weight tuning details live in [`docs/hybrid-ranking.md`](./docs/hybrid-ranking.md).

### Embeddings

**Default: built-in local model.** With no `EMBEDDING_*` settings and no
`OPENAI_API_KEY` in `~/.ccm/.env`, CCM embeds code in-process with
[`ibm-granite/granite-embedding-97m-multilingual-r2`](https://huggingface.co/ibm-granite/granite-embedding-97m-multilingual-r2)
(Apache-2.0; IBM's int8 ONNX export, 384-d vectors, CLS pooling, inputs truncated
to 512 tokens) through ONNX Runtime, on the physical CPU cores.

- **Download:** the first index (or `ccm-cli models pull`) downloads ~124 MB
  (98 MB model + 25 MB tokenizer + configs) from Hugging Face at a pinned revision
  into `~/.ccm/models/ibm-granite--granite-embedding-97m-multilingual-r2/<revision>/`.
  Every file is verified against a pinned SHA-256; a mismatch or failed download
  is an explicit error, never a silent fallback. Only the model is downloaded;
  nothing about your code leaves the machine.
- **Offline / air-gapped:** run `ccm-cli models pull` on a connected machine and
  copy `~/.ccm/models` over; pre-placed files are used after checksum verification.
  `CCM_MODEL_DIR` moves the models root, `HF_ENDPOINT` selects a mirror.
- **Tuning:** `CCM_EMBED_THREADS` (default: physical cores, capped by the
  available CPU quota, e.g. a container's cgroup limit). The model embeds one
  chunk per inference call: its int8 activations are quantized per call, so
  batching would make a chunk's vector depend on the chunks embedded with it.
  `CCM_LOCAL_EMBED_BATCH` sets the local inference batch (default 1);
  `CCM_EMBED_BATCH_SIZE` sets the texts per Ollama/OpenAI request (default 32).
- **Intel Macs (`x86_64-apple-darwin`):** ONNX Runtime ships no prebuilt binary
  for this target, so the local model is not compiled in. Until `OPENAI_API_KEY`
  is in `~/.ccm/.env` or `EMBEDDING_PROVIDER` is set, CCM builds a graph-only
  index; `ccm-cli doctor` and the index output say why.

**Optional upgrade: OpenAI.** Add `OPENAI_API_KEY=sk-...` to `~/.ccm/.env` and
CCM embeds with `text-embedding-3-small` on the official `https://api.openai.com/v1`
endpoint, which needs no `CCM_ALLOW_REMOTE_EMBEDDING` opt-in. Code chunks are
then sent to OpenAI. Only the key in `~/.ccm/.env` counts: a key that is merely
exported in your shell never switches the provider, so code does not leave the
machine without an explicit choice. [`benchmarks/`](./benchmarks/README.md)
compares it with the built-in model.

**Provider selection:** `EMBEDDING_PROVIDER=local|ollama|openai` wins when set.
Otherwise an `OPENAI_API_KEY` in `~/.ccm/.env` selects OpenAI unless
`EMBEDDING_HOST` is set (`EMBEDDING_MODEL` then only picks the OpenAI model).
Without that key, setting `EMBEDDING_HOST` or `EMBEDDING_MODEL` keeps the previous
Ollama/OpenAI behavior, so existing configurations work unchanged, and with
neither the local model is used. `CCM_DISABLE_EMBEDDER=1` turns semantic search off.

**Changing models:** the index manifest records which provider, model, revision
and dimension produced its vectors, and vectors of two models are never mixed.
After a change (including an upgrade from 0.3.x, whose Ollama-built index meets
the new local default), the MCP server re-embeds the active index once in the
background (the freshness line says `semantic index being rebuilt`; auto-refresh
keeps the graph fresh meanwhile and `search_code` uses graph results until it
finishes).
`ccm-cli index` / `index_project` re-embed on demand the same way.

If the embedding source is unavailable (Ollama or OpenAI unreachable, model
download failed),
indexing still activates a graph-only index (graph tools work, `search_code`
falls back to lexical matching) and reports why; the next index run fills in the
vectors. `ccm-cli doctor` reports the embedder state: for the local model it
checks the files without downloading and runs a probe embedding; for Ollama/OpenAI
it sends a real probe request.

**Security:** MCP enforces a strict allowlist by default — only directories under `CCM_ALLOWED_ROOTS` (falling back to `CCM_PROJECT_ROOT`) and workspaces the host reports via MCP `roots` can be indexed or read. Set `CCM_REQUIRE_ALLOWED_ROOTS=0` only if you explicitly want the relaxed mode; even then, access stays confined to the startup project root.

---

## Usage

### CLI Commands

```bash
# Index a project
ccm-cli index --path .

# Project map: files ordered by how much other files use them
ccm-cli map --path .

# Search semantically
ccm-cli query --text "authentication logic"

# Cursor prediction (file:line format)
ccm-cli query --text "src/main.rs:50"

# Watch mode - auto-reindex
ccm-cli index --path . --watch

# Diagnose installer, allowlist, index compatibility, and embedder state
ccm-cli doctor --path .

# Download and verify the built-in embedding model ahead of time
ccm-cli models pull

# Evaluate retrieval quality
ccm-cli eval --tasks eval/golden_tasks.v3.ccm.json
```

### MCP Tools

| Tool | Answers | Example |
|------|---------|---------|
| `map` | What is in this project and what matters most | `map {path:"src"}`: files by cross-file use, with their top symbols |
| `explain` | Everything about one symbol in one call | `explain {target:"Engine.start"}`: definition, body, members, callers, callees, tests |
| `find_usages` | Who uses a symbol, and how | "Who calls this function?" |
| `impact_of_change` | What may break if a file changes | Dependents across the codebase |
| `search_code` | Code by meaning or name | "Find auth handling" |
| `find_nodes` | Symbols by name or path | `find_nodes {query:"UserService"}` |
| `trace_call_chain` | How one symbol reaches another | `from` → `to` path |
| `diff_context` | Recently changed code via git | Last N days of changes |
| `index_project` | Refresh the project index | Incremental; `mode:"quick"` skips embeddings and upgrades them in the background |
| `index_now` | Index and wait for the final stats | `mode:"quick"`, `"full"` or `"upgrade"` |

Results are one line each: `- Kind: name · path:start-end · relation`. `target`
accepts a name (`run`, `Engine.start`), a file path, `path:line` (a result's
`path:start-end` works too) or a node ID; an ambiguous name returns its
candidates. Every graph tool takes `max_tokens` (default 1500,
`map` 1000) and says how many results did not fit.

> **0.4.0 (breaking):** `get_context` and `read_graph` were replaced by
> `explain`. Graph tools return compact one-line results without node IDs and
> take a `max_tokens` budget instead of `max_chars` (still accepted as
> characters / 4); pass `path:line` as `target`.

### Incremental indexing behavior

The first `index` builds the complete index. Later `index_project` or `index --watch` runs compare the filesystem manifest and update only changed or newly created files while removing deleted-file nodes. The vector database is not rebuilt when nothing changed. Cross-file reference edges are refreshed from the in-memory semantic graph so callers remain accurate after a symbol changes. Retrieval tools fail fast when an index is missing or being updated; call `index_project` explicitly instead of making a search request wait for a hidden rebuild.

For large repositories, MCP `index_project` returns before the client's timeout
and continues in the background. Call the tool again to poll until it returns
the final indexing statistics.

For fast first-response workflows, call `index_project` or `index_now` with
`mode: "quick"`. This scans and parses sources, builds the navigation graph, and
returns nearly immediately without waiting on the embedding provider. A
background task then fills the semantic vectors from the same graph; until that
completes, `search_code` serves graph-ranked results instead of failing. Use
`mode: "upgrade"` (or `index_now` with `"upgrade"`) to repair or complete a
graph-only index that was created while the embedding backend was unavailable,
without re-parsing source files.

Full rebuilds are prepared in a staging generation. A scan, parse, embedding or
vector write failure leaves the previous graph, manifest and vector table in
place. File fingerprints include content, so same-size edits with preserved
timestamps are still detected.

---

## Architecture

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│ AI Agent    │────▶│ MCP Server  │────▶│ Core Engine │
│ (Claude)    │◀────│ (ccm-mcp)   │◀────│ (Rust)      │
└─────────────┘     └─────────────┘     └─────────────┘
                                                  │
                    ┌────────────────────────────┼────────────────────────────┐
                    ▼                            ▼                            ▼
             ┌─────────────┐            ┌─────────────┐            ┌─────────────┐
             │ Code Graph  │            │  Vector DB  │            │  Parser     │
             │ (Petgraph)  │            │  (LanceDB)  │            │(Tree-sitter)│
             └─────────────┘            └─────────────┘            └─────────────┘
```

---

## Supported Languages

| Language | Extensions | Analysis |
|----------|------------|----------|
| Rust | `.rs` | Full AST |
| Python | `.py` | Full AST |
| TypeScript | `.ts`, `.tsx` | Full AST |
| JavaScript | `.js`, `.jsx` | Full AST |
| Go | `.go` | Full AST |
| Java | `.java` | Full AST |
| Kotlin | `.kt`, `.kts` | Full AST |
| C# | `.cs` | Full AST |
| C | `.c`, `.h` | Full AST |
| C++ | `.cc`, `.cpp`, `.cxx`, `.hh`, `.hpp`, `.hxx` | Full AST |
| Ruby | `.rb`, `.rake`, `.gemspec` | Full AST |
| PHP | `.php`, `.phtml` | Full AST |
| Swift | `.swift` | Full AST |
| Config/Data | `.md`, `.json`, `.yaml` | Full File |

---

## Evaluation

CCM includes a golden task evaluation framework:

```bash
# Run evaluation
ccm-cli eval --tasks eval/golden_tasks.v3.ccm.json

# Compare structural vs hybrid scoring
ccm-cli eval --tasks eval/golden_tasks.v3.ccm.json --compare
```

If the evaluation index is missing, CCM bootstraps it automatically before scoring.
Semantic `search_code` tasks still require a configured embedder.

**Regression gate (not a quality measure):** CI runs a synthetic task set,
[`eval/fixtures/golden_tasks.synthetic.json`](./eval/fixtures/golden_tasks.synthetic.json),
with deterministic fixture embeddings in [`eval.yml`](./.github/workflows/eval.yml)
and fails on any drop (`--min-pass-rate 100`). It guards against regressions;
it says nothing about retrieval quality on real code. The external benchmark
below is a 35-task pilot; its `get_context`/`read_graph` tasks are cursor and
neighbourhood lookups, not agent tasks.

**External benchmark (v0.3.13):** 35 hand-verified golden tasks on real repos
(serde, flask, express) with real Ollama embeddings. Hybrid scoring passes
82.9% (29/35) vs 80.0% (28/35) semantic-only; `get_context`/`read_graph` are
20/20. On `search_code` alone Recall@5 is 0.600 vs 0.533. The built-in local
model (the default) reaches Recall@5 0.600 semantic-only and 0.667 hybrid (MRR
0.419 and 0.497 vs 0.352 and 0.436 for mxbai) and indexes 3–4× faster on CPU
(19–23 ms vs 68–81 ms per chunk on an Apple M4). Full numbers, failure ledger and reproduction steps:
[`benchmarks/README.md`](./benchmarks/README.md).

---

## Release Reliability

CCM release builds are designed to be reproducible and install-safe:

- GitHub Releases publish platform binaries and `checksums.txt`
- The npm wrapper verifies downloaded binaries before first use
- MCP transport now enforces request size limits and redacts sensitive debug payloads
- The release workflow builds Linux, macOS, and Windows artifacts before attaching release assets
- npm publication is a separate manual step from `npm/` after the GitHub Release assets are attached
- The README quick-start now matches the same smoke path we use for first-install checks

For local source builds, `cargo build --release` still requires `protoc` to be installed on your machine.

---

## Troubleshooting

### "No context found"
1. Run `ccm-cli index --path .` first
2. If you override it, check `CCM_PROJECT_ROOT` matches the indexed directory
3. Run `ccm-cli doctor` to check the embedder (local model files, or your Ollama/OpenAI service)

### Slow indexing
- The first run downloads the local embedding model (~124 MB, once); `ccm-cli models pull` fetches it ahead of time
- Subsequent runs are fast (incremental)

### "Checksum manifest not found" / "Checksum mismatch"
1. Ensure the GitHub Release includes `checksums.txt`
2. Re-run the install once
3. As a last resort, set `CCM_ALLOW_UNVERIFIED_BINARIES=1` to bypass verification

### "Project path is not allowed"
- Strict allowlist mode is enabled by default
- Set `CCM_ALLOWED_ROOTS` to include the project root
- Only if you really need it, disable strict mode with `CCM_REQUIRE_ALLOWED_ROOTS=0` (access still stays within the startup project root)

### Large/binary files are skipped
- Increase `CCM_MAX_FILE_BYTES` if you need larger text files indexed

### Data files not showing in search
- By default, data files (`.md`, `.json`, `.yaml`) are indexed but not embedded.
- Enable `CCM_EMBED_DATA_FILES=1` to include them in semantic search.

---

## Resources

- **NPM Package:** [@senoldogann/context-manager](https://www.npmjs.com/package/@senoldogann/context-manager)
- **Turkish README:** [README.tr.md](./README.tr.md)
- **Getting Started:** [GETTING_STARTED.md](GETTING_STARTED.md)
- **Environment Example:** [.env.example](./.env.example)
- **Hybrid Ranking Notes:** [docs/hybrid-ranking.md](./docs/hybrid-ranking.md)
- **Contributing:** [CONTRIBUTING.md](CONTRIBUTING.md)

---

## Star History

[![Star History Chart](https://api.star-history.com/svg?repos=senoldogann/LLM-Context-Manager&type=Date)](https://star-history.com/#senoldogann/LLM-Context-Manager&Date)

---

## License

MIT License - Open source and free to use.
