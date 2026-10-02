use anyhow::Result;
use ccm_core::engine::RetrievalEngine;
use ccm_core::graph::{CodeGraph, EdgeType, NodeType};
use ccm_core::vector::store::LanceDbStore;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::tempdir;
use tokio::sync::RwLock;

#[tokio::test]
async fn incremental_index_adds_call_edges() -> Result<()> {
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");

    let dir = tempdir()?;
    let file_path = dir.path().join("a.rs");
    std::fs::write(&file_path, "fn bar() {}\nfn foo() { bar(); }\n")?;

    let db_path = dir.path().join("db");
    std::fs::create_dir_all(&db_path)?;

    let store = LanceDbStore::new(db_path.to_string_lossy().as_ref(), "code_vectors").await?;
    let graph = CodeGraph::new();
    let engine = RetrievalEngine::new(Arc::new(RwLock::new(graph)), store);

    engine
        .incremental_index_paths(
            dir.path().to_string_lossy().as_ref(),
            &[PathBuf::from("a.rs")],
        )
        .await?;

    let graph = engine.graph.read().await;
    let mut foo_idx = None;
    let mut bar_idx = None;

    for idx in graph.graph.node_indices() {
        let node = &graph.graph[idx];
        if node.node_type == NodeType::Function && node.name == "foo" {
            foo_idx = Some(idx);
        }
        if node.node_type == NodeType::Function && node.name == "bar" {
            bar_idx = Some(idx);
        }
    }

    let foo_idx = foo_idx.expect("foo node not found");
    let bar_idx = bar_idx.expect("bar node not found");

    let edge_idx = graph.graph.find_edge(foo_idx, bar_idx);
    assert!(edge_idx.is_some(), "call edge not found");

    let edge_weight = graph.graph.edge_weight(edge_idx.unwrap()).unwrap();
    // Rust sözdiziminden çözülür: aynı modüldeki tanım.
    assert!(matches!(edge_weight, EdgeType::Calls));

    Ok(())
}

#[tokio::test]
async fn cross_file_reference_resolves_struct_that_has_impl_blocks() -> Result<()> {
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");

    let dir = tempdir()?;
    // Aynı adlı struct ve impl blokları referans hedefini belirsizleştirmemeli;
    // `impl Display for Foo` bloğu trait adıyla değil tip adıyla anılmalı.
    std::fs::write(
        dir.path().join("foo.rs"),
        "use std::fmt::{Display, Formatter, Result};\n\
         pub struct Foo {}\n\
         impl Foo {\n    pub fn new() -> Foo { Foo {} }\n}\n\
         impl Display for Foo {\n    fn fmt(&self, f: &mut Formatter) -> Result { Ok(()) }\n}\n",
    )?;
    // Geçerli Rust: modüller crate kökünde bildirilir, tür `use` ile kapsama girer.
    std::fs::write(dir.path().join("lib.rs"), "mod foo;\nmod user;\n")?;
    std::fs::write(
        dir.path().join("user.rs"),
        "use crate::foo::Foo;\n\nfn build() -> Foo {\n    let value: Foo = make();\n    value\n}\n",
    )?;

    let db_path = dir.path().join("db");
    std::fs::create_dir_all(&db_path)?;
    let store = LanceDbStore::new(db_path.to_string_lossy().as_ref(), "code_vectors").await?;
    let engine = RetrievalEngine::new(Arc::new(RwLock::new(CodeGraph::new())), store);
    engine
        .incremental_index_paths(
            dir.path().to_string_lossy().as_ref(),
            &[
                PathBuf::from("lib.rs"),
                PathBuf::from("foo.rs"),
                PathBuf::from("user.rs"),
            ],
        )
        .await?;

    let graph = engine.graph.read().await;
    let impl_names: Vec<&str> = graph
        .graph
        .node_weights()
        .filter(|node| node.node_type == NodeType::Class)
        .map(|node| node.name.as_str())
        .collect();
    assert_eq!(impl_names, vec!["Foo", "Foo"], "impl nodes: {impl_names:?}");

    let find = |node_type: NodeType, name: &str| {
        graph
            .graph
            .node_indices()
            .find(|idx| graph.graph[*idx].node_type == node_type && graph.graph[*idx].name == name)
            .unwrap_or_else(|| panic!("{name} node not found"))
    };
    let build_idx = find(NodeType::Function, "build");
    let struct_idx = find(NodeType::Struct, "Foo");
    assert!(
        graph.graph.find_edge(build_idx, struct_idx).is_some(),
        "cross-file reference edge to struct Foo not found"
    );

    Ok(())
}
