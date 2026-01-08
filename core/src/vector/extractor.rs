//! Extractor: Converts Tree-sitter AST into CodeGraph nodes.
//!
//! This module bridges the Parser output to the Graph representation,
//! extracting semantic code elements (functions, classes, imports, etc.)
//! and creating appropriate graph nodes with their relationships.

use anyhow::Result;
use petgraph::graph::NodeIndex;
use std::collections::HashMap;
use tree_sitter::{Node, Tree};

use crate::graph::{CodeGraph, CodeNode, EdgeType, NodeType};
use crate::parser::SupportedLanguage;

/// Extractor extracts semantic code elements from an AST and populates a CodeGraph.
pub struct Extractor {
    source_code: String,
    language: SupportedLanguage,
    node_map: HashMap<String, NodeIndex>,
}

impl Extractor {
    pub fn new(source_code: String, language: SupportedLanguage) -> Self {
        Self {
            source_code,
            language,
            node_map: HashMap::new(),
        }
    }

    /// Extracts code elements from the given AST tree and populates the graph.
    pub fn extract(
        &mut self,
        tree: &Tree,
        graph: &mut CodeGraph,
        file_id: &str,
    ) -> Result<NodeIndex> {
        // Create File node as root
        let file_node = CodeNode {
            id: file_id.to_string(),
            node_type: NodeType::File,
            name: file_id.to_string(),
            content: String::new(),
            start_line: 0,
            end_line: tree.root_node().end_position().row,
        };
        let file_idx = graph.add_node(file_node);
        self.node_map.insert(file_id.to_string(), file_idx);

        // Walk the AST and extract elements
        self.walk_node(tree.root_node(), graph, file_idx)?;

        Ok(file_idx)
    }

