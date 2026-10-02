//! Sözdizimi çıkarıcısı olmayan dillerde ad eşleşmesi kesin ilişki gibi etiketlenmez.

use anyhow::Result;
use ccm_core::graph::{CodeGraph, EdgeType};

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
    Ok(())
}
