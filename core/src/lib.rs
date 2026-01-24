pub mod engine;
pub mod error;
pub mod git;
pub mod graph;
pub mod memory;
pub mod parser;
pub mod rpc;
pub mod server;
pub mod storage;
pub mod vector;

use crate::engine::{CursorPosition, RetrievalEngine};
use crate::graph::CodeGraph;
use crate::parser::{CodeParser, SupportedLanguage};
use crate::vector::store::LanceDbStore;
use anyhow::Result;
use std::fs;

pub fn init() {
    println!("CCM Core Initialized");
}

// Re-export ContextSuggestion for external use
pub use crate::engine::ContextSuggestion;

/// Run a semantic search query against the index.
/// Returns a list of context suggestions.
pub async fn run_query(query: &str, project_path: &str) -> Result<Vec<ContextSuggestion>> {
    tracing::info!(
        query = query,
        project = project_path,
        "Running semantic query"
    );

    // In production, you wouldn't rebuild valid state every time.
    // This assumes the DB exists at project_path/data/ccm_db
    // and the graph is loaded from project_path/data/ccm_graph.json

    let db_path = std::path::Path::new(project_path).join("data/ccm_db");
    let db_path_str = db_path.to_string_lossy().to_string();

    // Check if DB exists
    if !db_path.exists() {
        return Err(anyhow::anyhow!(
            "Index not found. Please run indexing first."
        ));
    }

    // Load graph (for simplicity in this prototype refactor we create new,
    // but ideally we load from disk)
    let graph_path = std::path::Path::new(project_path).join("data/ccm_graph.json");

    let graph = if graph_path.exists() {
        // Load graph logic would go here
        // For now, we initialize empty and warn, as full persistence refactor is next
        tracing::debug!("Graph file found at {}, loading...", graph_path.display());
        CodeGraph::from_file(&graph_path.to_string_lossy())?
    } else {
        tracing::warn!("Graph file not found, creating new");
        CodeGraph::new()
    };

    let store = LanceDbStore::new(&db_path_str, "code_vectors").await?;
    let engine = RetrievalEngine::new(std::sync::Arc::new(tokio::sync::RwLock::new(graph)), store);

    // If query looks like file:line, do cursor prediction
    if query.contains(':') && !query.contains(' ') {
        let parts: Vec<&str> = query.split(':').collect();
        if parts.len() == 2 {
            if let Ok(line) = parts[1].parse::<usize>() {
                let file_path = parts[0];
                // Resolve absolute path or relative to project
                // Simple logic for now:
                let cursor = CursorPosition {
                    file_path: file_path.to_string(),
                    line,
                    column: 0,
                };
                return engine.predict_context(&cursor).await;
            }
        }
    }

    // Default: Semantic Search
    let results = engine.search_code(query, 5).await?;
    Ok(results)
}

