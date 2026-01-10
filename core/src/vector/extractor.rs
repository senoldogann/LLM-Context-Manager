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
            SupportedLanguage::Data => None, // Data files don't have sub-nodes via AST
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
        let end = node.end_byte().min(self.source_code.len());
        self.source_code.get(start..end).unwrap_or("").to_string()
    }

    // ========== PHASE 2: REFERENCE EXTRACTION (Graph Navigator) ==========

    /// Second pass: Extract function calls and link them to definitions.
    /// Call this AFTER `extract()` to populate the symbol table.
    pub fn extract_references(&self, tree: &Tree, graph: &mut CodeGraph) -> Result<usize> {
        let mut edges_created = 0;
        self.walk_for_calls(tree.root_node(), graph, &mut edges_created)?;
        Ok(edges_created)
    }

    fn walk_for_calls(
        &self,
        node: Node,
        graph: &mut CodeGraph,
        edges_created: &mut usize,
    ) -> Result<()> {
        let kind = node.kind();

        // Detect call expressions based on language
        let is_call = match &self.language {
            SupportedLanguage::Rust => kind == "call_expression",
            SupportedLanguage::Python => kind == "call",
            SupportedLanguage::TypeScript => kind == "call_expression",
            SupportedLanguage::Data => false,
        };

        if is_call {
            if let Some(callee_name) = self.extract_callee_name(&node) {
                // Find the calling function (parent context)
                if let Some(caller_idx) = self.find_enclosing_function(&node, graph) {
                    // Find the called function by name
                    if let Some(callee_idx) = self.find_function_by_name(graph, &callee_name) {
                        // Don't create self-edges
                        if caller_idx != callee_idx {
                            graph.add_edge(caller_idx, callee_idx, EdgeType::Calls);
                            *edges_created += 1;
                        }
                    }
                }
            }
        }

        // Recurse into children
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.walk_for_calls(child, graph, edges_created)?;
        }

        Ok(())
    }

    /// Extract the function name being called from a call expression.
    fn extract_callee_name(&self, call_node: &Node) -> Option<String> {
        // For Rust: call_expression has a "function" child
        // For Python: call has a "function" child
        // For TS: call_expression has a "function" child

        let func_child = call_node.child_by_field_name("function")?;
        let kind = func_child.kind();

        // Handle simple identifier calls like `foo()`
        if kind == "identifier" {
            return Some(self.get_node_text(&func_child));
        }

        // Handle method calls like `self.foo()` or `obj.method()`
        if kind == "field_expression" || kind == "attribute" || kind == "member_expression" {
            // Get the rightmost identifier (the method name)
            if let Some(field) = func_child
                .child_by_field_name("field")
                .or_else(|| func_child.child_by_field_name("attribute"))
                .or_else(|| func_child.child_by_field_name("property"))
            {
                return Some(self.get_node_text(&field));
            }
            // Fallback: get last child if it's an identifier
            let mut cursor = func_child.walk();
            let children: Vec<_> = func_child.children(&mut cursor).collect();
            for child in children.iter().rev() {
                if child.kind() == "identifier" || child.kind() == "property_identifier" {
                    return Some(self.get_node_text(child));
                }
            }
        }

        // Handle scoped calls like `module::function()`
        if kind == "scoped_identifier" {
            if let Some(name) = func_child.child_by_field_name("name") {
                return Some(self.get_node_text(&name));
            }
        }

        None
    }

    /// Find the enclosing function node for a given AST node.
    fn find_enclosing_function(&self, node: &Node, graph: &CodeGraph) -> Option<NodeIndex> {
        let line = node.start_position().row + 1; // 1-indexed

        // Find the function node that contains this line
        for idx in graph.graph.node_indices() {
            let code_node = &graph.graph[idx];
            if matches!(code_node.node_type, NodeType::Function | NodeType::Method)
                && code_node.start_line <= line
                && code_node.end_line >= line
            {
                return Some(idx);
            }
        }
        None
    }

    /// Find a function node by its name (fuzzy match).
    fn find_function_by_name(&self, graph: &CodeGraph, name: &str) -> Option<NodeIndex> {
        for idx in graph.graph.node_indices() {
            let code_node = &graph.graph[idx];
            if matches!(code_node.node_type, NodeType::Function | NodeType::Method)
                && code_node.name == name
            {
                return Some(idx);
            }
        }
        None
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
        let mut parser = CodeParser::new();
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
        let mut parser = CodeParser::new();
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
        let mut parser = CodeParser::new();
        let tree = parser.parse_tree(code, SupportedLanguage::Rust).unwrap();

        let mut graph = CodeGraph::new();
        let mut extractor = Extractor::new(code.to_string(), SupportedLanguage::Rust);
        extractor.extract(&tree, &mut graph, "point.rs").unwrap();

        // Should have: 1 File + 1 Struct + 1 impl (Class) + 1 Function = 4 nodes
        assert!(graph.graph.node_count() >= 4);
    }
}
