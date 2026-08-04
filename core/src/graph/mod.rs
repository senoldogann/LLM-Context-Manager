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
    pub content: Arc<str>,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    pub id_index: std::collections::HashMap<String, NodeIndex>,
    pub name_index: std::collections::HashMap<String, Vec<NodeIndex>>,
    pub file_nodes_index: std::collections::HashMap<String, Vec<NodeIndex>>,
}

impl Default for CodeGraph {
    fn default() -> Self {
        Self {
            graph: DiGraph::new(),
            id_index: std::collections::HashMap::new(),
            name_index: std::collections::HashMap::new(),
            file_nodes_index: std::collections::HashMap::new(),
        }
    }
}

impl CodeGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_node(&mut self, node: CodeNode) -> NodeIndex {
        let id = node.id.clone();
        let name = node.name.clone();
        let file_id = graph_node_file_path(&node.id).to_string();
        let idx = self.graph.add_node(node);
        self.id_index.insert(id, idx);
        self.name_index.entry(name).or_default().push(idx);
        self.file_nodes_index.entry(file_id).or_default().push(idx);
        idx
    }

    pub fn add_edge(&mut self, source: NodeIndex, target: NodeIndex, weight: EdgeType) {
        if self
            .graph
            .edges_connecting(source, target)
            .any(|edge| edge.weight() == &weight)
        {
            return;
        }
        self.graph.add_edge(source, target, weight);
    }

    /// Rebuilds cross-symbol references from the current semantic node contents.
    ///
    /// Definitions are extracted per file, but callers can be indexed before their
    /// targets. Rebuilding after all changed definitions are present makes reference
    /// edges deterministic and repairs incoming edges after incremental updates.
    pub fn rebuild_reference_edges(&mut self) -> usize {
        self.graph.retain_edges(|graph, edge_idx| {
            !matches!(graph[edge_idx], EdgeType::Calls | EdgeType::Imports)
        });

        let mut symbols: std::collections::HashMap<String, Vec<NodeIndex>> =
            std::collections::HashMap::new();
        for idx in self.graph.node_indices() {
            let node = &self.graph[idx];
            if matches!(
                node.node_type,
                NodeType::Function
                    | NodeType::Method
                    | NodeType::Class
                    | NodeType::Struct
                    | NodeType::Module
            ) && is_referenceable_symbol(&node.name)
            {
                symbols.entry(node.name.clone()).or_default().push(idx);
            }
        }

        let source_indices: Vec<NodeIndex> = self
            .graph
            .node_indices()
            .filter(|idx| {
                matches!(
                    self.graph[*idx].node_type,
                    NodeType::Function | NodeType::Method | NodeType::Variable | NodeType::Import
                )
            })
            .collect();

        let mut references: HashSet<(NodeIndex, NodeIndex, bool)> = HashSet::new();
        for source_idx in source_indices {
            let source = &self.graph[source_idx];
            let source_file = graph_node_file_path(&source.id);
            let tokens = identifier_tokens(&source.content);
            for (name, call_like) in tokens {
                let Some(targets) = symbols.get(name) else {
                    continue;
                };
                let same_file_targets: Vec<_> = targets
                    .iter()
                    .copied()
                    .filter(|target_idx| {
                        graph_node_file_path(&self.graph[*target_idx].id) == source_file
                    })
                    .collect();
                let resolved_targets: Vec<_> = if same_file_targets.is_empty() {
                    if targets.len() == 1 {
                        targets.clone()
                    } else {
                        continue;
                    }
                } else {
                    same_file_targets
                };

                for target_idx in resolved_targets {
                    if source_idx == target_idx {
                        continue;
                    }
                    let target = &self.graph[target_idx];
                    let edge_type = if call_like {
                        EdgeType::Calls
                    } else if matches!(
                        target.node_type,
                        NodeType::Class | NodeType::Struct | NodeType::Module
                    ) {
                        EdgeType::Imports
                    } else {
                        continue;
                    };
                    references.insert((
                        source_idx,
                        target_idx,
                        matches!(edge_type, EdgeType::Calls),
                    ));
                }
            }
        }

        let count = references.len();
        for (source, target, is_call) in references {
            self.add_edge(
                source,
                target,
                if is_call {
                    EdgeType::Calls
                } else {
                    EdgeType::Imports
                },
            );
        }
        count
    }

    /// Finds the node corresponding to a specific file path.
    pub fn find_file_node(&self, file_path: &str) -> Option<NodeIndex> {
        // O(n) tarama yerine dosya bazlı index kullanılır; bu fonksiyon
        // cursor/impact/diff gibi sıcak path'lerde çağrılıyor.
        self.file_nodes_index
            .get(file_path)?
            .iter()
            .copied()
            .find(|&idx| {
                matches!(
                    self.graph[idx].node_type,
                    NodeType::File | NodeType::DataFile
                )
            })
    }

    /// Finds the deepest node within a file hierarchy that covers the given line.
    pub fn find_node_in_file(&self, file_path: &str, line: usize) -> Option<NodeIndex> {
        let file_node_idx = self.find_file_node(file_path)?;

        // En dar aralıklı sembol node tercih edilir; File/DataFile node yalnızca
        // hiçbir sembol satırı kapsamıyorsa fallback olarak döner.
        let mut best_match: Option<NodeIndex> = None;
        let mut min_len = usize::MAX;

        // BFS/DFS to visit all children of the file
        let mut stack = vec![file_node_idx];

        while let Some(idx) = stack.pop() {
            let node = &self.graph[idx];

            // Check if node covers the line
            if node.start_line <= line && node.end_line >= line {
                if !matches!(node.node_type, NodeType::File | NodeType::DataFile) {
                    let len = node.end_line - node.start_line;
                    if len < min_len {
                        min_len = len;
                        best_match = Some(idx);
                    }
                }

                // Çocuklara inmek için yalnızca Contains kenarları izlenir.
                // find_edge kullanılmaz: aynı çift üzerinde Calls gibi ikinci bir
                // kenar varsa belirsiz olanı döner ve Contains atlanır.
                for edge in self
                    .graph
                    .edges_directed(idx, petgraph::Direction::Outgoing)
                {
                    if matches!(edge.weight(), EdgeType::Contains) {
                        stack.push(edge.target());
                    }
                }
            }
        }

        Some(best_match.unwrap_or(file_node_idx))
    }

    /// Returns enclosing semantic scopes from the narrowest to the widest.
    pub fn find_enclosing_scopes(&self, file_path: &str, line: usize) -> Vec<NodeIndex> {
        let Some(file_node_idx) = self.find_file_node(file_path) else {
            return Vec::new();
        };
        let mut matches = Vec::new();
        let mut stack = vec![file_node_idx];

        while let Some(idx) = stack.pop() {
            let node = &self.graph[idx];
            if node.start_line <= line && node.end_line >= line {
                if matches!(
                    node.node_type,
                    NodeType::Function
                        | NodeType::Method
                        | NodeType::Class
                        | NodeType::Struct
                        | NodeType::Module
                ) {
                    matches.push(idx);
                }
                for edge in self
                    .graph
                    .edges_directed(idx, petgraph::Direction::Outgoing)
                {
                    if matches!(edge.weight(), EdgeType::Contains) {
                        stack.push(edge.target());
                    }
                }
            }
        }

        matches.sort_by_key(|idx| {
            let node = &self.graph[*idx];
            node.end_line.saturating_sub(node.start_line)
        });
        matches
    }
    pub fn find_node_by_id(&self, id: &str) -> Option<CodeNode> {
        // O(1) lookup via id_index — previously was O(n) linear scan
        self.id_index.get(id).map(|&idx| self.graph[idx].clone())
    }

    /// Kesin ID eşleşmesi bulunamazsa aynı dosya+tür+yakın satır ile düzeltilmiş arama yapar.
    /// Kod değişiklikleri nedeniyle satır numarası kaymış node_id'leri çözmek için kullanılır.
    pub fn find_node_fuzzy_by_id(&self, id: &str) -> Option<CodeNode> {
        if let Some(node) = self.find_node_by_id(id) {
            return Some(node);
        }

        let (path_part, kind_part, row_part) = parse_node_id(id)?;
        let target_row: usize = row_part.parse().ok()?;

        let mut best: Option<&CodeNode> = None;
        let mut best_dist = usize::MAX;

        for node in self.graph.node_weights() {
            let (node_path, node_kind, node_row) =
                if let Some((path, kind)) = parse_stable_node_id(&node.id) {
                    (path, kind, node.start_line.saturating_sub(1))
                } else if let Some((path, kind, row)) = parse_node_id(&node.id) {
                    let Ok(row) = row.parse::<usize>() else {
                        continue;
                    };
                    (path, kind, row)
                } else {
                    continue;
                };
            if node_path != path_part || node_kind != kind_part {
                continue;
            }
            let dist = node_row.abs_diff(target_row);
            if dist < best_dist && dist <= 200 {
                best_dist = dist;
                best = Some(node);
            }
        }

        if let Some(node) = best {
            tracing::debug!(
                requested = %id,
                found = %node.id,
                drift = %best_dist,
                "Fuzzy node ID matched"
            );
            return Some(node.clone());
        }

        None
    }

    /// Retrieves outgoing edges for a given node ID.
    /// Returns Vec<(TargetID, EdgeType)>.
    pub fn get_outgoing_edges(&self, id: &str) -> Vec<(String, EdgeType)> {
        let mut edges = Vec::new();

        if let Some(idx) = self.find_node_index_by_id(id) {
            for edge in self
                .graph
                .edges_directed(idx, petgraph::Direction::Outgoing)
            {
                let target_node = &self.graph[edge.target()];
                edges.push((target_node.id.clone(), edge.weight().clone()));
            }
        }

        edges
    }

    pub fn find_node_index_by_id(&self, id: &str) -> Option<NodeIndex> {
        self.id_index.get(id).cloned()
    }

    /// Saves the graph to a JSON file.
    pub fn save_to_file(&self, path: &str) -> anyhow::Result<()> {
        let path = std::path::Path::new(path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let temp_path = path.with_extension(format!("json.{}.tmp", std::process::id()));
        let file = std::fs::File::create(&temp_path)?;
        let mut writer = std::io::BufWriter::new(file);
        serde_json::to_writer(&mut writer, &self.graph)?;
        use std::io::Write;
        writer.flush()?;
        writer.get_ref().sync_all()?;
        std::fs::rename(temp_path, path)?;
        Ok(())
    }

    /// Loads the graph from a JSON file.
    pub fn load_from_file(path: &str) -> anyhow::Result<Self> {
        let file = std::fs::File::open(path)?;
        let reader = std::io::BufReader::new(file);
        let graph: DiGraph<CodeNode, EdgeType> = serde_json::from_reader(reader)?;
        let mut g = Self {
            graph,
            id_index: std::collections::HashMap::new(),
            name_index: std::collections::HashMap::new(),
            file_nodes_index: std::collections::HashMap::new(),
        };
        g.rebuild_index();
        Ok(g)
    }

    /// Rebuilds the HashMap index from the current graph nodes
    pub fn rebuild_index(&mut self) {
        self.id_index.clear();
        self.name_index.clear();
        self.file_nodes_index.clear();
        for idx in self.graph.node_indices() {
            let node = &self.graph[idx];
            self.id_index.insert(node.id.clone(), idx);
            self.name_index
                .entry(node.name.clone())
                .or_default()
                .push(idx);
            let file_id = graph_node_file_path(&node.id).to_string();
            self.file_nodes_index.entry(file_id).or_default().push(idx);
        }
    }

    /// Returns all node indices matching the given symbol/node name.
    pub fn find_nodes_by_name(&self, name: &str) -> &[NodeIndex] {
        self.name_index
            .get(name)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Returns all node indices belonging to the given file ID.
    pub fn find_nodes_by_file(&self, file_id: &str) -> &[NodeIndex] {
        self.file_nodes_index
            .get(file_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Alias for load_from_file to match API conventions
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        Self::load_from_file(path)
    }

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
            // find_edge belirsiz kenar döndürebilir; tüm kenarlar taranır.
            for edge in self
                .graph
                .edges_directed(idx, petgraph::Direction::Outgoing)
            {
                if matches!(edge.weight(), EdgeType::Contains) {
                    to_remove.insert(edge.target());
                    stack.push(edge.target());
                }
            }
        }

        // 3. Remove nodes using retain_nodes to avoid index swaps.
        self.graph.retain_nodes(|g, idx| {
            if to_remove.contains(&idx) {
                let node = &g[idx];
                self.id_index.remove(&node.id);
                false
            } else {
                true
            }
        });

        // retain_nodes shifts indices! We MUST rebuild the index entirely.
        self.rebuild_index();
    }
}

