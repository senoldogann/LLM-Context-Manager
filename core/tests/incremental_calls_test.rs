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
    assert!(matches!(edge_weight, EdgeType::Calls));

    Ok(())
}
