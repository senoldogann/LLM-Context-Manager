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