fn is_referenceable_symbol(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(is_identifier_start)
        && chars.all(|ch| is_identifier_start(ch) || ch.is_numeric())
}

fn graph_node_file_path(node_id: &str) -> &str {
    if let Some((path_and_kind, _)) = node_id.split_once(":symbol:") {
        return path_and_kind
            .rsplit_once(':')
            .map(|(path, _)| path)
            .unwrap_or(node_id);
    }
    node_id.rsplitn(4, ':').last().unwrap_or(node_id)
}

fn is_identifier_start(ch: char) -> bool {
    ch == '_' || ch == '$' || ch.is_alphabetic()
}

fn identifier_tokens(content: &str) -> Vec<(&str, bool)> {
    let mut tokens = Vec::new();
    let mut chars = content.char_indices().peekable();

    while let Some((start, first)) = chars.next() {
        if !is_identifier_start(first) {
            continue;
        }

        let mut end = start + first.len_utf8();
        while let Some(&(index, ch)) = chars.peek() {
            if !is_identifier_start(ch) && !ch.is_numeric() {
                break;
            }
            chars.next();
            end = index + ch.len_utf8();
        }

        let call_like = content[end..]
            .chars()
            .find(|ch| !ch.is_whitespace())
            .is_some_and(|ch| ch == '(');
        tokens.push((&content[start..end], call_like));
    }

    tokens
}

