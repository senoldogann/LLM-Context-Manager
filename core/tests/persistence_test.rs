use ccm_core::graph::{CodeGraph, CodeNode, NodeType};
use ccm_core::storage::{GraphStorage, SledStorage};
use std::sync::Arc;
use tempfile::tempdir;

#[test]
fn test_sled_persistence() {
    let dir = tempdir().unwrap();
    let db_path = dir.path().join("test_db");

    // 1. Create Storage and Graph
    let storage = Arc::new(SledStorage::new(&db_path).unwrap());
    let mut graph = CodeGraph::new().with_storage(storage.clone());

    // 2. Add Node
    let node = CodeNode {
        id: "node1".to_string(),
        node_type: NodeType::Function,
        name: "test_func".to_string(),
        content: "fn test() {}".to_string(),
        start_line: 1,
        end_line: 10,
    };
    graph.add_node(node.clone());

    // 3. Verify it's in Sled immediately
    let loaded_node = storage.get_node("node1").unwrap();
    assert!(loaded_node.is_some());
    assert_eq!(loaded_node.unwrap().name, "test_func");

    // 4. Close and Re-open Storage
    drop(graph);
    drop(storage);

    let storage2 = SledStorage::new(&db_path).unwrap();
    let loaded_node_2 = storage2.get_node("node1").unwrap();
    assert!(loaded_node_2.is_some());
    assert_eq!(loaded_node_2.unwrap().content, "fn test() {}");
}
