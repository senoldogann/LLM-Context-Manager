//! M3: Rust referanslarının sözdiziminden çıkarılması ve çözümü.

use anyhow::Result;
use ccm_core::engine::RetrievalEngine;
use ccm_core::graph::{CallTarget, CodeGraph, EdgeType, ImportBinding, NodeType, SyntaxFacts};
use ccm_core::vector::rust_facts;
use ccm_core::vector::store::LanceDbStore;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::{tempdir, TempDir};
use tokio::sync::RwLock;

const ENGINE_RS: &str = r#"use crate::util::helper;

pub struct Engine {
    pub mode: crate::util::Mode,
}

pub trait Runner {
    fn run(&self) -> u32;
}

impl Engine {
    pub fn new() -> Self {
        Engine { mode: crate::util::Mode::Fast }
    }

    pub fn start(&self) -> u32 {
        self.stop();
        helper()
    }

    pub fn describe(&self) -> usize {
        let text = "helper()";
        text.len()
    }

    fn stop(&self) {}
}

impl Runner for Engine {
    fn run(&self) -> u32 {
        self.start()
    }
}
"#;

const MAIN_RS: &str = r#"use app_core::util::{self, Mode};
use app_core::Engine;

fn main() {
    let engine = Engine::new();
    engine.start();
    util::helper();
    let _mode = Mode::Slow;
    println!("{}", compute());
}

fn compute() -> u32 {
    serde_json::to_string(&1).map(|text| text.len() as u32).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes() {
        assert_eq!(compute(), 1);
    }
}
"#;

