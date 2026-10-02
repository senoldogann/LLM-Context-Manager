//! M3: Rust referanslarının sözdiziminden çıkarılması ve çözümü.

use anyhow::Result;
use ccm_core::engine::RetrievalEngine;
use ccm_core::graph::{CallTarget, CodeGraph, ImportBinding, NodeType, SyntaxFacts};
use ccm_core::vector::rust_facts;
use ccm_core::vector::store::LanceDbStore;
use petgraph::graph::NodeIndex;
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
