use petgraph::graph::{DiGraph, NodeIndex};
use serde::{Deserialize, Serialize};

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

pub struct CodeGraph {
    pub graph: DiGraph<CodeNode, EdgeType>,
}

impl Default for CodeGraph {
    fn default() -> Self {
        Self {
            graph: DiGraph::new(),
        }
    }
}

impl CodeGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, node: CodeNode) -> NodeIndex {
        self.graph.add_node(node)
    }

    pub fn add_edge(&mut self, source: NodeIndex, target: NodeIndex, weight: EdgeType) {
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
                let mut neighbors = self
                    .graph
                    .neighbors_directed(idx, petgraph::Direction::Outgoing);
                while let Some(neighbor_idx) = neighbors.next() {
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
    pub fn find_node_by_id(&self, id: &str) -> Option<&CodeNode> {
        self.graph.node_weights().find(|node| node.id == id)
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