/// İki crate'li çalışma alanı: `app-core` kütüphanesi ve onu kullanan `app-cli`.
const FIXTURE: &[(&str, &str)] = &[
    ("Cargo.toml", "[workspace]\nmembers = [\"app_core\", \"app_cli\"]\n"),
    (
        "app_core/Cargo.toml",
        "[package]\nname = \"app-core\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    ),
    (
        "app_core/src/lib.rs",
        "pub mod engine;\npub mod other;\npub mod util;\n\npub use engine::Engine;\n",
    ),
    (
        "app_core/src/util.rs",
        "/// helper() bir yorumda: kenar değil\npub fn helper() -> u32 {\n    1\n}\n\npub enum Mode {\n    Fast,\n    Slow,\n}\n\nimpl Mode {\n    pub fn len(&self) -> usize {\n        0\n    }\n}\n",
    ),
    ("app_core/src/other.rs", "pub fn helper() -> u32 {\n    2\n}\n"),
    ("app_core/src/engine.rs", ENGINE_RS),
    (
        "app_cli/Cargo.toml",
        "[package]\nname = \"app-cli\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    ),
    ("app_cli/src/main.rs", MAIN_RS),
];

/// Dosyaları geçici bir projeye yazar ve gömücüsüz indeksler.
async fn index_fixture(files: &[(&str, &str)]) -> Result<(TempDir, RetrievalEngine)> {
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let dir = tempdir()?;
    let mut paths = Vec::new();
    for (path, content) in files {
        let full = dir.path().join(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&full, content)?;
        paths.push(PathBuf::from(path));
    }
    let db_path = dir.path().join("db");
    std::fs::create_dir_all(&db_path)?;
    let store = LanceDbStore::new(db_path.to_string_lossy().as_ref(), "code_vectors").await?;
    let engine = RetrievalEngine::new(Arc::new(RwLock::new(CodeGraph::new())), store);
    engine
        .incremental_index_paths(dir.path().to_string_lossy().as_ref(), &paths)
        .await?;
    Ok((dir, engine))
}

/// Dosyada verilen türde ve adda tek düğüm.
fn typed(graph: &CodeGraph, file: &str, name: &str, node_type: NodeType) -> NodeIndex {
    let prefix = format!("./{file}:");
    let found: Vec<NodeIndex> = graph
        .graph
        .node_indices()
        .filter(|idx| {
            let node = &graph.graph[*idx];
            node.name == name && node.node_type == node_type && node.id.starts_with(&prefix)
        })
        .collect();
    assert_eq!(
        found.len(),
        1,
        "expected one {node_type:?} {name} in {file}, found {}",
        found.len()
    );
    found[0]
}

/// Dosyadaki verilen türün `impl` blokları, kaynak sırasıyla.
fn impls(graph: &CodeGraph, file: &str, type_name: &str) -> Vec<NodeIndex> {
    let prefix = format!("./{file}:");
    let mut found: Vec<NodeIndex> = graph
        .graph
        .node_indices()
        .filter(|idx| {
            let node = &graph.graph[*idx];
            node.node_type == NodeType::Class
                && node.name == type_name
                && node.id.starts_with(&prefix)
        })
        .collect();
    found.sort_by_key(|idx| graph.graph[*idx].start_line);
    found
}

/// Sahibin (`impl`, `trait`, `mod`) adı verilen tek doğrudan üyesi.
fn member_of(graph: &CodeGraph, owner: NodeIndex, name: &str) -> NodeIndex {
    let found: Vec<NodeIndex> = graph
        .graph
        .edges_directed(owner, Direction::Outgoing)
        .filter(|edge| matches!(edge.weight(), EdgeType::Contains))
        .map(|edge| edge.target())
        .filter(|idx| graph.graph[*idx].name == name)
        .collect();
    assert_eq!(found.len(), 1, "expected one member {name}");
    found[0]
}

/// Dosya düğümü.
fn file_node(graph: &CodeGraph, file: &str) -> NodeIndex {
    graph
        .find_file_node(&format!("./{file}"))
        .unwrap_or_else(|| panic!("file node {file} missing"))
}

/// İki düğüm arasındaki `Contains` dışı kenar türleri.
fn edge_types(graph: &CodeGraph, from: NodeIndex, to: NodeIndex) -> Vec<EdgeType> {
    let mut types: Vec<EdgeType> = graph
        .graph
        .edges_connecting(from, to)
        .map(|edge| edge.weight().clone())
        .filter(|weight| !matches!(weight, EdgeType::Contains))
        .collect();
    types.sort_by_key(|weight| format!("{weight:?}"));
    types
}

/// Düğümün `Contains` dışı tüm çıkan kenarları.
fn outgoing(graph: &CodeGraph, from: NodeIndex) -> Vec<(NodeIndex, EdgeType)> {
    graph
        .graph
        .edges_directed(from, Direction::Outgoing)
        .filter(|edge| !matches!(edge.weight(), EdgeType::Contains))
        .map(|edge| (edge.target(), edge.weight().clone()))
        .collect()
}

/// Kaynağı tree-sitter-rust ile ayrıştırır.
fn parse(source: &str) -> tree_sitter::Tree {
    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .expect("rust grammar");
    parser.parse(source, None).expect("parse")
}

/// Ağaçtaki verilen türde düğümler, kaynak sırasıyla.
fn nodes_of_kind<'t>(root: tree_sitter::Node<'t>, kind: &str) -> Vec<tree_sitter::Node<'t>> {
    let mut found = Vec::new();
    let mut stack = vec![root];
    while let Some(node) = stack.pop() {
        if node.kind() == kind {
            found.push(node);
        }
        let mut cursor = node.walk();
        let children: Vec<tree_sitter::Node<'t>> = node.children(&mut cursor).collect();
        stack.extend(children.into_iter().rev());
    }
    found
}

/// Adı verilen türdeki ilk düğüm (`name` alanına göre).
fn named<'t>(
    root: tree_sitter::Node<'t>,
    source: &str,
    kind: &str,
    name: &str,
) -> tree_sitter::Node<'t> {
    nodes_of_kind(root, kind)
        .into_iter()
        .find(|node| {
            node.child_by_field_name("name")
                .and_then(|name_node| name_node.utf8_text(source.as_bytes()).ok())
                == Some(name)
        })
        .unwrap_or_else(|| panic!("no {kind} named {name}"))
}

fn targets(facts: &SyntaxFacts) -> Vec<CallTarget> {
    facts.calls.iter().map(|call| call.target.clone()).collect()
}

fn binding(local: &str, module: &str, symbol: &str) -> ImportBinding {
    ImportBinding {
        local: local.to_string(),
        module: module.to_string(),
        symbol: Some(symbol.to_string()),
    }
}

fn member(qualifier: &str, name: &str) -> CallTarget {
    CallTarget::Member {
        qualifier: qualifier.to_string(),
        name: name.to_string(),
    }
}

