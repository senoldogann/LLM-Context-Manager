//! Sözdizimi çıkarıcısı olmayan dillerde ad eşleşmesi kesin ilişki gibi etiketlenmez.

use anyhow::Result;
use ccm_core::graph::{CodeGraph, EdgeType, NodeType, UsageRelation};

/// Go fikstürü: `Run` tek tanımlı `Helper`'ı çağırır ve `Engine` türünü anar.
const FILES: &[(&str, &str)] = &[
    (
        "pkg/helper.go",
        "package pkg\n\nfunc Helper() int {\n\treturn 1\n}\n\ntype Engine struct{}\n",
    ),
    (
        "pkg/run.go",
        "package pkg\n\nfunc Run() int {\n\tvar e Engine\n\t_ = e\n\treturn Helper()\n}\n",
    ),
];

#[tokio::test]
async fn name_matches_are_inferred_not_resolved() -> Result<()> {
    let dir = tempfile::tempdir()?;
    for (path, content) in FILES {
        let full = dir.path().join(path);
        std::fs::create_dir_all(full.parent().expect("parent"))?;
        std::fs::write(full, content)?;
    }
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = dir.path().to_string_lossy().to_string();
    ccm_core::index_directory(&project, None).await?;
    let artifacts = ccm_core::resolve_index_artifacts(&project, None)?;
    let graph = CodeGraph::from_file(&artifacts.graph_path.to_string_lossy())?;
    let edge = |from: &str, to: &str| -> Vec<EdgeType> {
        let source = graph.find_nodes_by_name(from)[0];
        let target = graph.find_nodes_by_name(to)[0];
        graph
            .graph
            .edges_connecting(source, target)
            .map(|edge| edge.weight().clone())
            .collect()
    };
    assert_eq!(edge("Run", "Helper"), vec![EdgeType::CallInferred]);
    assert_eq!(edge("Run", "Engine"), vec![EdgeType::References]);
    // Etiket kesinlik iddia etmez: aynı dosya kuralı başka tanımlar varken de bağlar.
    assert_eq!(
        UsageRelation::CallsInferred.label(),
        "calls (inferred from the name, not resolved)"
    );
    Ok(())
}

#[tokio::test]
async fn javascript_assignment_function_aliases_are_callable_symbols() -> Result<()> {
    let dir = tempfile::tempdir()?;
    std::fs::write(
        dir.path().join("request.js"),
        "var req = {};\n\nreq.get =\nreq.header = function header(name) {\n  return name;\n};\n\nfunction range() {\n  return req.get('Range');\n}\n",
    )?;
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = dir.path().to_string_lossy().to_string();
    ccm_core::index_directory(&project, None).await?;
    let artifacts = ccm_core::resolve_index_artifacts(&project, None)?;
    let graph = CodeGraph::from_file(&artifacts.graph_path.to_string_lossy())?;

    let callable = |name: &str| {
        graph
            .find_nodes_by_name(name)
            .iter()
            .copied()
            .find(|idx| graph.graph[*idx].node_type == NodeType::Function)
            .unwrap_or_else(|| panic!("missing callable alias {name}"))
    };
    let get = callable("get");
    let header = callable("header");
    let range = callable("range");

    assert_ne!(get, header, "each assignment alias should be addressable");
    assert_eq!(
        graph
            .graph
            .edges_connecting(range, get)
            .map(|edge| edge.weight().clone())
            .collect::<Vec<_>>(),
        vec![EdgeType::CallInferred],
        "req.get() should link to the assignment alias named get"
    );
    Ok(())
}

#[tokio::test]
async fn javascript_named_function_expressions_are_callable_symbols() -> Result<()> {
    let dir = tempfile::tempdir()?;
    std::fs::write(
        dir.path().join("request.js"),
        "var req = {};\n\nreq.get = function get(name) {\n  return name;\n};\n\ndefineGetter(req, 'protocol', function protocol() {\n  var header = this.get('X-Forwarded-Proto');\n  return header;\n});\n",
    )?;
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = dir.path().to_string_lossy().to_string();
    ccm_core::index_directory(&project, None).await?;
    let artifacts = ccm_core::resolve_index_artifacts(&project, None)?;
    let graph = CodeGraph::from_file(&artifacts.graph_path.to_string_lossy())?;

    let function = |name: &str| {
        graph
            .find_nodes_by_name(name)
            .iter()
            .copied()
            .find(|idx| graph.graph[*idx].node_type == NodeType::Function)
            .unwrap_or_else(|| panic!("missing function {name}"))
    };
    let get = function("get");
    let protocol = function("protocol");
    let header_var = graph
        .find_nodes_by_name("header")
        .iter()
        .copied()
        .find(|idx| graph.graph[*idx].node_type == NodeType::Variable)
        .expect("missing header variable");

    assert_eq!(
        graph
            .graph
            .edges_connecting(protocol, get)
            .map(|edge| edge.weight().clone())
            .collect::<Vec<_>>(),
        vec![EdgeType::CallInferred],
        "the enclosing callback should own this.get()"
    );
    assert!(
        graph
            .graph
            .edges_connecting(header_var, get)
            .next()
            .is_none(),
        "a variable inside a callable must not appear as a separate caller"
    );
    Ok(())
}
