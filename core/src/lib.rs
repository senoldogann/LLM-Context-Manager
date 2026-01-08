pub mod engine;
pub mod graph;
pub mod memory;
pub mod parser;
pub mod server;
pub mod vector;

pub fn init() {
    println!("CCM Core Initialized");
}

/// Facade function to run a query against the core engine.
/// This initializes a temporary graph and store for demonstration purposes.
/// In a real scenario, this would connect to a running server or load persisted state.
pub async fn run_query(text: &str) -> anyhow::Result<()> {
    use crate::engine::RetrievalEngine;
    use crate::graph::CodeGraph;
    use crate::vector::store::LanceDbStore;

    println!("Initializing Core Engine...");
    let graph = CodeGraph::new();
    let store = LanceDbStore::new("data/ccm_db", "code_vectors").await?;
    let _engine = RetrievalEngine::new(graph, store);

    // Mock cursor for CLI query - interpreting "query" as searching for context around a file/line
    // If text is just a string, we might need semantic search which isn't fully implemented yet.
    // For now, let's assume the user passes "file:line" or just text.
    println!("Processing Query: '{}'", text);

    // TODO: Parse text to create a CursorPosition or implement semantic search in Engine.
    // For validation, we'll just log that we reached the core.

    Ok(())
}
