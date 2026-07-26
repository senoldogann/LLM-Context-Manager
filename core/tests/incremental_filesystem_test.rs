use anyhow::Result;
use ccm_core::graph::CodeGraph;
use tempfile::tempdir;

#[tokio::test]
async fn update_index_only_applies_added_changed_and_deleted_files() -> Result<()> {
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = tempdir()?;
    let data = project.path().join("data");
    let graph_path = data.join("ccm_graph.json");

    std::fs::write(project.path().join("untouched.rs"), "fn untouched() {}\n")?;
    std::fs::write(project.path().join("deleted.rs"), "fn deleted() {}\n")?;
    std::fs::write(project.path().join("changed.rs"), "fn before() {}\n")?;
    ccm_core::index_directory(project.path().to_string_lossy().as_ref(), None).await?;

    let initial = CodeGraph::from_file(graph_path.to_string_lossy().as_ref())?;
    let untouched_id = initial
        .graph
        .node_weights()
        .find(|node| node.name == "untouched")
        .expect("untouched node")
        .id
        .clone();

    std::fs::write(project.path().join("changed.rs"), "fn after() {}\n")?;
    std::fs::write(project.path().join("added.py"), "def added():\n    pass\n")?;
    std::fs::remove_file(project.path().join("deleted.rs"))?;
    ccm_core::update_index(project.path().to_string_lossy().as_ref(), None).await?;

    let updated = CodeGraph::from_file(graph_path.to_string_lossy().as_ref())?;
    assert!(updated
        .graph
        .node_weights()
        .any(|node| node.name == "after"));
    assert!(updated
        .graph
        .node_weights()
        .any(|node| node.name == "added"));
    assert!(updated.find_node_by_id(&untouched_id).is_some());
    assert!(!updated
        .graph
        .node_weights()
        .any(|node| node.name == "deleted"));
    assert!(!updated
        .graph
        .node_weights()
        .any(|node| node.name == "before"));

    Ok(())
}
