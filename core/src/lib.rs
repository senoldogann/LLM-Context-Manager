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

    // If db_path is provided, use it. Otherwise default to path/data/ccm_db
    let default_db_path = std::path::Path::new(path).join("data/ccm_db");
    let db_path_buf = db_path
        .map(std::path::PathBuf::from)
        .unwrap_or(default_db_path);
    let db_path_str = db_path_buf.to_string_lossy().to_string();

    eprintln!("Indexing directory: {}", path);
    eprintln!("Using database: {}", db_path_str);

    let mut graph = CodeGraph::new();
    let store = LanceDbStore::new(&db_path_str, "code_vectors").await?;

    let mut stats = IndexStats::default();

    // Walk directory recursively
    for entry in WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let file_path = entry.path();
        let file_path_str = file_path.to_string_lossy().to_string();

        // Check if file is supported
        let extension = file_path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let is_supported = matches!(
            extension,
            "rs" | "py" | "ts" | "js" | "tsx" | "jsx" | "md" | "json" | "yaml" | "yml" | "toml"
        );

        // Robust ignore check using path components
        let should_ignore = file_path.components().any(|c| {
            if let std::path::Component::Normal(os_str) = c {
                let s = os_str.to_string_lossy();
                s == "target"
                    || s == "node_modules"
                    || s == ".git"
                    || s == "dist"
                    || s == "build"
                    || s == ".next"
                    || s.starts_with('.')
            } else {
                false
            }
        });

        if should_ignore {
            continue;
        }

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
        let parent_dir = std::path::Path::new(&db_path_str).parent().ok_or_else(|| {
            anyhow::anyhow!(
                "Invalid DB path '{}': cannot determine parent directory",
                db_path_str
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
    } else if file_path.ends_with(".ts")
        || file_path.ends_with(".js")
        || file_path.ends_with(".tsx")
        || file_path.ends_with(".jsx")
    {
        SupportedLanguage::TypeScript
    } else if file_path.ends_with(".md")
        || file_path.ends_with(".json")
        || file_path.ends_with(".yaml")
        || file_path.ends_with(".yml")
        || file_path.ends_with(".toml")
    {
        SupportedLanguage::Data
    } else {
        return Ok(());
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
        eprintln!("    ↳ Linked {} call edges", edges_created);
    }

    Ok(())
}
