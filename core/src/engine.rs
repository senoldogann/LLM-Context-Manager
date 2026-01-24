pub mod predictive;

use crate::git::GitIntegrator;
use crate::graph::{CodeGraph, CodeNode, NodeType};
use crate::parser::CodeParser;
use crate::parser::SupportedLanguage;
use crate::vector::extractor::Extractor;
use crate::vector::store::LanceDbStore;
use anyhow::Result;
use petgraph::visit::EdgeRef;

use tokio::sync::RwLock;

/// Represents the user's cursor position in the editor.
#[derive(Debug, Clone)]
pub struct CursorPosition {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
}

/// A suggested code context item.
#[derive(Debug, Clone)]
pub struct ContextSuggestion {
    pub title: String,
    pub content: String,
    pub relevance_score: f32,
    pub reason: String,
}

/// Neighbors of a node in the code graph, categorized by relationship.
#[derive(Debug, Clone, Default)]
pub struct NodeNeighbors {
    pub calls: Vec<String>,     // Functions this node calls
    pub called_by: Vec<String>, // Functions that call this node
    pub contains: Vec<String>,  // Child nodes (for files/classes)
}

use crate::engine::predictive::SpeculativeCache;
use std::sync::Arc;

/// The main intelligence engine for speculative retrieval.
pub struct RetrievalEngine {
    pub graph: Arc<RwLock<CodeGraph>>,
    store: LanceDbStore,
    #[allow(dead_code)]
    cache: Arc<SpeculativeCache>,
}

impl RetrievalEngine {
    pub fn new(graph: Arc<RwLock<CodeGraph>>, store: LanceDbStore) -> Self {
        Self {
            graph,
            store,
            cache: Arc::new(SpeculativeCache::new(500)),
        }
    }

    /// Indexes the current graph into the vector store.
    /// This should be called after parsing/populating the graph.
    pub async fn index_graph(&self) -> Result<()> {
        let mut ids = Vec::new();
        let mut texts = Vec::new();

        let graph = self.graph.read().await;

        // Iterate over all nodes in the graph
        for node in graph.graph.node_weights() {
            // Index Functions, Classes, Structs, and Methods.
            // We consciously exclude 'File' nodes from embeddings in Phase 3 because they are too large
            // and duplicate the content of their children. We want granular retrieval.
            if matches!(
                node.node_type,
                NodeType::Function | NodeType::Method | NodeType::Class | NodeType::Struct
            ) {
                // Combine name, type, and content for better embedding context
                // Format: "Type: Name \n Content"
                // Content already includes Docstring from Extractor.
                let text_representation =
                    format!("{:?}: {}\n{}", node.node_type, node.name, node.content);

                // Basic Token Limit Check (Approx 1 token ~= 4 chars)
                // If node is > 8000 chars (~2k tokens), we might want to split it further or truncated?
                // For Phase 3, we index it but log a warning if extremely large.
                if text_representation.len() > 8000 {
                    eprintln!("Warning: Node {} is very large ({} chars). Retrieval accuracy might degrade.", node.id, text_representation.len());
                }

                ids.push(node.id.clone());
                texts.push(text_representation);
            }
        }

        if !ids.is_empty() {
            eprintln!("Indexing {} nodes into vector store...", ids.len());
            self.store.add_documents(ids, texts).await?;
        }

        Ok(())
    }

