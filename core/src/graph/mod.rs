use crate::storage::GraphStorage;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NodeType {
    File,
    DataFile,
    Module,
    Class,
    Function,
    Method,
    Variable,
    Import,
    Struct,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeNode {
    pub id: String,
    pub node_type: NodeType,
    pub name: String,
    pub content: String,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EdgeType {
    Calls,
    Defines,
    Imports,
    Contains,
    Inherits,
    Reads,
    Writes,
}

#[derive(Clone)]
pub struct CodeGraph {
    pub graph: DiGraph<CodeNode, EdgeType>,
    pub storage: Option<Arc<dyn GraphStorage>>,
}

impl Default for CodeGraph {
    fn default() -> Self {
        Self {
            graph: DiGraph::new(),
            storage: None,
        }
    }
}

impl CodeGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_storage(mut self, storage: Arc<dyn GraphStorage>) -> Self {
        self.storage = Some(storage);
        self
    }

    pub fn add_node(&mut self, node: CodeNode) -> NodeIndex {
        if let Some(storage) = &self.storage {
            if let Err(e) = storage.save_node(&node) {
                tracing::warn!(error = %e, "Failed to save node to storage");
            }
        }
        self.graph.add_node(node)
    }

    pub fn add_edge(&mut self, source: NodeIndex, target: NodeIndex, weight: EdgeType) {
        if let Some(storage) = &self.storage {
            let source_node = &self.graph[source];
            let target_node = &self.graph[target];
            if let Err(e) = storage.save_edge(&source_node.id, &target_node.id, weight.clone()) {
                tracing::warn!(error = %e, "Failed to save edge to storage");
            }
        }
        self.graph.add_edge(source, target, weight);
    }

    /// Finds the node corresponding to a specific file path.
    pub fn find_file_node(&self, file_path: &str) -> Option<NodeIndex> {
        self.graph.node_indices().find(|&idx| {
            let node = &self.graph[idx];
            matches!(node.node_type, NodeType::File | NodeType::DataFile) && node.name == file_path
        })
    }

    /// Finds the deepest node within a file hierarchy that covers the given line.
    pub fn find_node_in_file(&self, file_path: &str, line: usize) -> Option<NodeIndex> {
        let file_node_idx = self.find_file_node(file_path)?;

        // We want the most specific node (narrowest range)
        let mut best_match = file_node_idx;
        let mut min_len = usize::MAX;

        // BFS/DFS to visit all children of the file
        let mut stack = vec![file_node_idx];

        while let Some(idx) = stack.pop() {
            let node = &self.graph[idx];

            // Check if node covers the line
            if node.start_line <= line && node.end_line >= line {
                let len = node.end_line - node.start_line;
                if len < min_len {
                    min_len = len;
                    best_match = idx;
                }

                // Add children to stack to go deeper
                // We only follow output edges of type Contains
                let neighbors = self
                    .graph
                    .neighbors_directed(idx, petgraph::Direction::Outgoing);
                for neighbor_idx in neighbors {
                    let edge = self.graph.find_edge(idx, neighbor_idx);
                    if let Some(edge_idx) = edge {
                        if let Some(weight) = self.graph.edge_weight(edge_idx) {
                            if matches!(weight, EdgeType::Contains) {
                                stack.push(neighbor_idx);
                            }
                        }
                    }
                }
            }
        }

        Some(best_match)
    }
    pub fn find_node_by_id(&self, id: &str) -> Option<CodeNode> {
        // 1. Check RAM
        if let Some(node) = self.graph.node_weights().find(|node| node.id == id) {
            return Some(node.clone());
        }
        // 2. Check Storage
        if let Some(storage) = &self.storage {
            if let Ok(Some(node)) = storage.get_node(id) {
                return Some(node);
            }
        }
        None
    }

    /// Retrieves outgoing edges for a given node ID.
    /// Returns Vec<(TargetID, EdgeType)>.
    pub fn get_outgoing_edges(&self, id: &str) -> Vec<(String, EdgeType)> {
        let mut edges = Vec::new();

        // 1. Check RAM (if node exists in RAM)
        if let Some(idx) = self.find_node_index_by_id(id) {
            for edge in self
                .graph
                .edges_directed(idx, petgraph::Direction::Outgoing)
            {
                let target_node = &self.graph[edge.target()];
                edges.push((target_node.id.clone(), edge.weight().clone()));
            }
            return edges;
        }

        // 2. Check Storage
        if let Some(storage) = &self.storage {
            if let Ok(stored_edges) = storage.get_edges(id) {
                return stored_edges;
            }
        }
        edges
    }

    pub fn find_node_index_by_id(&self, id: &str) -> Option<NodeIndex> {
        self.graph.node_indices().find(|&idx| {
            let node = &self.graph[idx];
            node.id == id
        })
    }

    /// Saves the graph to a JSON file.
    pub fn save_to_file(&self, path: &str) -> anyhow::Result<()> {
        let file = std::fs::File::create(path)?;
        let writer = std::io::BufWriter::new(file);
        serde_json::to_writer(writer, &self.graph)?;
        Ok(())
    }

    /// Loads the graph from a JSON file.
    pub fn load_from_file(path: &str) -> anyhow::Result<Self> {
        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);
        let graph: DiGraph<CodeNode, EdgeType> = serde_json::from_reader(reader)?;
        Ok(Self {
            graph,
            storage: None,
        })
    }

    /// Alias for load_from_file to match API conventions
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        Self::load_from_file(path)
    }

    /// Removes all nodes belonging to a specific file.
    /// Used for incremental indexing (clearing old state).
    /// Removes all nodes belonging to a specific file.
    /// Used for incremental indexing (clearing old state).
    pub fn remove_file_nodes(&mut self, file_path: &str) {
        // 1. Find the File Node
        let file_node_idx = match self.find_file_node(file_path) {
            Some(idx) => idx,
            None => return, // File not in graph, nothing to remove
        };

        // 2. Collect all descendants via "Contains" edges (DFS)
        let mut to_remove = HashSet::new();
        let mut stack = vec![file_node_idx];

        // We include the file node itself in the removal
        to_remove.insert(file_node_idx);

        while let Some(idx) = stack.pop() {
            let neighbors = self
                .graph
                .neighbors_directed(idx, petgraph::Direction::Outgoing);
            for neighbor_idx in neighbors {
                let edge = self.graph.find_edge(idx, neighbor_idx);
                if let Some(edge_idx) = edge {
                    if let Some(weight) = self.graph.edge_weight(edge_idx) {
                        if matches!(weight, EdgeType::Contains) {
                            to_remove.insert(neighbor_idx);
                            stack.push(neighbor_idx);
                        }
                    }
                }
            }
        }

        // 3. Remove nodes using retain_nodes to avoid index swaps.
        self.graph.retain_nodes(|_, idx| !to_remove.contains(&idx));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_node() {
        let mut graph = CodeGraph::new();
        let node = CodeNode {
            id: "test_func".to_string(),
            node_type: NodeType::Function,
            name: "test".to_string(),
            content: "fn test() {}".to_string(),
            start_line: 1,
            end_line: 1,
        };
        let index = graph.add_node(node);
        assert_eq!(index.index(), 0);
    }

    #[test]
    fn remove_file_nodes_only_removes_target_file() {
        let mut graph = CodeGraph::new();

        let file_a_idx = graph.add_node(CodeNode {
            id: "./a.rs".to_string(),
            node_type: NodeType::File,
            name: "./a.rs".to_string(),
            content: "fn foo() {}".to_string(),
            start_line: 1,
            end_line: 1,
        });
        let func_a_idx = graph.add_node(CodeNode {
            id: "./a.rs:func:foo".to_string(),
            node_type: NodeType::Function,
            name: "foo".to_string(),
            content: "fn foo() {}".to_string(),
            start_line: 1,
            end_line: 1,
        });
        graph.add_edge(file_a_idx, func_a_idx, EdgeType::Contains);

        let file_b_idx = graph.add_node(CodeNode {
            id: "./b.rs".to_string(),
            node_type: NodeType::File,
            name: "./b.rs".to_string(),
            content: "fn bar() {}".to_string(),
            start_line: 1,
            end_line: 1,
        });
        let func_b_idx = graph.add_node(CodeNode {
            id: "./b.rs:func:bar".to_string(),
            node_type: NodeType::Function,
            name: "bar".to_string(),
            content: "fn bar() {}".to_string(),
            start_line: 1,
            end_line: 1,
        });
        graph.add_edge(file_b_idx, func_b_idx, EdgeType::Contains);

        graph.remove_file_nodes("./a.rs");

        assert!(graph.find_file_node("./a.rs").is_none());
        assert!(graph.find_file_node("./b.rs").is_some());
        assert!(graph
            .graph
            .node_weights()
            .all(|node| !node.id.starts_with("./a.rs")));
    }
}
