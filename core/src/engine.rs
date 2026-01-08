use crate::graph::{CodeGraph, CodeNode, NodeType};
use crate::vector::store::LanceDbStore;
use anyhow::Result;

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

/// The main intelligence engine for speculative retrieval.
pub struct RetrievalEngine {
    graph: CodeGraph,
    store: LanceDbStore,
}

impl RetrievalEngine {
    pub fn new(graph: CodeGraph, store: LanceDbStore) -> Self {
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
            let mut neighbors = self.graph.graph.neighbors(node_idx);
            while let Some(neighbor_idx) = neighbors.next() {
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
