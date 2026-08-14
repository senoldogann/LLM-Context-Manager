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
    id_occurrences: HashMap<String, usize>,
}

impl Extractor {
    pub fn new(source_code: String, language: SupportedLanguage) -> Self {
        Self {
            source_code,
            language,
            node_map: HashMap::new(),
            id_occurrences: HashMap::new(),
        }
    }

    pub fn extract(
        &mut self,
        tree: &Tree,
        graph: &mut CodeGraph,
        file_id: &str,
    ) -> Result<NodeIndex> {
        // File node kök konteyner olarak oluşturulur.
        // Satır aralığı sembol node'larıyla tutarlı şekilde 1-indexed olmalı;
        // aksi halde son satır sorguları File node'u kapsam dışı bırakır.
        let end_line = self.source_code.lines().count().max(1);
        let file_node = CodeNode {
            id: file_id.to_string(),
            node_type: NodeType::File,
            name: file_id.to_string(),
            content: "".into(),
            start_line: 1,
            end_line,
        };
        let file_idx = graph.add_node(file_node);
        self.node_map.insert(file_id.to_string(), file_idx);

        // Walk the AST and extract elements
        self.walk_node(tree.root_node(), graph, file_idx, file_id, "")?;

        Ok(file_idx)
    }

    fn walk_node(
        &mut self,
        node: Node,
        graph: &mut CodeGraph,
        parent_idx: NodeIndex,
        file_id: &str,
        semantic_parent: &str,
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

            let semantic_path = if semantic_parent.is_empty() {
                name.clone()
            } else {
                format!("{}::{}", semantic_parent, name)
            };
            let identity = format!("{}:{}:{}", file_id, node.kind(), semantic_path);
            let occurrence = self.id_occurrences.entry(identity.clone()).or_insert(0);
            let id = format!(
                "{}:{}:symbol:{:016x}:{}",
                file_id,
                node.kind(),
                stable_hash(&identity),
                *occurrence
            );
            *occurrence += 1;

            let code_node = CodeNode {
                id: id.clone(),
                node_type,
                name,
                content: final_content.into(),
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
                self.walk_node(child, graph, current_idx, file_id, &semantic_path)?;
            }
        } else {
            // Not a semantic node, continue walking with same parent
            let mut cursor = node.walk();
            for child in node.children(&mut cursor) {
                self.walk_node(child, graph, parent_idx, file_id, semantic_parent)?;
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
            SupportedLanguage::TypeScript | SupportedLanguage::Tsx => {
                self.classify_typescript_node(node, kind)
            }
            SupportedLanguage::Go => self.classify_go_node(node, kind),
            SupportedLanguage::Java => self.classify_java_node(node, kind),
            SupportedLanguage::Kotlin => self.classify_kotlin_node(node, kind),
            SupportedLanguage::CSharp => self.classify_csharp_node(node, kind),
            SupportedLanguage::C => self.classify_c_node(node, kind),
            SupportedLanguage::Cpp => self.classify_cpp_node(node, kind),
            SupportedLanguage::Ruby => self.classify_ruby_node(node, kind),
            SupportedLanguage::Php => self.classify_php_node(node, kind),
            SupportedLanguage::Swift => self.classify_swift_node(node, kind),
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
                                .find(|c| c.kind() == "struct_type" || c.kind() == "interface_type")
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
            "secondary_constructor" => Some((NodeType::Function, "constructor".to_string())),
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

    fn classify_c_node(&self, node: &Node, kind: &str) -> Option<(NodeType, String)> {
        match kind {
            "function_definition" => Some((
                NodeType::Function,
                self.find_descendant_text(node, &["identifier"])
                    .unwrap_or_else(|| "anonymous".to_string()),
            )),
            "struct_specifier" | "union_specifier" | "enum_specifier" => Some((
                NodeType::Struct,
                self.find_descendant_text(node, &["type_identifier", "identifier"])
                    .unwrap_or_else(|| "anonymous".to_string()),
            )),
            "preproc_include" => Some((NodeType::Import, self.get_node_text(node))),
            "declaration" => self
                .find_descendant_text(node, &["identifier"])
                .map(|name| (NodeType::Variable, name)),
            _ => None,
        }
    }

    fn classify_cpp_node(&self, node: &Node, kind: &str) -> Option<(NodeType, String)> {
        match kind {
            "function_definition" => Some((
                NodeType::Function,
                self.find_descendant_text(
                    node,
                    &["field_identifier", "operator_name", "identifier"],
                )
                .unwrap_or_else(|| "anonymous".to_string()),
            )),
            "class_specifier" => Some((
                NodeType::Class,
                self.find_descendant_text(node, &["type_identifier", "identifier"])
                    .unwrap_or_else(|| "anonymous".to_string()),
            )),
            "struct_specifier" | "union_specifier" | "enum_specifier" => Some((
                NodeType::Struct,
                self.find_descendant_text(node, &["type_identifier", "identifier"])
                    .unwrap_or_else(|| "anonymous".to_string()),
            )),
            "namespace_definition" => Some((
                NodeType::Module,
                self.find_descendant_text(node, &["namespace_identifier", "identifier"])
                    .unwrap_or_else(|| "anonymous".to_string()),
            )),
            "preproc_include" => Some((NodeType::Import, self.get_node_text(node))),
            _ => None,
        }
    }

    fn classify_ruby_node(&self, node: &Node, kind: &str) -> Option<(NodeType, String)> {
        match kind {
            "method" | "singleton_method" => Some((
                NodeType::Method,
                self.find_descendant_text(node, &["identifier", "constant"])
                    .unwrap_or_else(|| "anonymous".to_string()),
            )),
            "class" => Some((
                NodeType::Class,
                self.find_descendant_text(node, &["constant", "identifier"])
                    .unwrap_or_else(|| "anonymous".to_string()),
            )),
            "module" => Some((
                NodeType::Module,
                self.find_descendant_text(node, &["constant", "identifier"])
                    .unwrap_or_else(|| "anonymous".to_string()),
            )),
            "assignment" => self
                .find_descendant_text(
                    node,
                    &[
                        "identifier",
                        "instance_variable",
                        "class_variable",
                        "global_variable",
                    ],
                )
                .map(|name| (NodeType::Variable, name)),
            "call" if self.get_node_text(node).starts_with("require") => {
                Some((NodeType::Import, self.get_node_text(node)))
            }
            _ => None,
        }
    }

    fn classify_php_node(&self, node: &Node, kind: &str) -> Option<(NodeType, String)> {
        match kind {
            "function_definition" => Some((
                NodeType::Function,
                self.find_descendant_text(node, &["name", "identifier"])
                    .unwrap_or_else(|| "anonymous".to_string()),
            )),
            "method_declaration" => Some((
                NodeType::Method,
                self.find_descendant_text(node, &["name", "identifier"])
                    .unwrap_or_else(|| "anonymous".to_string()),
            )),
            "class_declaration" => Some((
                NodeType::Class,
                self.find_descendant_text(node, &["name", "identifier"])
                    .unwrap_or_else(|| "anonymous".to_string()),
            )),
            "interface_declaration" | "trait_declaration" | "enum_declaration" => Some((
                NodeType::Struct,
                self.find_descendant_text(node, &["name", "identifier"])
                    .unwrap_or_else(|| "anonymous".to_string()),
            )),
            "namespace_definition" => Some((
                NodeType::Module,
                self.find_descendant_text(node, &["namespace_name", "name"])
                    .unwrap_or_else(|| "global".to_string()),
            )),
            "namespace_use_declaration" | "require_expression" | "include_expression" => {
                Some((NodeType::Import, self.get_node_text(node)))
            }
            _ => None,
        }
    }

    fn classify_swift_node(&self, node: &Node, kind: &str) -> Option<(NodeType, String)> {
        match kind {
            "function_declaration"
            | "initializer_declaration"
            | "init_declaration"
            | "deinit_declaration"
            | "protocol_function_declaration" => Some((
                NodeType::Function,
                self.swift_declaration_name(node)
                    .unwrap_or_else(|| "init".to_string()),
            )),
            "class_declaration" | "actor_declaration" => Some((
                NodeType::Class,
                self.swift_declaration_name(node)
                    .unwrap_or_else(|| "anonymous".to_string()),
            )),
            "struct_declaration"
            | "protocol_declaration"
            | "enum_declaration"
            | "extension_declaration" => Some((
                NodeType::Struct,
                self.swift_declaration_name(node)
                    .unwrap_or_else(|| "anonymous".to_string()),
            )),
            "import_declaration" => Some((NodeType::Import, self.get_node_text(node))),
            "property_declaration" | "protocol_property_declaration" => self
                .swift_declaration_name(node)
                .map(|name| (NodeType::Variable, name)),
            _ => None,
        }
    }

    /// Swift isim çözümü: grammar `name:` field'ını tanımlar; attribute'lar
    /// (örn. `@MainActor`) gerçek isimden önce geldiği için ilk eşleşen
    /// tanımlayıcıyı almak yanlış isim üretir. Önce field, yoksa kaynak
    /// sırasındaki son tanımlayıcı kullanılır.
    fn swift_declaration_name(&self, node: &Node) -> Option<String> {
        if let Some(name_node) = node.child_by_field_name("name") {
            let text = self.get_node_text(&name_node);
            if !text.trim().is_empty() {
                return Some(text);
            }
        }
        let mut found = None;
        let mut stack = vec![*node];
        while let Some(current) = stack.pop() {
            if current.id() != node.id()
                && matches!(
                    current.kind(),
                    "type_identifier" | "simple_identifier" | "identifier"
                )
            {
                found = Some(self.get_node_text(&current));
            }
            let mut cursor = current.walk();
            let mut children: Vec<Node> = current.children(&mut cursor).collect();
            children.reverse();
            stack.extend(children);
        }
        found
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

    fn find_descendant_text(&self, node: &Node, kinds: &[&str]) -> Option<String> {
        let mut stack = vec![*node];
        while let Some(current) = stack.pop() {
            if current.id() != node.id() && kinds.contains(&current.kind()) {
                return Some(self.get_node_text(&current));
            }
            let mut cursor = current.walk();
            let mut children: Vec<Node> = current.children(&mut cursor).collect();
            children.reverse();
            stack.extend(children);
        }
        None
    }

    /// Helper: Gets the source code text for a node.
    fn get_node_text(&self, node: &Node) -> String {
        let start = node.start_byte();
        let end = node.end_byte().min(self.source_code.len());
        self.source_code.get(start..end).unwrap_or("").to_string()
    }
}

fn stable_hash(value: &str) -> u64 {
    value
        .as_bytes()
        .iter()
        .fold(0xcbf29ce484222325_u64, |hash, byte| {
            (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
        })
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

    #[test]
    fn file_node_covers_last_line_without_trailing_newline() {
        // Trailing newline yokken File node'un end_line'ı son satırı
        // kapsayamıyordu; son satır sorguları boş File node döndürüyordu.
        let code = "fn foo() {}";
        let mut parser = CodeParser::new();
        let tree = parser.parse_tree(code, SupportedLanguage::Rust).unwrap();

        let mut graph = CodeGraph::new();
        let mut extractor = Extractor::new(code.to_string(), SupportedLanguage::Rust);
        let file_idx = extractor.extract(&tree, &mut graph, "single.rs").unwrap();

        let file_node = &graph.graph[file_idx];
        assert_eq!(file_node.start_line, 1);
        assert_eq!(file_node.end_line, 1);

        let found = graph
            .find_node_in_file("single.rs", 1)
            .expect("node on line 1");
        assert_ne!(found, file_idx, "son satır File node'a düşmemeli");
        assert_eq!(graph.graph[found].name, "foo");
    }

    #[test]
    fn extracts_functions_from_tsx() {
        let code = "export function App() {\n  return <div>Hello</div>;\n}\n";
        let mut parser = CodeParser::new();
        let tree = parser.parse_tree(code, SupportedLanguage::Tsx).unwrap();
        assert!(!tree.root_node().has_error());

        let mut graph = CodeGraph::new();
        let mut extractor = Extractor::new(code.to_string(), SupportedLanguage::Tsx);
        extractor.extract(&tree, &mut graph, "App.tsx").unwrap();

        assert!(
            graph.graph.node_weights().any(|node| node.name == "App"),
            "TSX dosyasından App fonksiyonu çıkarılamadı"
        );
    }

    #[test]
    fn swift_attributes_do_not_steal_declaration_names() {
        let code = r#"
@MainActor
final class SettingsStore {
    @MainActor func startRecording() {}
}

@objc class AudioPlayerService {
    @objc func play() {}
}

struct Config { let name: String }
enum Mode { case auto }
actor Cache { func get() {} }
"#;
        let mut parser = CodeParser::new();
        let tree = parser.parse_tree(code, SupportedLanguage::Swift).unwrap();
        assert!(!tree.root_node().has_error(), "Swift parse failed");

        let mut graph = CodeGraph::new();
        let mut extractor = Extractor::new(code.to_string(), SupportedLanguage::Swift);
        extractor
            .extract(&tree, &mut graph, "settings.swift")
            .unwrap();

        let names: Vec<String> = graph
            .graph
            .node_indices()
            .map(|idx| graph.graph[idx].name.clone())
            .collect();
        assert!(
            names.iter().any(|name| name == "SettingsStore"),
            "SettingsStore adı attribute tarafından çalındı: {:?}",
            names
        );
        assert!(
            names.iter().any(|name| name == "AudioPlayerService"),
            "AudioPlayerService adı attribute tarafından çalındı: {:?}",
            names
        );
        assert!(
            names.iter().any(|name| name == "startRecording"),
            "startRecording metodu attribute tarafından çalındı: {:?}",
            names
        );
        assert!(
            names.iter().any(|name| name == "play"),
            "play metodu attribute tarafından çalındı: {:?}",
            names
        );
    }

    #[test]
    fn swift_call_edges_are_created_from_method_bodies() {
        let code = r#"
final class Recorder {
    func startRecording() {}
}

final class HomeView {
    let recorder = Recorder()
    func startRecording() {
        recorder.startRecording()
    }
}
"#;
        let mut parser = CodeParser::new();
        let tree = parser.parse_tree(code, SupportedLanguage::Swift).unwrap();
        assert!(!tree.root_node().has_error(), "Swift parse failed");

        let mut graph = CodeGraph::new();
        let mut extractor = Extractor::new(code.to_string(), SupportedLanguage::Swift);
        extractor.extract(&tree, &mut graph, "home.swift").unwrap();
        graph.rebuild_reference_edges();

        let calls: Vec<String> = graph
            .graph
            .edge_indices()
            .filter_map(|edge_idx| {
                let (a, b) = graph.graph.edge_endpoints(edge_idx)?;
                if graph.graph[edge_idx] == EdgeType::Calls {
                    Some(format!(
                        "{} -> {}",
                        graph.graph[a].name, graph.graph[b].name
                    ))
                } else {
                    None
                }
            })
            .collect();
        assert!(
            calls.iter().any(|edge| edge.ends_with("-> startRecording")),
            "HomeView.startRecording -> Recorder.startRecording çağrı kenarı yok: {:?}",
            calls
        );
    }

    #[test]
    fn semantic_ids_survive_unrelated_line_insertions() {
        fn function_id(source: &str) -> String {
            let mut parser = CodeParser::new();
            let tree = parser.parse_tree(source, SupportedLanguage::Rust).unwrap();
            let mut graph = CodeGraph::new();
            let mut extractor = Extractor::new(source.to_string(), SupportedLanguage::Rust);
            extractor
                .extract(&tree, &mut graph, "./src/lib.rs")
                .unwrap();
            graph
                .graph
                .node_weights()
                .find(|node| node.name == "stable")
                .expect("stable function")
                .id
                .clone()
        }

        let before = function_id("fn stable() {}\n");
        let after = function_id("\n\n// unrelated\nfn stable() {}\n");
        assert_eq!(before, after);
        assert!(before.contains(":symbol:"));
    }

    #[test]
    fn extracts_symbols_from_extended_languages() {
        let cases = [
            (
                SupportedLanguage::C,
                "int answer(void) { return 42; }",
                "answer",
            ),
            (
                SupportedLanguage::Cpp,
                "class Greeter { public: void hello() {} };",
                "Greeter",
            ),
            (
                SupportedLanguage::Ruby,
                "class Greeter\n def hello; end\nend",
                "Greeter",
            ),
            (
                SupportedLanguage::Php,
                "<?php class Greeter { public function hello() {} }",
                "Greeter",
            ),
            (
                SupportedLanguage::Swift,
                "struct Greeter { func hello() {} }",
                "Greeter",
            ),
        ];

        for (language, source, expected_name) in cases {
            let mut parser = CodeParser::new();
            let tree = parser.parse_tree(source, language).unwrap();
            let mut graph = CodeGraph::new();
            let mut extractor = Extractor::new(source.to_string(), language);
            extractor.extract(&tree, &mut graph, "./sample").unwrap();
            assert!(
                graph
                    .graph
                    .node_weights()
                    .any(|node| node.name == expected_name),
                "{language:?} did not extract {expected_name}"
            );
        }
    }
}