    fn walk_node(
        &mut self,
        node: Node,
        graph: &mut CodeGraph,
        parent_idx: NodeIndex,
    ) -> Result<()> {
        let kind = node.kind();

        // Determine if this node is semantically significant
        if let Some((node_type, name)) = self.classify_node(&node) {
            let code_node = CodeNode {
                id: format!(
                    "{}:{}:{}",
                    node.kind(),
                    node.start_position().row,
                    node.start_position().column
                ),
                node_type,
                name,
                content: self.get_node_text(&node),
                start_line: node.start_position().row + 1, // 1-indexed
                end_line: node.end_position().row + 1,
            };

            let current_idx = graph.add_node(code_node);
            self.node_map.insert(
                format!("{}:{}", kind, node.start_position().row),
                current_idx,
            );

            // Create CONTAINS edge from parent -> this node
            graph.add_edge(parent_idx, current_idx, EdgeType::Contains);

            // Walk children with this node as parent
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                self.walk_node(child, graph, current_idx)?;
            }
        } else {
            // Not a semantic node, continue walking with same parent
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                self.walk_node(child, graph, parent_idx)?;
            }
        }

        Ok(())
    }

    /// Classifies an AST node into a semantic NodeType, returning None if not significant.
    fn classify_node(&self, node: &Node) -> Option<(NodeType, String)> {
        let kind = node.kind();

        match &self.language {
            SupportedLanguage::Rust => self.classify_rust_node(node, kind),
            SupportedLanguage::Python => self.classify_python_node(node, kind),
            SupportedLanguage::TypeScript => self.classify_typescript_node(node, kind),
        }
    }

    fn classify_rust_node(&self, node: &Node, kind: &str) -> Option<(NodeType, String)> {
        match kind {
            "function_item" => {
                let name = self.find_child_text(node, "identifier")?;
                Some((NodeType::Function, name))
            }
            "struct_item" => {
                let name = self.find_child_text(node, "type_identifier")?;
                Some((NodeType::Struct, name))
            }
            "impl_item" => {
                // For impl blocks, get the type being implemented
                let name = self
                    .find_child_text(node, "type_identifier")
                    .unwrap_or_else(|| "anonymous_impl".to_string());
                Some((NodeType::Class, name))
            }
            "mod_item" => {
                let name = self.find_child_text(node, "identifier")?;
                Some((NodeType::Module, name))
            }
            "use_declaration" => {
                let content = self.get_node_text(node);
                Some((NodeType::Import, content))
            }
            "let_declaration" | "const_item" | "static_item" => {
                let name = self
                    .find_child_text(node, "identifier")
                    .unwrap_or_else(|| "anonymous_var".to_string());
                Some((NodeType::Variable, name))
            }
            _ => None,
        }
    }

    fn classify_python_node(&self, node: &Node, kind: &str) -> Option<(NodeType, String)> {
        match kind {
            "function_definition" => {
                let name = self.find_child_text(node, "identifier")?;
                Some((NodeType::Function, name))
            }
            "class_definition" => {
                let name = self.find_child_text(node, "identifier")?;
                Some((NodeType::Class, name))
            }
            "import_statement" | "import_from_statement" => {
                let content = self.get_node_text(node);
                Some((NodeType::Import, content))
            }
            "assignment" => {
                // Get the left side of assignment
                if let Some(left) = node.child_by_field_name("left") {
                    let name = self.get_node_text(&left);
                    return Some((NodeType::Variable, name));
                }
                None
            }
            _ => None,
        }
    }

    fn classify_typescript_node(&self, node: &Node, kind: &str) -> Option<(NodeType, String)> {
        match kind {
            "function_declaration" | "arrow_function" | "method_definition" => {
                let name = self
                    .find_child_text(node, "identifier")
                    .or_else(|| self.find_child_text(node, "property_identifier"))
                    .unwrap_or_else(|| "anonymous".to_string());

                if kind == "method_definition" {
                    Some((NodeType::Method, name))
                } else {
                    Some((NodeType::Function, name))
                }
            }
            "class_declaration" => {
                let name = self.find_child_text(node, "type_identifier")?;
                Some((NodeType::Class, name))
            }
            "import_statement" => {
                let content = self.get_node_text(node);
                Some((NodeType::Import, content))
            }
            "variable_declarator" => {
                let name = self.find_child_text(node, "identifier")?;
                Some((NodeType::Variable, name))
            }
            "interface_declaration" | "type_alias_declaration" => {
                let name = self.find_child_text(node, "type_identifier")?;
                Some((NodeType::Struct, name)) // Using Struct for type definitions
            }
            _ => None,
        }
    }

    /// Helper: Finds a child node by kind and returns its text.
    fn find_child_text(&self, node: &Node, child_kind: &str) -> Option<String> {
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            if child.kind() == child_kind {
                return Some(self.get_node_text(&child));
            }
        }
        None
    }

    /// Helper: Gets the source code text for a node.
    fn get_node_text(&self, node: &Node) -> String {
        let start = node.start_byte();
        let end = node.end_byte();
        self.source_code[start..end].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::CodeParser;

    #[test]
    fn test_extract_rust_function() {
        let code = r#"
fn main() {
    println!("Hello");
}

fn helper(x: i32) -> i32 {
    x + 1
}
"#;
        let mut parser = CodeParser::new().unwrap();
        let tree = parser.parse_tree(code, SupportedLanguage::Rust).unwrap();

        let mut graph = CodeGraph::new();
        let mut extractor = Extractor::new(code.to_string(), SupportedLanguage::Rust);
        let file_idx = extractor.extract(&tree, &mut graph, "test.rs").unwrap();

        // Should have: 1 File + 2 Functions = 3 nodes
        assert_eq!(graph.graph.node_count(), 3);

        // Verify file node exists
        let file_node = &graph.graph[file_idx];
        assert_eq!(file_node.node_type, NodeType::File);
    }

    #[test]
    fn test_extract_python_class() {
        let code = r#"
class MyClass:
    def __init__(self):
        self.value = 0
    
    def get_value(self):
        return self.value
"#;
        let mut parser = CodeParser::new().unwrap();
        let tree = parser.parse_tree(code, SupportedLanguage::Python).unwrap();

        let mut graph = CodeGraph::new();
        let mut extractor = Extractor::new(code.to_string(), SupportedLanguage::Python);
        extractor.extract(&tree, &mut graph, "test.py").unwrap();

        // Should have: 1 File + 1 Class + 2 Functions + 1 Variable (self.value) = ~5 nodes
        assert!(graph.graph.node_count() >= 4);
    }

    #[test]
    fn test_extract_rust_struct_and_impl() {
        let code = r#"
struct Point {
    x: i32,
    y: i32,
}

impl Point {
    fn new(x: i32, y: i32) -> Self {
        Point { x, y }
    }
}
"#;
        let mut parser = CodeParser::new().unwrap();
        let tree = parser.parse_tree(code, SupportedLanguage::Rust).unwrap();

        let mut graph = CodeGraph::new();
        let mut extractor = Extractor::new(code.to_string(), SupportedLanguage::Rust);
        extractor.extract(&tree, &mut graph, "point.rs").unwrap();

        // Should have: 1 File + 1 Struct + 1 impl (Class) + 1 Function = 4 nodes
        assert!(graph.graph.node_count() >= 4);
    }
}