/// "<path>:<kind>:<row>:<col>" formatındaki node ID'sini parçalarına ayırır.
/// Başarılı olursa (path, kind, row) döndürür.
fn parse_node_id(id: &str) -> Option<(&str, &str, &str)> {
    let mut parts = id.rsplitn(4, ':');
    let _col = parts.next()?;
    let row = parts.next()?;
    let kind = parts.next()?;
    let path = parts.next()?;
    Some((path, kind, row))
}

fn parse_stable_node_id(id: &str) -> Option<(&str, &str)> {
    let mut parts = id.rsplitn(5, ':');
    let _occurrence = parts.next()?;
    let _hash = parts.next()?;
    if parts.next()? != "symbol" {
        return None;
    }
    let kind = parts.next()?;
    let path = parts.next()?;
    Some((path, kind))
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
            content: "fn test() {}".into(),
            start_line: 1,
            end_line: 1,
        };
        let index = graph.add_node(node);
        assert_eq!(index.index(), 0);
    }

    #[test]
    fn test_find_nodes_by_name_and_file() {
        let mut graph = CodeGraph::new();
        let node1 = CodeNode {
            id: "./src/lib.rs:function_definition:symbol:1:0".to_string(),
            node_type: NodeType::Function,
            name: "process_data".to_string(),
            content: "fn process_data() {}".into(),
            start_line: 1,
            end_line: 5,
        };
        let node2 = CodeNode {
            id: "./src/lib.rs:function_definition:symbol:10:0".to_string(),
            node_type: NodeType::Function,
            name: "process_data".to_string(),
            content: "fn process_data() {}".into(),
            start_line: 10,
            end_line: 15,
        };

        graph.add_node(node1);
        graph.add_node(node2);

        let by_name = graph.find_nodes_by_name("process_data");
        assert_eq!(by_name.len(), 2);

        let by_file = graph.find_nodes_by_file("./src/lib.rs");
        assert_eq!(by_file.len(), 2);
    }

    #[test]
    fn remove_file_nodes_only_removes_target_file() {
        let mut graph = CodeGraph::new();

        let file_a_idx = graph.add_node(CodeNode {
            id: "./a.rs".to_string(),
            node_type: NodeType::File,
            name: "./a.rs".to_string(),
            content: "fn foo() {}".into(),
            start_line: 1,
            end_line: 1,
        });
        let func_a_idx = graph.add_node(CodeNode {
            id: "./a.rs:func:foo".to_string(),
            node_type: NodeType::Function,
            name: "foo".to_string(),
            content: "fn foo() {}".into(),
            start_line: 1,
            end_line: 1,
        });
        graph.add_edge(file_a_idx, func_a_idx, EdgeType::Contains);

        let file_b_idx = graph.add_node(CodeNode {
            id: "./b.rs".to_string(),
            node_type: NodeType::File,
            name: "./b.rs".to_string(),
            content: "fn bar() {}".into(),
            start_line: 1,
            end_line: 1,
        });
        let func_b_idx = graph.add_node(CodeNode {
            id: "./b.rs:func:bar".to_string(),
            node_type: NodeType::Function,
            name: "bar".to_string(),
            content: "fn bar() {}".into(),
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

    #[test]
    fn remove_file_nodes_follows_contains_even_with_parallel_call_edge() {
        // rebuild_reference_edges, Contains'tan sonra aynı node çifti üzerine
        // Calls kenarı ekleyebilir. find_edge bu durumda Calls'u döndürürdü
        // ve Contains kenarı izlenmeden inner node silinmeden kalırdı.
        let mut graph = CodeGraph::new();

        let file_idx = graph.add_node(CodeNode {
            id: "./f.rs".to_string(),
            node_type: NodeType::File,
            name: "./f.rs".to_string(),
            content: "".into(),
            start_line: 1,
            end_line: 10,
        });
        let outer_idx = graph.add_node(CodeNode {
            id: "./f.rs:function:symbol:0000000000000001:0".to_string(),
            node_type: NodeType::Function,
            name: "outer".to_string(),
            content: "fn outer() { fn inner() {} inner(); }".into(),
            start_line: 1,
            end_line: 10,
        });
        let inner_idx = graph.add_node(CodeNode {
            id: "./f.rs:function:symbol:0000000000000002:0".to_string(),
            node_type: NodeType::Function,
            name: "inner".to_string(),
            content: "fn inner() {}".into(),
            start_line: 3,
            end_line: 5,
        });

        graph.add_edge(file_idx, outer_idx, EdgeType::Contains);
        graph.add_edge(outer_idx, inner_idx, EdgeType::Contains);
        graph.add_edge(outer_idx, inner_idx, EdgeType::Calls);

        graph.remove_file_nodes("./f.rs");

        assert_eq!(graph.graph.node_count(), 0);
    }

    #[test]
    fn find_node_in_file_follows_contains_even_with_parallel_call_edge() {
        let mut graph = CodeGraph::new();

        let file_idx = graph.add_node(CodeNode {
            id: "./f.rs".to_string(),
            node_type: NodeType::File,
            name: "./f.rs".to_string(),
            content: "".into(),
            start_line: 1,
            end_line: 10,
        });
        let outer_idx = graph.add_node(CodeNode {
            id: "./f.rs:function:symbol:0000000000000001:0".to_string(),
            node_type: NodeType::Function,
            name: "outer".to_string(),
            content: "fn outer() { fn inner() {} inner(); }".into(),
            start_line: 1,
            end_line: 10,
        });
        let inner_idx = graph.add_node(CodeNode {
            id: "./f.rs:function:symbol:0000000000000002:0".to_string(),
            node_type: NodeType::Function,
            name: "inner".to_string(),
            content: "fn inner() {}".into(),
            start_line: 3,
            end_line: 5,
        });

        graph.add_edge(file_idx, outer_idx, EdgeType::Contains);
        graph.add_edge(outer_idx, inner_idx, EdgeType::Contains);
        graph.add_edge(outer_idx, inner_idx, EdgeType::Calls);

        let found = graph
            .find_node_in_file("./f.rs", 4)
            .expect("node for line 4");
        assert_eq!(found, inner_idx);
    }

    #[test]
    fn rebuild_reference_edges_links_imports_constructors_and_type_annotations() {
        let mut graph = CodeGraph::new();
        let class_idx = graph.add_node(CodeNode {
            id: "./detector.py:class_definition:symbol:1:0".to_string(),
            node_type: NodeType::Class,
            name: "YoloDetector".to_string(),
            content: "class YoloDetector: pass".into(),
            start_line: 1,
            end_line: 1,
        });
        let import_idx = graph.add_node(CodeNode {
            id: "./camera.py:import_from_statement:symbol:2:0".to_string(),
            node_type: NodeType::Import,
            name: "from detector import YoloDetector".to_string(),
            content: "from detector import YoloDetector".into(),
            start_line: 1,
            end_line: 1,
        });
        let function_idx = graph.add_node(CodeNode {
            id: "./camera.py:function_definition:symbol:3:0".to_string(),
            node_type: NodeType::Function,
            name: "open_camera".to_string(),
            content: "def open_camera(detector: YoloDetector):\n    return YoloDetector()".into(),
            start_line: 3,
            end_line: 4,
        });

        graph.rebuild_reference_edges();

        assert!(graph
            .graph
            .edges_connecting(import_idx, class_idx)
            .any(|edge| matches!(edge.weight(), EdgeType::Imports)));
        assert!(graph
            .graph
            .edges_connecting(function_idx, class_idx)
            .any(|edge| matches!(edge.weight(), EdgeType::Calls)));
    }

    #[test]
    fn rebuild_reference_edges_skips_ambiguous_cross_file_symbols() {
        let mut graph = CodeGraph::new();
        for (id, file) in [
            ("./a.py:function_definition:symbol:1:0", "./a.py"),
            ("./b.py:function_definition:symbol:2:0", "./b.py"),
        ] {
            graph.add_node(CodeNode {
                id: id.to_string(),
                node_type: NodeType::Function,
                name: "load".to_string(),
                content: format!("def load(): return '{file}'").into(),
                start_line: 1,
                end_line: 1,
            });
        }
        let source_idx = graph.add_node(CodeNode {
            id: "./caller.py:function_definition:symbol:3:0".to_string(),
            node_type: NodeType::Function,
            name: "run".to_string(),
            content: "def run(): return load()".into(),
            start_line: 1,
            end_line: 1,
        });

        graph.rebuild_reference_edges();

        assert_eq!(
            graph
                .graph
                .edges_directed(source_idx, petgraph::Direction::Outgoing)
                .filter(|edge| matches!(edge.weight(), EdgeType::Calls))
                .count(),
            0
        );
    }

    #[test]
    fn test_code_node_serde_arc_str() {
        // 1. Standard node
        let original_node = CodeNode {
            id: "./src/lib.rs:func:1".to_string(),
            node_type: NodeType::Function,
            name: "test_func".to_string(),
            content: "fn test_func() { println!(\"hello\"); }".into(),
            start_line: 1,
            end_line: 5,
        };

        let json = serde_json::to_string(&original_node).expect("failed to serialize node");
        let deserialized: CodeNode =
            serde_json::from_str(&json).expect("failed to deserialize node");

        assert_eq!(deserialized.id, original_node.id);
        assert_eq!(deserialized.node_type, original_node.node_type);
        assert_eq!(deserialized.name, original_node.name);
        assert_eq!(
            deserialized.content.as_ref(),
            "fn test_func() { println!(\"hello\"); }"
        );
        assert_eq!(deserialized.start_line, original_node.start_line);
        assert_eq!(deserialized.end_line, original_node.end_line);

        // 2. Empty content string
        let empty_node = CodeNode {
            id: "./empty.rs".to_string(),
            node_type: NodeType::File,
            name: "./empty.rs".to_string(),
            content: "".into(),
            start_line: 0,
            end_line: 0,
        };
        let empty_json =
            serde_json::to_string(&empty_node).expect("failed to serialize empty node");
        let deserialized_empty: CodeNode =
            serde_json::from_str(&empty_json).expect("failed to deserialize empty node");
        assert_eq!(deserialized_empty.content.as_ref(), "");

        // 3. Long content string (10KB+)
        let long_text = "x".repeat(15_000);
        let long_node = CodeNode {
            id: "./large.rs".to_string(),
            node_type: NodeType::File,
            name: "./large.rs".to_string(),
            content: long_text.as_str().into(),
            start_line: 1,
            end_line: 1000,
        };
        let long_json = serde_json::to_string(&long_node).expect("failed to serialize large node");
        let deserialized_long: CodeNode =
            serde_json::from_str(&long_json).expect("failed to deserialize large node");
        assert_eq!(deserialized_long.content.len(), 15_000);
        assert_eq!(deserialized_long.content.as_ref(), long_text.as_str());
    }
}