    /// Performs incremental indexing using Git status.
    pub async fn incremental_index(&self, project_root: &str) -> Result<()> {
        let git = GitIntegrator::new(project_root)?;
        let changed_files = git.get_changed_files()?;

        if changed_files.is_empty() {
            eprintln!("No changes detected.");
            return Ok(());
        }

        eprintln!(
            "Incremental Indexing: Found {} changed files.",
            changed_files.len()
        );

        let mut parser = CodeParser::new();
        let mut nodes_to_index = Vec::new(); // Collect new nodes for vector DB

        let root_path = std::fs::canonicalize(project_root)
            .unwrap_or_else(|_| std::path::PathBuf::from(project_root));

        for path in changed_files {
            // Canonicalize file path to ensure it matches root
            let abs_path = std::fs::canonicalize(&path).unwrap_or(path.clone());

            let relative_path = abs_path
                .strip_prefix(&root_path)
                .unwrap_or_else(|_| abs_path.strip_prefix(project_root).unwrap_or(&abs_path))
                .to_string_lossy()
                .to_string();

            eprintln!("Processing: {}", relative_path);

            // 1. GARBAGE COLLECTION: Delete old vectors for this file
            // Now that Extractor uses file-prefixed IDs ("path/to/file:kind:row..."),
            // we can safely delete all vectors belonging to this file.
            if let Err(e) = self.store.delete_by_prefix(&relative_path).await {
                eprintln!(
                    "Warning: Failed to perform vector GC for {}: {}",
                    relative_path, e
                );
            }

            {
                // Write Lock Scope: Remove old nodes
                // This removes them from the Graph structure (RAM/Disk)
                let mut graph = self.graph.write().await;
                graph.remove_file_nodes(&relative_path);
            }

            // Parse File
            if let Ok(content) = std::fs::read_to_string(&abs_path) {
                // Detect language
                let lang = if path.extension().is_some_and(|e| e == "rs") {
                    SupportedLanguage::Rust
                } else if path.extension().is_some_and(|e| e == "py") {
                    SupportedLanguage::Python
                } else if path.extension().is_some_and(|e| e == "ts") {
                    SupportedLanguage::TypeScript
                } else {
                    SupportedLanguage::Data
                };

                if let Ok(tree) = parser.parse_tree(&content, lang) {
                    let mut extractor = Extractor::new(content.clone(), lang);

                    // Write Lock Scope: Update Graph
                    let mut graph = self.graph.write().await;

                    if let Ok(_file_idx) = extractor.extract(&tree, &mut graph, &relative_path) {
                        // Collect ALL new nodes for this file
                        // Extractor doesn't return them. We have to query them back or maintain state.
                        // Or simply scan the graph for nodes belonging to this file?

                        // Efficient scan:
                        if let Some(file_node_idx) = graph.find_file_node(&relative_path) {
                            // DFS to find children
                            // This duplicates `remove_file_nodes` logic but for collection.
                            let mut stack = vec![file_node_idx];
                            while let Some(idx) = stack.pop() {
                                let node = &graph.graph[idx];

                                // Filter for semantic nodes
                                if matches!(
                                    node.node_type,
                                    NodeType::Function
                                        | NodeType::Method
                                        | NodeType::Class
                                        | NodeType::Struct
                                ) {
                                    nodes_to_index.push(node.clone());
                                }

                                // Children
                                let neighbors = graph.graph.neighbors(idx);
                                for n in neighbors {
                                    // Only follow Contains edges?
                                    // Graph neighbors iterator includes all outgoing edges.
                                    // We need to filter edge type.
                                    // Getting edge weight requires `edges_directed`.
                                    // Simpler: Just rely on the fact we just added them?
                                    // Or use `Recurse` logic.
                                    stack.push(n);
                                    // Warning: Infinite loop if cycles. CodeGraph (AST) is a Tree (DAG), so OK.
                                }
                            }
                        }
                    }
                }
            }
        }

        // Remove duplicates from nodes_to_index (DFS visits multiple times?)
        // `graph.neighbors` returns unique neighbor indices. But if multiple paths?
        // AST is a Tree. No multi-parent.

        // Batch Index New Nodes
        if !nodes_to_index.is_empty() {
            eprintln!(
                "Incremental: Indexing {} new semantic nodes...",
                nodes_to_index.len()
            );
            let ids: Vec<String> = nodes_to_index.iter().map(|n| n.id.clone()).collect();
            let texts: Vec<String> = nodes_to_index
                .iter()
                .map(|n| format!("{:?}: {}\n{}", n.node_type, n.name, n.content))
                .collect();

            // Deduplicate?

            self.store.add_documents(ids, texts).await?;
        }

        eprintln!("Incremental update complete.");

        Ok(())
    }

    /// Performs a purely semantic search using vectors.
    pub async fn search_code(&self, query: &str, limit: usize) -> Result<Vec<ContextSuggestion>> {
        let hits = self.store.search(query, limit).await?;

        let mut results = Vec::new();
        for (id, content, score) in hits {
            // Distance in LanceDB is usually L2 or Cosine distance.
            // Lower is better for L2, higher is better for Cosine similarity.
            // Assuming default L2 for now, we invert/normalize simplified score.

            // 1. Strip chunk suffix if present (e.g., "func:10:20#chunk0" -> "func:10:20")
            let node_id = id.split('#').next().unwrap_or(&id);

            // 2. Lookup the real Node in the Graph to get metadata (Name, Type)
            let mut title = "Semantic Match".to_string();
            if let Some(node) = self.get_node_by_id(node_id).await {
                title = format!("{:?}: {}", node.node_type, node.name);
            }

            results.push(ContextSuggestion {
                title,
                content,
                relevance_score: 1.0 - score, // Rough normalization
                reason: format!("Vector Similarity (Dist: {:.4})", score),
            });
        }

        Ok(results)
    }