/// Index a directory recursively.
/// Parses all supported files and stores embeddings in the vector database.
pub async fn index_directory(path: &str, db_path: Option<&str>) -> Result<IndexStats> {
    use ignore::WalkBuilder;
    use tracing::{error, info, warn};

    // If db_path is provided, use it. Otherwise default to path/data/ccm_db
    let default_db_path = std::path::Path::new(path).join("data/ccm_db");
    let db_path_buf = db_path
        .map(std::path::PathBuf::from)
        .unwrap_or(default_db_path);
    let db_path_str = db_path_buf.to_string_lossy().to_string();

    info!(path = path, db_path = %db_path_str, "Starting directory indexing");

    let mut graph = CodeGraph::new();
    let store = LanceDbStore::new(&db_path_str, "code_vectors").await?;

    let mut stats = IndexStats::default();

    // Create a walker that respects .gitignore
    let walker = WalkBuilder::new(path)
        .hidden(false) // Still scan hidden files if needed, but respect gitignore
        .git_ignore(true)
        .build();

    // Walk directory recursively
    for result in walker {
        match result {
            Ok(entry) => {
                if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                    continue;
                }

                let file_path = entry.path();
                let file_path_str = file_path.to_string_lossy().to_string();

                // Universal Support: We attempt to index ALL files returned by the ignore-walker.

                match populate_graph_for_file(&mut graph, &file_path_str) {
                    Ok(_) => {
                        stats.files_indexed += 1;
                    }
                    Err(e) => {
                        stats.files_failed += 1;
                        tracing::debug!(file = %file_path_str, error = %e, "Skipped file");
                    }
                }
            }
            Err(err) => {
                warn!("Error during directory traversal: {}", err);
            }
        }
    }

    // Count nodes
    stats.nodes_created = graph.graph.node_count();

    // Index into vector store
    if stats.nodes_created > 0 {
        use std::sync::Arc;
        let graph_arc = Arc::new(tokio::sync::RwLock::new(graph.clone())); // Still need clone because graph is used below for save_to_file
        let engine = RetrievalEngine::new(graph_arc, store);
        engine.index_graph().await?;

        info!(
            nodes = stats.nodes_created,
            files = stats.files_indexed,
            "Indexing completed successfully"
        );

        // PERSISTENCE: Save graph to disk
        let parent_dir = std::path::Path::new(&db_path_str).parent().ok_or_else(|| {
            anyhow::anyhow!(
                "Invalid DB path '{}': cannot determine parent directory",
                db_path_str
            )
        })?;

        let graph_path = format!("{}/ccm_graph.json", parent_dir.to_string_lossy());
        match graph.save_to_file(&graph_path) {
            Ok(_) => info!(path = %graph_path, "Graph saved to disk"),
            Err(e) => error!(error = %e, "Failed to save graph"),
        }
    } else {
        warn!("No supported files found to index");
    }

    Ok(stats)
}

/// Statistics from an indexing operation
#[derive(Debug, Default)]
pub struct IndexStats {
    pub files_indexed: usize,
    pub files_failed: usize,
    pub nodes_created: usize,
}

fn populate_graph_for_file(graph: &mut CodeGraph, file_path: &str) -> Result<()> {
    use crate::vector::extractor::Extractor;

    let content = fs::read_to_string(file_path)?;

    // Determine language
    // Determine language
    let lang = if file_path.ends_with(".rs") {
        SupportedLanguage::Rust
    } else if file_path.ends_with(".py") {
        SupportedLanguage::Python
    } else if file_path.ends_with(".ts")
        || file_path.ends_with(".js")
        || file_path.ends_with(".tsx")
        || file_path.ends_with(".jsx")
    {
        SupportedLanguage::TypeScript
    } else {
        // Fallback for everything else (md, json, yaml, txt, etc.)
        // We treat everything else as "Data" (TextBlob)
        SupportedLanguage::Data
    };

    // If it's a Data file, we bypass the AST parser and just create a file-level node
    if matches!(lang, SupportedLanguage::Data) {
        use crate::graph::CodeNode;
        use crate::graph::NodeType;

        let node = CodeNode {
            id: file_path.to_string(),
            node_type: NodeType::File,
            name: file_path.to_string(),
            content: content.clone(),
            start_line: 1,
            end_line: content.lines().count(),
        };
        graph.add_node(node);
        return Ok(());
    }

    // Parse AST
    let mut parser = CodeParser::new();
    let tree = parser.parse_tree(&content, lang)?;

    // PASS 1: Extract definitions (Files, Functions, Classes, etc.)
    let mut extractor = Extractor::new(content.clone(), lang);
    extractor.extract(&tree, graph, file_path)?;

    // PASS 2: Extract references (Function Calls -> Calls edges)
    let edges_created = extractor.extract_references(&tree, graph)?;
    if edges_created > 0 {
        tracing::debug!("Linked {} call edges", edges_created);
    }

    Ok(())
}
