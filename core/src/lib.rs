pub mod engine;
pub mod graph;
pub mod memory;
pub mod parser;
pub mod rpc;
pub mod server;
pub mod vector;

use crate::engine::{CursorPosition, RetrievalEngine};
use crate::graph::CodeGraph;
use crate::parser::{CodeParser, SupportedLanguage};
use crate::vector::store::LanceDbStore;
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

pub fn init() {
    println!("CCM Core Initialized");
}

/// Facade function to run a query against the core engine.
/// This initializes a temporary graph and store for demonstration purposes.
/// In a real scenario, this would connect to a running server or load persisted state.
pub async fn run_query(text: &str) -> Result<()> {
    println!("Initializing Core Engine...");
    let mut graph = CodeGraph::new();
    let store = LanceDbStore::new("data/ccm_db", "code_vectors").await?;

    // Check if query is in file:line format
    let parts: Vec<&str> = text.split(':').collect();
    if parts.len() >= 2 {
        let file_path = parts[0];
        let line = parts[1].parse::<usize>().context("Invalid line number")?;

        println!("Processing Query for file: {}, line: {}", file_path, line);

        if Path::new(file_path).exists() {
            // Populate graph for the file
            populate_graph_for_file(&mut graph, file_path)?;

            let engine = RetrievalEngine::new(std::sync::Arc::new(graph), store);

            let cursor = CursorPosition {
                file_path: file_path.to_string(),
                line,
                column: 0,
            };

            let suggestions = engine.predict_context(&cursor).await?;

            println!("\n--- Suggested Context ---");
            if suggestions.is_empty() {
                println!("No context found.");
            } else {
                for (i, suggestion) in suggestions.iter().enumerate() {
                    println!(
                        "\n#{}: {} (Score: {:.2})",
                        i + 1,
                        suggestion.title,
                        suggestion.relevance_score
                    );
                    println!("Reason: {}", suggestion.reason);
                    println!(
                        "Content Snippet:\n{}",
                        suggestion
                            .content
                            .lines()
                            .take(5)
                            .collect::<Vec<_>>()
                            .join("\n")
                    );
                    if suggestion.content.lines().count() > 5 {
                        println!("...");
                    }
                }
            }
        } else {
            println!("File not found: {}", file_path);
        }
    } else {
        println!("Processing Semantic Query: '{}'", text);
        let engine = RetrievalEngine::new(std::sync::Arc::new(graph), store);

        match engine.search_code(text, 5).await {
            Ok(results) => {
                if results.is_empty() {
                    println!("No semantic matches found (Database might be empty).");
                } else {
                    println!("\n--- Semantic Search Results ---");
                    for (i, result) in results.iter().enumerate() {
                        println!(
                            "#{}: {} (Score: {:.4})",
                            i + 1,
                            result.title,
                            result.relevance_score
                        );
                    }
                }
            }
            Err(e) => println!("Error executing semantic search: {}", e),
        }
    }

    Ok(())
}

/// Index a directory recursively.
/// Parses all supported files and stores embeddings in the vector database.
pub async fn index_directory(path: &str, db_path: Option<&str>) -> Result<IndexStats> {
    use walkdir::WalkDir;

    let db_path = db_path.unwrap_or("data/ccm_db");
    eprintln!("Indexing directory: {}", path);
    eprintln!("Using database: {}", db_path);

    let mut graph = CodeGraph::new();
    let store = LanceDbStore::new(db_path, "code_vectors").await?;

    let mut stats = IndexStats::default();

    // Walk directory recursively
    for entry in WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let file_path = entry.path();
        let file_path_str = file_path.to_string_lossy().to_string();

        // Skip hidden files and common non-code directories
        if file_path_str.contains("/.")
            || file_path_str.contains("/target/")
            || file_path_str.contains("/node_modules/")
            || file_path_str.contains("/.git/")
        {
            continue;
        }

        // Check if file is supported
        let extension = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let is_supported = matches!(extension, "rs" | "py" | "ts" | "js");

        if is_supported {
            match populate_graph_for_file(&mut graph, &file_path_str) {
                Ok(_) => {
                    stats.files_indexed += 1;
                    eprintln!("  ✓ {}", file_path_str);
                }
                Err(e) => {
                    stats.files_failed += 1;
                    eprintln!("  ✗ {} ({})", file_path_str, e);
                }
            }
        }
    }

    // Count nodes
    stats.nodes_created = graph.graph.node_count();

    // Index into vector store
    if stats.nodes_created > 0 {
        use std::sync::Arc;
        let graph_arc = Arc::new(graph.clone()); // Still need clone because graph is used below for save_to_file
        let engine = RetrievalEngine::new(graph_arc, store);
        engine.index_graph().await?;
        eprintln!(
            "\n✓ Indexed {} nodes from {} files",
            stats.nodes_created, stats.files_indexed
        );

        // PERSISTENCE: Save graph to disk
        let parent_dir = std::path::Path::new(db_path).parent().ok_or_else(|| {
            anyhow::anyhow!(
                "Invalid DB path '{}': cannot determine parent directory",
                db_path
            )
        })?;

        let graph_path = format!("{}/ccm_graph.json", parent_dir.to_string_lossy());
        match graph.save_to_file(&graph_path) {
            Ok(_) => eprintln!("✓ Graph saved to: {}", graph_path),
            Err(e) => eprintln!("⚠ Failed to save graph: {}", e),
        }
    } else {
        eprintln!("\n⚠ No supported files found to index");
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
    let lang = if file_path.ends_with(".rs") {
        SupportedLanguage::Rust
    } else if file_path.ends_with(".py") {
        SupportedLanguage::Python
    } else if file_path.ends_with(".ts") || file_path.ends_with(".js") {
        SupportedLanguage::TypeScript
    } else {
        return Ok(());
    };

    // Parse AST
    let mut parser = CodeParser::new();
    let tree = parser.parse_tree(&content, lang)?;

    // PASS 1: Extract definitions (Files, Functions, Classes, etc.)
    let mut extractor = Extractor::new(content.clone(), lang);
    extractor.extract(&tree, graph, file_path)?;

    // PASS 2: Extract references (Function Calls -> Calls edges)
    let edges_created = extractor.extract_references(&tree, graph)?;
    if edges_created > 0 {
        eprintln!("    ↳ Linked {} call edges", edges_created);
    }

    Ok(())
}
