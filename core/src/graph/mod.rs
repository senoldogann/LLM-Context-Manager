use crate::storage::GraphStorage;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum NodeType {
    File,
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
                eprintln!("Failed to save node to storage: {}", e);
            }
        }
        self.graph.add_node(node)
    }

    pub fn add_edge(&mut self, source: NodeIndex, target: NodeIndex, weight: EdgeType) {
        if let Some(storage) = &self.storage {
            let source_node = &self.graph[source];
            let target_node = &self.graph[target];
            if let Err(e) = storage.save_edge(&source_node.id, &target_node.id, weight.clone()) {
                eprintln!("Failed to save edge to storage: {}", e);
            }
        }
        self.graph.add_edge(source, target, weight);
    }

    /// Finds the node corresponding to a specific file path.
    pub fn find_file_node(&self, file_path: &str) -> Option<NodeIndex> {
        self.graph.node_indices().find(|&idx| {
            let node = &self.graph[idx];
            node.node_type == NodeType::File && node.name == file_path
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
        let mut to_remove = Vec::new();
        let mut stack = vec![file_node_idx];

        // We include the file node itself in the removal
        to_remove.push(file_node_idx);

        while let Some(idx) = stack.pop() {
            let neighbors = self
                .graph
                .neighbors_directed(idx, petgraph::Direction::Outgoing);
            for neighbor_idx in neighbors {
                let edge = self.graph.find_edge(idx, neighbor_idx);
                if let Some(edge_idx) = edge {
                    if let Some(weight) = self.graph.edge_weight(edge_idx) {
                        if matches!(weight, EdgeType::Contains) {
                            to_remove.push(neighbor_idx);
                            stack.push(neighbor_idx);
                        }
                    }
                }
            }
        }

        // 3. Remove nodes in reverse order (children first usually better but petgraph handles it)
        // Note: Removing a node invalidates indices if we are not careful.
        // Stack removal in petgraph (retain_nodes) is safer or sorting indices
        // But simply iterating and calling removed_node might panic if indices shift?
        // Petgraph's remove_node shifts indices for `Graph` but `StableGraph` keeps them.
        // We are using `DiGraph` which is `Graph`. Removing node N swaps last node into N.
        // So all stored indices >= N might be invalid or swapped.

        // Strategy: Filter function `retain_nodes`.
        // We build a HashSet of indices to remove?
        // Or simpler: Sort indices descending and remove?

        to_remove.sort_by(|a, b| b.cmp(a)); // Descending sort
        to_remove.dedup(); // Ensure unique

        for idx in to_remove {
            self.graph.remove_node(idx);
        }
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
}