    /// Retrieves a node from the graph by its ID.
    pub async fn get_node_by_id(&self, id: &str) -> Option<CodeNode> {
        self.graph.read().await.find_node_by_id(id)
    }

    /// Retrieves neighbors of a node, categorized by relationship type.
    /// Returns (calls: nodes this function calls, called_by: nodes that call this function)
    pub async fn get_node_neighbors(&self, id: &str) -> Option<NodeNeighbors> {
        use crate::graph::EdgeType;
        // ... Logic update needed for hybrid ...
        // For now, we still rely on finding index for RAM neighbors?
        // CodeGraph::get_outgoing_edges returns a Vec.

        let mut calls = Vec::new();
        let mut called_by = Vec::new();
        let mut contains = Vec::new();

        // Hybrid Outgoing Edges
        let outgoing = { self.graph.read().await.get_outgoing_edges(id) };

        for (target_id, weight) in outgoing {
            // We need names? get_outgoing_edges returns TargetID.
            // We need to lookup the node to get the name?
            // Or Sled stored Edge has ID.
            // RetrievalEngine::get_node_neighbors returns NAMES?
            // "calls.push(target_node.name.clone())"

            // This is expensive if we have to fetch every node to get its name.
            // But for correctness, yes.
            if let Some(node) = self.get_node_by_id(&target_id).await {
                match weight {
                    EdgeType::Calls => calls.push(node.name),
                    EdgeType::Contains => contains.push(node.name),
                    _ => {}
                }
            }
        }

        // Incoming edges (called_by) - Only RAM supported or scan?
        // CodeGraph currently only exposes get_outgoing_edges from storage.
        // So we keep RAM based incoming for now?
        if let Some(idx) = self.graph.read().await.find_node_index_by_id(id) {
            use petgraph::Direction;
            // Incoming edges: Others CALL this node
            let graph = self.graph.read().await;
            for edge in graph.graph.edges_directed(idx, Direction::Incoming) {
                let source_node = &graph.graph[edge.source()];
                if let EdgeType::Calls = edge.weight() {
                    called_by.push(source_node.name.clone());
                }
            }
        }

        Some(NodeNeighbors {
            calls,
            called_by,
            contains,
        })
    }

    /// Predicts relevant code context based on the user's cursor position.
    /// Uses a hybrid approach: Graph (Structural) + Vector (Semantic).
    pub async fn predict_context(&self, cursor: &CursorPosition) -> Result<Vec<ContextSuggestion>> {
        let mut suggestions = Vec::new();

        // 1. Spatial Query: Find which node (Function/Class) the cursor is in.
        let node_idx_opt = self
            .graph
            .read()
            .await
            .find_node_in_file(&cursor.file_path, cursor.line);
        if let Some(node_idx) = node_idx_opt {
            let graph = self.graph.read().await;
            let current_node = &graph.graph[node_idx];

            // Add the current node itself as context
            suggestions.push(ContextSuggestion {
                title: format!("Current: {}", current_node.name),
                content: current_node.content.clone(),
                relevance_score: 1.0,
                reason: "Active Focus".to_string(),
            });

            // 2. Structural Retrieval: Find immediate neighbors (Callers/Callees)
            let neighbors = graph.graph.neighbors(node_idx);
            for neighbor_idx in neighbors {
                let neighbor = &graph.graph[neighbor_idx];

                if neighbor.node_type == NodeType::File {
                    continue;
                }

                suggestions.push(ContextSuggestion {
                    title: format!("Related: {}", neighbor.name),
                    content: neighbor.content.clone(),
                    relevance_score: 0.8,
                    reason: "Structural Relation".to_string(),
                });
            }

            // 3. Semantic Retrieval (Hybrid): "What else is like this function?"
            // We use the current function's signature/docstring as a query
            let query = format!("related to {}", current_node.name);
            // Limiting to top 2 semantic matches to avoid noise
            if let Ok(semantic_hits) = self.search_code(&query, 2).await {
                for mut hit in semantic_hits {
                    hit.reason = "Semantic Similarity to Active Node".to_string();
                    hit.relevance_score *= 0.7; // Weigh semantic less than structural
                    suggestions.push(hit);
                }
            }
        }

        Ok(suggestions)
    }
}
