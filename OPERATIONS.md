# OPERATIONS.md (Usage Guide)

## 1. Development Principles (SPAP v2.2)
This project follows strict architectural rules:
-   **Core Independence:** The `core` crate must not depend on `cli` or `mcp`. It is the pure logic layer.
-   **Observability:** Uses `tracing` crate. No `println!` allowed in library code.
-   **Error Handling:** Uses `thiserror` for libraries, `anyhow` for binaries.

## 2. Standard Workflows

### 🛠️ Building the Project
```bash
# Build all crates (Core, MCP, CLI)
cargo build

# Run Tests
cargo test
```

### 🚀 Releasing a New Version
1.  **Test:** Ensure `cargo test` passes.
2.  **Lint:** Run `cargo clippy`.
3.  **Publish:** 
    - Update version in `Cargo.toml` files.
    - Run `npm publish` in the `npm/` directory for the wrapper.

### 🐛 Debugging
To see detailed logs during development, use the `RUST_LOG` environment variable:
```bash
RUST_LOG=debug cargo run --bin ccm-mcp
```

## 3. Project Structure
-   **`core/`**: The brain. Handles parsing, vector storage (LanceDB), and graph logic.
-   **`mcp/`**: The interface. Exposes `core` functionality via Model Context Protocol.
-   **`cli/`**: The utility. Manual tools for indexing and querying.
