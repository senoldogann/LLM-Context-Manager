use crate::graph::{CodeGraph, CodeNode, NodeType};
use crate::vector::store::LanceDbStore;
use anyhow::Result;
use petgraph::visit::EdgeRef;

/// Represents the user's cursor position in the editor.
#[derive(Debug, Clone)]
pub struct CursorPosition {
    pub file_path: String,
    pub line: usize,
    pub column: usize,
}

/// A suggested code context item.
#[derive(Debug, Clone)]
pub struct SuggestedContext {
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

use std::sync::Arc;

/// The main intelligence engine for speculative retrieval.
pub struct RetrievalEngine {
    graph: Arc<CodeGraph>,
    store: LanceDbStore,
}

impl RetrievalEngine {
    pub fn new(graph: Arc<CodeGraph>, store: LanceDbStore) -> Self {
        Self { graph, store }
    }

    /// Indexes the current graph into the vector store.
    /// This should be called after parsing/populating the graph.
    pub async fn index_graph(&self) -> Result<()> {
        let mut ids = Vec::new();
        let mut texts = Vec::new();

        // Iterate over all nodes in the graph
        for node in self.graph.graph.node_weights() {
            // Only index Functions and Classes, skip Files (too large) or Variables (too small)
            if matches!(node.node_type, NodeType::Function | NodeType::Class) {
                // Combine name and content for better embedding context
                let text_representation = format!("{}\n{}", node.name, node.content);

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

    /// Performs a purely semantic search using vectors.
    pub async fn search_code(&self, query: &str, limit: usize) -> Result<Vec<SuggestedContext>> {
        let hits = self.store.search(query, limit).await?;

        let mut results = Vec::new();
        for (content, score) in hits {
            // Distance in LanceDB is usually L2 or Cosine distance.
            // Lower is better for L2, higher is better for Cosine similarity.
            // Assuming default L2 for now, we invert/normalize simplified score.
            // Ideally we should lookup the Node in the graph by ID, but store returns text/id.

            // For this quick implementation, we trust the store returns text.
            // We can improve this by storing Node ID in vector db and fetching full Node from graph.

            results.push(SuggestedContext {
                title: "Semantic Match".to_string(), // TODO: Get real name from metadata
                content,
                relevance_score: 1.0 - score, // Rough normalization
                reason: format!("Vector Similarity (Dist: {:.4})", score),
            });
        }

        Ok(results)
    }

    /// Retrieves a node from the graph by its ID.
    pub fn get_node_by_id(&self, id: &str) -> Option<&CodeNode> {
        self.graph.find_node_by_id(id)
    }

    /// Retrieves neighbors of a node, categorized by relationship type.
    /// Returns (calls: nodes this function calls, called_by: nodes that call this function)
    pub fn get_node_neighbors(&self, id: &str) -> Option<NodeNeighbors> {
        use crate::graph::EdgeType;
        use petgraph::Direction;

        let node_idx = self.graph.find_node_index_by_id(id)?;

        let mut calls = Vec::new();
        let mut called_by = Vec::new();
        let mut contains = Vec::new();

        // Outgoing edges: This node CALLS others
        for edge in self
            .graph
            .graph
            .edges_directed(node_idx, Direction::Outgoing)
        {
            let target_node = &self.graph.graph[edge.target()];
            match edge.weight() {
                EdgeType::Calls => calls.push(target_node.name.clone()),
                EdgeType::Contains => contains.push(target_node.name.clone()),
                _ => {}
            }
        }

        // Incoming edges: Others CALL this node
        for edge in self
            .graph
            .graph
            .edges_directed(node_idx, Direction::Incoming)
        {
            let source_node = &self.graph.graph[edge.source()];
            if let EdgeType::Calls = edge.weight() {
                called_by.push(source_node.name.clone());
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
    pub async fn predict_context(&self, cursor: &CursorPosition) -> Result<Vec<SuggestedContext>> {
        let mut suggestions = Vec::new();

        // 1. Spatial Query: Find which node (Function/Class) the cursor is in.
        if let Some(node_idx) = self.graph.find_node_in_file(&cursor.file_path, cursor.line) {
            let current_node = &self.graph.graph[node_idx];

            // Add the current node itself as context
            suggestions.push(SuggestedContext {
                title: format!("Current: {}", current_node.name),
                content: current_node.content.clone(),
                relevance_score: 1.0,
                reason: "Active Focus".to_string(),
            });

            // 2. Structural Retrieval: Find immediate neighbors (Callers/Callees)
            let neighbors = self.graph.graph.neighbors(node_idx);
            for neighbor_idx in neighbors {
                let neighbor = &self.graph.graph[neighbor_idx];

                if neighbor.node_type == NodeType::File {
                    continue;
                }

                suggestions.push(SuggestedContext {
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
