pub mod engine;
pub mod graph;
pub mod memory;
pub mod parser;
pub mod rpc;
pub mod server;
pub mod vector;

use crate::engine::{CursorPosition, RetrievalEngine};
use crate::graph::{CodeGraph, CodeNode, EdgeType, NodeType};
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

            let engine = RetrievalEngine::new(graph, store);

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
        let engine = RetrievalEngine::new(graph, store);

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

fn populate_graph_for_file(graph: &mut CodeGraph, file_path: &str) -> Result<()> {
    let content = fs::read_to_string(file_path)?;
    let total_lines = content.lines().count();

    // Add File Node
    let file_node = CodeNode {
        id: file_path.to_string(),
        node_type: NodeType::File,
        name: file_path.to_string(),
        content: content.clone(),
        start_line: 0,
        end_line: total_lines,
    };
    let file_idx = graph.add_node(file_node);

    // Determine language
    let lang = if file_path.ends_with(".rs") {
        SupportedLanguage::Rust
    } else if file_path.ends_with(".py") {
        SupportedLanguage::Python
    } else if file_path.ends_with(".ts") {
        SupportedLanguage::TypeScript
    } else {
        return Ok(());
    };

    // Parse and add functions
    let mut parser = CodeParser::new();
    let tree = parser.parse_tree(&content, lang)?;
    let root = tree.root_node();
    let mut cursor = root.walk();

    for child in root.children(&mut cursor) {
        let kind = child.kind();
        // Simplified: only looking for top-level functions
        let is_function = match lang {
            SupportedLanguage::Rust => kind == "function_item",
            SupportedLanguage::Python => kind == "function_definition",
            SupportedLanguage::TypeScript => kind == "function_declaration",
        };

        if is_function {
            let start_line = child.start_position().row + 1; // 1-based for user
            let end_line = child.end_position().row + 1;

            // Extract name (simplified)
            let name_node = child.child_by_field_name("name");
            let name = if let Some(n) = name_node {
                &content[n.byte_range()]
            } else {
                "anonymous"
            };

            let func_node = CodeNode {
                id: format!("{}::{}", file_path, name),
                node_type: NodeType::Function,
                name: name.to_string(),
                content: content[child.byte_range()].to_string(),
                start_line,
                end_line,
            };

            let func_idx = graph.add_node(func_node);
            graph.add_edge(file_idx, func_idx, EdgeType::Contains);
        }
    }

    Ok(())
}
