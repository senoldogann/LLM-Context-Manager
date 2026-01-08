use crate::graph::{CodeGraph, NodeType};
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
    #[allow(dead_code)]
    store: LanceDbStore,
}

impl RetrievalEngine {
    pub fn new(graph: CodeGraph, store: LanceDbStore) -> Self {
        Self { graph, store }
    }

    /// Predicts relevant code context based on the user's cursor position.
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
            // We verify outgoing edges
            let mut neighbors = self.graph.graph.neighbors(node_idx);
            while let Some(neighbor_idx) = neighbors.next() {
                let neighbor = &self.graph.graph[neighbor_idx];

                // Skip if it's the File node itself or just a small variable
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
        }

        Ok(suggestions)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{CodeNode, EdgeType};

    #[tokio::test]
    async fn test_predict_context() {
        let mut graph = CodeGraph::new();

        // Setup: File -> Function
        let file_node = CodeNode {
            id: "src/main.rs".to_string(),
            node_type: NodeType::File,
            name: "src/main.rs".to_string(),
            content: "".to_string(),
            start_line: 0,
            end_line: 100,
        };
        let file_idx = graph.add_node(file_node);

        let func_node = CodeNode {
            id: "main_func".to_string(),
            node_type: NodeType::Function,
            name: "main".to_string(),
            content: "fn main() {}".to_string(),
            start_line: 10,
            end_line: 20,
        };
        let func_idx = graph.add_node(func_node);

        // Link File -> Function
        graph.add_edge(file_idx, func_idx, EdgeType::Contains);

        // Setup Store (Mockish)
        let store = LanceDbStore::new("data/test_db", "test").await.unwrap();

        let engine = RetrievalEngine::new(graph, store);

        let cursor = CursorPosition {
            file_path: "src/main.rs".to_string(),
            line: 15, // Inside main implementation
            column: 0,
        };

        let suggestions = engine.predict_context(&cursor).await.unwrap();

        // Assertions
        assert!(!suggestions.is_empty());
        assert_eq!(suggestions[0].title, "Current: main");
    }
}
