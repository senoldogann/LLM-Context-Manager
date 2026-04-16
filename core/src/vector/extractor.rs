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

    pub fn extract(
        &mut self,
        tree: &Tree,
        graph: &mut CodeGraph,
        file_id: &str,
    ) -> Result<NodeIndex> {
        // Create File node as root
        // Note: For Phase 3 Optimization, we want the File node to be a "container"
        // but not necessarily the primary embedding target unless specifically asked.
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
        self.walk_node(tree.root_node(), graph, file_idx, file_id)?;

        Ok(file_idx)
    }

    fn walk_node(
        &mut self,
        node: Node,
        graph: &mut CodeGraph,
        parent_idx: NodeIndex,
        file_id: &str,
    ) -> Result<()> {
        // Determine if this node is semantically significant
        if let Some((node_type, name)) = self.classify_node(&node) {
            // PHASE 3: Capture Docstring (Preceding comments)
            let docstring = self.find_docstring(&node);

            // Enriched Content: "Docstring\nCode"
            // We store the RAW code in `content` for display accuracy.
            // But we might prepend the docstring if it's outside the node range?
            // Usually docstrings are *inside* or *immediately before*.
            // Tree-sitter node range usually includes internal docstrings (Python).
            // But Rust doc comments `///` are separate nodes *before* the function.

            // If we found an external docstring, we should update content to include it,
            // OR store it separately. `CodeNode` doesn't have a `metadata` field yet.
            // For now, let's prepend it to `content` if it exists and isn't already inside.

            let raw_content = self.get_node_text(&node);
            let final_content = if let Some(doc) = docstring {
                format!("{}\n{}", doc, raw_content)
            } else {
                raw_content
            };

            // ID Generation: Prefix with file_id for global uniqueness and GC
            let id = format!(
                "{}:{}:{}:{}",
                file_id,
                node.kind(),
                node.start_position().row,
                node.start_position().column
            );

            let code_node = CodeNode {
                id: id.clone(),
                node_type,
                name,
                content: final_content,
                start_line: node.start_position().row + 1, // 1-indexed
                end_line: node.end_position().row + 1,
            };

            let current_idx = graph.add_node(code_node);
            self.node_map.insert(id, current_idx);

            // Create CONTAINS edge from parent -> this node
            graph.add_edge(parent_idx, current_idx, EdgeType::Contains);

            // Walk children with this node as parent
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                self.walk_node(child, graph, current_idx, file_id)?;
            }
        } else {
            // Not a semantic node, continue walking with same parent
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                self.walk_node(child, graph, parent_idx, file_id)?;
            }
        }

        Ok(())
    }

    /// Heuristic to find docstrings/comments immediately preceding a node.
    fn find_docstring(&self, node: &Node) -> Option<String> {
        // Look at previous sibling. If it's a comment, that's our docstring.
        // We might need to look back multiple siblings if there are multiple comment lines.

        let mut doc_lines = Vec::new();
        let mut current_node = *node;

        // Safety limit: look back at most this many siblings for docstring/comment nodes.
        const MAX_DOC_LOOKBACK: usize = 5;
        for _ in 0..MAX_DOC_LOOKBACK {
            if let Some(prev) = current_node.prev_sibling() {
                let kind = prev.kind();
                if kind == "line_comment" || kind == "block_comment" || kind == "comment" {
                    doc_lines.push(self.get_node_text(&prev));
                    current_node = prev;
                } else {
                    // Check if whitespace? Tree-sitter usually ignores pure whitespace nodes
                    // unless configured otherwise, but sometimes they appear.
                    // Assuming standard behavior where comments are siblings.
                    break;
                }
            } else {
                break;
            }
        }

        if doc_lines.is_empty() {
            None
        } else {
            // We traversed backwards, so reverse lines
            doc_lines.reverse();
            Some(doc_lines.join("\n"))
        }
    }

    /// Classifies an AST node into a semantic NodeType, returning None if not significant.
    fn classify_node(&self, node: &Node) -> Option<(NodeType, String)> {
        let kind = node.kind();

        match &self.language {
            SupportedLanguage::Rust => self.classify_rust_node(node, kind),
            SupportedLanguage::Python => self.classify_python_node(node, kind),
            SupportedLanguage::TypeScript => self.classify_typescript_node(node, kind),
            SupportedLanguage::Go => self.classify_go_node(node, kind),
            SupportedLanguage::Java => self.classify_java_node(node, kind),
            SupportedLanguage::Kotlin => self.classify_kotlin_node(node, kind),
            SupportedLanguage::CSharp => self.classify_csharp_node(node, kind),
            SupportedLanguage::Data => None,
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
                let mut name_opt = self
                    .find_child_text(node, "identifier")
                    .or_else(|| self.find_child_text(node, "property_identifier"));

                // If arrow function is defined via variable `const myFunc = () => {}`
                if name_opt.is_none() && kind == "arrow_function" {
                    if let Some(parent) = node.parent() {
                        if parent.kind() == "variable_declarator" {
                            name_opt = self.find_child_text(&parent, "identifier");
                        } else if parent.kind() == "pair" {
                            // Object literal: `myFunc: () => {}`
                            name_opt = self.find_child_text(&parent, "property_identifier");
                        }
                    }
                }

                let name = name_opt.unwrap_or_else(|| "anonymous".to_string());

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

    fn classify_go_node(&self, node: &Node, kind: &str) -> Option<(NodeType, String)> {
        match kind {
            "function_declaration" => {
                let name = self.find_child_text(node, "identifier")?;
                Some((NodeType::Function, name))
            }
            "method_declaration" => {
                // Go method: func (recv T) Name(...) — isim field_identifier olarak gelir
                let name = self
                    .find_child_text(node, "field_identifier")
                    .or_else(|| self.find_child_text(node, "identifier"))
                    .unwrap_or_else(|| "anonymous".to_string());
                Some((NodeType::Method, name))
            }
            "type_declaration" => {
                // type Foo struct{} / type Bar interface{}
                // İçindeki type_spec'ten ismi al
                let mut cursor = node.walk();
                for child in node.children(&mut cursor) {
                    if child.kind() == "type_spec" {
                        if let Some(name) = self.find_child_text(&child, "type_identifier") {
                            // Struct mu interface mi?
                            let body_kind = child
                                .children(&mut child.walk())
                                .find(|c| {
                                    c.kind() == "struct_type" || c.kind() == "interface_type"
                                })
                                .map(|c| c.kind())
                                .unwrap_or("");
                            let node_type = if body_kind == "interface_type" {
                                NodeType::Class
                            } else {
                                NodeType::Struct
                            };
                            return Some((node_type, name));
                        }
                    }
                }
                None
            }
            "import_declaration" => {
                let content = self.get_node_text(node);
                Some((NodeType::Import, content))
            }
            "var_declaration" | "const_declaration" | "short_var_declaration" => {
                // Basit değişken adı — ilk identifier'ı al
                let name = self
                    .find_child_text(node, "identifier")
                    .unwrap_or_else(|| "anonymous_var".to_string());
                Some((NodeType::Variable, name))
            }
            _ => None,
        }
    }

    fn classify_java_node(&self, node: &Node, kind: &str) -> Option<(NodeType, String)> {
        match kind {
            "method_declaration" => {
                let name = self.find_child_text(node, "identifier")?;
                Some((NodeType::Method, name))
            }
            "constructor_declaration" => {
                let name = self.find_child_text(node, "identifier")?;
                Some((NodeType::Function, name))
            }
            "class_declaration" => {
                let name = self.find_child_text(node, "identifier")?;
                Some((NodeType::Class, name))
            }
            "interface_declaration" => {
                let name = self.find_child_text(node, "identifier")?;
                Some((NodeType::Struct, name))
            }
            "enum_declaration" => {
                let name = self.find_child_text(node, "identifier")?;
                Some((NodeType::Struct, name))
            }
            "import_declaration" => {
                let content = self.get_node_text(node);
                Some((NodeType::Import, content))
            }
            "field_declaration" | "local_variable_declaration" => {
                let name = self
                    .find_child_text(node, "variable_declarator")
                    .or_else(|| self.find_child_text(node, "identifier"))
                    .unwrap_or_else(|| "field".to_string());
                Some((NodeType::Variable, name))
            }
            _ => None,
        }
    }

    fn classify_csharp_node(&self, node: &Node, kind: &str) -> Option<(NodeType, String)> {
        match kind {
            "method_declaration" => {
                let name = self.find_child_text(node, "identifier")?;
                Some((NodeType::Method, name))
            }
            "constructor_declaration" => {
                let name = self.find_child_text(node, "identifier")?;
                Some((NodeType::Function, name))
            }
            "class_declaration" => {
                let name = self.find_child_text(node, "identifier")?;
                Some((NodeType::Class, name))
            }
            "interface_declaration" => {
                let name = self.find_child_text(node, "identifier")?;
                Some((NodeType::Struct, name))
            }
            "record_declaration" | "struct_declaration" => {
                let name = self.find_child_text(node, "identifier")?;
                Some((NodeType::Struct, name))
            }
            "enum_declaration" => {
                let name = self.find_child_text(node, "identifier")?;
                Some((NodeType::Struct, name))
            }
            "using_directive" => {
                let content = self.get_node_text(node);
                Some((NodeType::Import, content))
            }
            "field_declaration" | "property_declaration" | "event_field_declaration" => {
                let name = self
                    .find_child_text(node, "variable_declarator")
                    .or_else(|| self.find_child_text(node, "identifier"))
                    .unwrap_or_else(|| "field".to_string());
                Some((NodeType::Variable, name))
            }
            _ => None,
        }
    }

    fn classify_kotlin_node(&self, node: &Node, kind: &str) -> Option<(NodeType, String)> {
        match kind {
            "function_declaration" => {
                let name = self.find_child_text(node, "simple_identifier")?;
                Some((NodeType::Function, name))
            }
            "secondary_constructor" => {
                Some((NodeType::Function, "constructor".to_string()))
            }
            "class_declaration" | "object_declaration" => {
                let name = self
                    .find_child_text(node, "type_identifier")
                    .or_else(|| self.find_child_text(node, "simple_identifier"))
                    .unwrap_or_else(|| "anonymous".to_string());
                // Both class_declaration and object_declaration map to Class:
                // Kotlin objects are effectively singleton classes.
                Some((NodeType::Class, name))
            }
            "import_header" => {
                let content = self.get_node_text(node);
                Some((NodeType::Import, content))
            }
            "property_declaration" => {
                let name = self
                    .find_child_text(node, "simple_identifier")
                    .unwrap_or_else(|| "property".to_string());
                Some((NodeType::Variable, name))
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
    pub fn extract_references(
        &self,
        tree: &Tree,
        graph: &mut CodeGraph,
        file_id: &str,
    ) -> Result<usize> {
        let mut edges_created = 0;
        self.walk_for_calls(tree.root_node(), graph, &mut edges_created, file_id)?;
        Ok(edges_created)
    }

    fn walk_for_calls(
        &self,
        node: Node,
        graph: &mut CodeGraph,
        edges_created: &mut usize,
        file_id: &str,
    ) -> Result<()> {
        let kind = node.kind();

        // Detect call expressions based on language
        let is_call = match &self.language {
            SupportedLanguage::Rust => kind == "call_expression",
            SupportedLanguage::Python => kind == "call",
            SupportedLanguage::TypeScript => kind == "call_expression",
            SupportedLanguage::Go => kind == "call_expression",
            SupportedLanguage::Java => kind == "method_invocation",
            SupportedLanguage::Kotlin => kind == "call_expression",
            SupportedLanguage::CSharp => kind == "invocation_expression",
            SupportedLanguage::Data => false,
        };

        if is_call {
            if let Some(callee_name) = self.extract_callee_name(&node) {
                // Find the calling function (parent context) within the same file
                if let Some(caller_idx) = self.find_enclosing_function(&node, graph, file_id) {
                    // Find the called function by name (can be anywhere in graph)
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
            self.walk_for_calls(child, graph, edges_created, file_id)?;
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

    /// Find the enclosing function node for a given AST node within its file.
    fn find_enclosing_function(
        &self,
        node: &Node,
        graph: &CodeGraph,
        file_id: &str,
    ) -> Option<NodeIndex> {
        let line = node.start_position().row + 1; // 1-indexed

        // Pre-compute prefix for fast matching (e.g., "./src/main.rs:")
        let prefix = format!("{}:", file_id);

        // Find the function node that contains this line
        for idx in graph.graph.node_indices() {
            let code_node = &graph.graph[idx];

            // Critical fix: ensure we only match functions in the EXACT same file
            if !code_node.id.starts_with(&prefix) && code_node.id != file_id {
                continue;
            }

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