#[tokio::test]
async fn rust_facts_capture_calls_uses_bases_and_macros() -> Result<()> {
    let engine_tree = parse(ENGINE_RS);
    let engine_root = engine_tree.root_node();
    let start = rust_facts::function_facts(
        named(engine_root, ENGINE_RS, "function_item", "start"),
        ENGINE_RS,
    );
    assert_eq!(
        targets(&start),
        vec![
            CallTarget::SelfMember("stop".into()),
            CallTarget::Bare("helper".into())
        ]
    );
    let describe = rust_facts::function_facts(
        named(engine_root, ENGINE_RS, "function_item", "describe"),
        ENGINE_RS,
    );
    assert_eq!(
        targets(&describe),
        vec![CallTarget::Chained("len".into())],
        "a string is not a call"
    );
    let new = rust_facts::function_facts(
        named(engine_root, ENGINE_RS, "function_item", "new"),
        ENGINE_RS,
    );
    assert_eq!(targets(&new), vec![CallTarget::Bare("Engine".into())]);
    assert!(
        new.names.contains(&"crate.util.Mode.Fast".to_string()),
        "{:?}",
        new.names
    );
    let impls = nodes_of_kind(engine_root, "impl_item");
    assert_eq!(
        rust_facts::item_facts(impls[1], ENGINE_RS).bases,
        vec![CallTarget::Bare("Runner".into())]
    );

    let main_tree = parse(MAIN_RS);
    let main_root = main_tree.root_node();
    let main =
        rust_facts::function_facts(named(main_root, MAIN_RS, "function_item", "main"), MAIN_RS);
    for expected in [
        member("Engine", "new"),
        CallTarget::Chained("start".into()),
        member("util", "helper"),
        CallTarget::Bare("compute".into()),
    ] {
        assert!(
            targets(&main).contains(&expected),
            "{expected:?} in {:?}",
            main.calls
        );
    }
    assert!(
        main.names.contains(&"Mode.Slow".to_string()),
        "{:?}",
        main.names
    );
    let computes = rust_facts::function_facts(
        named(main_root, MAIN_RS, "function_item", "computes"),
        MAIN_RS,
    );
    assert_eq!(targets(&computes), vec![CallTarget::Bare("compute".into())]);
    let file = rust_facts::module_facts(main_root, MAIN_RS);
    for expected in [
        binding("util", "app_core", "util"),
        binding("Mode", "app_core.util", "Mode"),
        binding("Engine", "app_core", "Engine"),
    ] {
        assert!(
            file.imports.contains(&expected),
            "{expected:?} in {:?}",
            file.imports
        );
    }
    let tests_mod =
        rust_facts::module_facts(named(main_root, MAIN_RS, "mod_item", "tests"), MAIN_RS);
    assert_eq!(tests_mod.imports, vec![binding("*", "super", "*")]);

    let (_dir, engine) = index_fixture(FIXTURE).await?;
    let graph = engine.graph.read().await;
    typed(&graph, "app_core/src/util.rs", "Mode", NodeType::Enum);
    typed(&graph, "app_core/src/engine.rs", "Runner", NodeType::Trait);
    Ok(())
}

#[tokio::test]
async fn rust_calls_resolve_through_scopes_use_and_impls() -> Result<()> {
    let (_dir, engine) = index_fixture(FIXTURE).await?;
    let graph = engine.graph.read().await;
    let engine_rs = "app_core/src/engine.rs";
    let util_rs = "app_core/src/util.rs";
    let engine_impls = impls(&graph, engine_rs, "Engine");
    assert_eq!(engine_impls.len(), 2);
    let start = member_of(&graph, engine_impls[0], "start");
    let util_helper = typed(&graph, util_rs, "helper", NodeType::Function);
    let other_helper = typed(
        &graph,
        "app_core/src/other.rs",
        "helper",
        NodeType::Function,
    );
    let mode = typed(&graph, util_rs, "Mode", NodeType::Enum);
    let mode_len = member_of(&graph, impls(&graph, util_rs, "Mode")[0], "len");
    let engine_struct = typed(&graph, engine_rs, "Engine", NodeType::Struct);

    assert_eq!(
        edge_types(&graph, start, util_helper),
        vec![EdgeType::Calls],
        "use crate::util::helper"
    );
    assert_eq!(
        edge_types(&graph, start, other_helper),
        vec![],
        "same name, other module"
    );
    assert_eq!(
        edge_types(&graph, start, member_of(&graph, engine_impls[0], "stop")),
        vec![EdgeType::Calls],
        "self.stop()"
    );
    let describe = member_of(&graph, engine_impls[0], "describe");
    assert_eq!(
        edge_types(&graph, describe, util_helper),
        vec![],
        "a string is not a call"
    );
    assert_eq!(
        edge_types(&graph, describe, mode_len),
        vec![EdgeType::CallAmbiguous],
        "text.len(): the receiver type is unknown"
    );
    let new = member_of(&graph, engine_impls[0], "new");
    assert_eq!(
        edge_types(&graph, new, engine_struct),
        vec![EdgeType::Calls],
        "struct expression"
    );
    assert_eq!(
        edge_types(&graph, new, mode),
        vec![EdgeType::References],
        "enum variant path"
    );
    assert_eq!(
        edge_types(&graph, member_of(&graph, engine_impls[1], "run"), start),
        vec![EdgeType::Calls],
        "self.start() through the impl's type"
    );
    assert_eq!(
        edge_types(
            &graph,
            engine_impls[1],
            typed(&graph, engine_rs, "Runner", NodeType::Trait)
        ),
        vec![EdgeType::Inherits]
    );
    Ok(())
}

#[tokio::test]
async fn rust_paths_resolve_across_workspace_crates_and_reexports() -> Result<()> {
    let (_dir, engine) = index_fixture(FIXTURE).await?;
    let graph = engine.graph.read().await;
    let engine_rs = "app_core/src/engine.rs";
    let util_rs = "app_core/src/util.rs";
    let main_rs = "app_cli/src/main.rs";
    let engine_impls = impls(&graph, engine_rs, "Engine");
    let main = typed(&graph, main_rs, "main", NodeType::Function);
    let util_helper = typed(&graph, util_rs, "helper", NodeType::Function);
    let mode = typed(&graph, util_rs, "Mode", NodeType::Enum);
    let engine_struct = typed(&graph, engine_rs, "Engine", NodeType::Struct);

    assert_eq!(
        edge_types(&graph, main, member_of(&graph, engine_impls[0], "new")),
        vec![EdgeType::Calls],
        "app_core::Engine through `pub use engine::Engine`"
    );
    assert_eq!(
        edge_types(&graph, main, member_of(&graph, engine_impls[0], "start")),
        vec![EdgeType::CallAmbiguous],
        "engine.start(): the receiver type is unknown"
    );
    assert_eq!(
        edge_types(&graph, main, util_helper),
        vec![EdgeType::Calls],
        "util::helper()"
    );
    assert_eq!(
        edge_types(
            &graph,
            main,
            typed(
                &graph,
                "app_core/src/other.rs",
                "helper",
                NodeType::Function
            )
        ),
        vec![]
    );
    assert_eq!(
        edge_types(&graph, main, mode),
        vec![EdgeType::References],
        "Mode::Slow"
    );
    let main_file = file_node(&graph, main_rs);
    assert_eq!(
        edge_types(&graph, main_file, engine_struct),
        vec![EdgeType::Imports]
    );
    assert_eq!(edge_types(&graph, main_file, mode), vec![EdgeType::Imports]);
    assert_eq!(
        edge_types(&graph, main_file, file_node(&graph, util_rs)),
        vec![EdgeType::Imports],
        "a module import points at the module's file"
    );
    assert_eq!(
        edge_types(
            &graph,
            file_node(&graph, "app_core/src/lib.rs"),
            engine_struct
        ),
        vec![EdgeType::Imports],
        "pub use engine::Engine"
    );
    let mode_len = member_of(&graph, impls(&graph, util_rs, "Mode")[0], "len");
    assert_eq!(
        outgoing(
            &graph,
            typed(&graph, main_rs, "compute", NodeType::Function)
        ),
        vec![(mode_len, EdgeType::CallAmbiguous)],
        "serde_json and std calls produce no edge"
    );
    Ok(())
}

#[tokio::test]
async fn rust_test_modules_see_parent_items_through_glob_imports() -> Result<()> {
    let (_dir, engine) = index_fixture(FIXTURE).await?;
    let graph = engine.graph.read().await;
    let main_rs = "app_cli/src/main.rs";
    let compute = typed(&graph, main_rs, "compute", NodeType::Function);
    assert_eq!(
        edge_types(
            &graph,
            typed(&graph, main_rs, "computes", NodeType::Function),
            compute
        ),
        vec![EdgeType::Calls],
        "assert_eq!(compute(), 1) through `use super::*`"
    );
    assert_eq!(
        edge_types(
            &graph,
            typed(&graph, main_rs, "main", NodeType::Function),
            compute
        ),
        vec![EdgeType::Calls],
        "println!(\"{{}}\", compute())"
    );
    Ok(())
}
