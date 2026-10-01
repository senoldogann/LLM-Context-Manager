//! M1: Python referanslarının sözdiziminden çıkarılması ve çözümü.

use anyhow::Result;
use ccm_core::engine::RetrievalEngine;
use ccm_core::graph::{CallTarget, CodeGraph, EdgeType, ImportBinding, ReferenceFacts};
use ccm_core::vector::store::LanceDbStore;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use std::path::PathBuf;
use std::sync::Arc;
use tempfile::{tempdir, TempDir};
use tokio::sync::RwLock;

const CLI: &str = r#"import app.other as other
from flask import Flask
from . import start_app


def main():
    other.helper()
    Flask(__name__)
    return start_app()


def persist(record):
    return record.save()
"#;

const FIXTURE: &[(&str, &str)] = &[
    ("app/__init__.py", "from .core import run as start_app\n"),
    ("app/util.py", "def helper():\n    return 1\n"),
    (
        "app/other.py",
        "def helper():\n    return 2\n\n\ndef start():\n    return 3\n",
    ),
    (
        "app/core.py",
        r#"from app.util import helper


class Engine:
    def start(self):
        self.stop()
        return helper()

    def stop(self):
        return 0


def run():
    # helper() yorumda: kenar değil
    text = "helper()"
    engine = Engine()
    return engine.start()
"#,
    ),
    ("app/cli.py", CLI),
    (
        "app/models.py",
        r#"class Base:
    def save(self):
        return 1


class User(Base):
    def save(self):
        return super().save()
"#,
    ),
    (
        "app/shim.py",
        "class Flask:\n    def __init__(self, name):\n        self.name = name\n",
    ),
    ("app/broken.py", "def broken(:\n    return helper(\n"),
    ("lib.rs", "fn bar() {}\nfn foo() { bar(); }\n"),
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

/// Dosyadaki adı tek olan düğüm. Dosya kimlikleri `./göreli/yol` biçimindedir.
fn node(graph: &CodeGraph, file: &str, name: &str) -> NodeIndex {
    let prefix = format!("./{file}:");
    let found: Vec<NodeIndex> = graph
        .graph
        .node_indices()
        .filter(|idx| graph.graph[*idx].name == name && graph.graph[*idx].id.starts_with(&prefix))
        .collect();
    assert_eq!(
        found.len(),
        1,
        "expected one {name} in {file}, found {found:?}"
    );
    found[0]
}

/// Sınıfın doğrudan üyesi.
fn member(graph: &CodeGraph, file: &str, class: &str, name: &str) -> NodeIndex {
    let class_idx = node(graph, file, class);
    let found: Vec<NodeIndex> = graph
        .graph
        .edges_directed(class_idx, Direction::Outgoing)
        .filter(|edge| matches!(edge.weight(), EdgeType::Contains))
        .map(|edge| edge.target())
        .filter(|idx| graph.graph[*idx].name == name)
        .collect();
    assert_eq!(found.len(), 1, "expected one {class}.{name} in {file}");
    found[0]
}

/// Dosya düğümü.
fn file_node(graph: &CodeGraph, file: &str) -> NodeIndex {
    graph
        .graph
        .node_indices()
        .find(|idx| graph.graph[*idx].id == format!("./{file}"))
        .unwrap_or_else(|| panic!("file node {file} missing"))
}

/// İki düğüm arasındaki `Contains` dışı kenar türleri.
#[allow(dead_code)]
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

/// Düğümün sözdizimi olguları; sözcükselse test düşer.
fn syntax(graph: &CodeGraph, idx: NodeIndex) -> &ccm_core::graph::SyntaxFacts {
    match &graph.graph[idx].facts {
        ReferenceFacts::Syntax(facts) => facts,
        ReferenceFacts::Lexical => panic!("{} has lexical facts", graph.graph[idx].id),
    }
}

#[tokio::test]
async fn python_facts_come_from_the_syntax_tree() -> Result<()> {
    let (_dir, engine) = index_fixture(FIXTURE).await?;
    let graph = engine.graph.read().await;

    let start = member(&graph, "app/core.py", "Engine", "start");
    let targets: Vec<&CallTarget> = syntax(&graph, start)
        .calls
        .iter()
        .map(|call| &call.target)
        .collect();
    assert_eq!(
        targets,
        vec![
            &CallTarget::SelfMember("stop".to_string()),
            &CallTarget::Bare("helper".to_string())
        ]
    );

    // Yorum ve string içindeki `helper()` çağrı değildir.
    let run = node(&graph, "app/core.py", "run");
    assert!(syntax(&graph, run)
        .calls
        .iter()
        .all(|call| call.target.name() != "helper"));

    assert_eq!(
        syntax(&graph, file_node(&graph, "app/core.py")).imports,
        vec![ImportBinding {
            local: "helper".into(),
            module: "app.util".into(),
            symbol: Some("helper".into()),
        }]
    );
    let cli_imports = &syntax(&graph, file_node(&graph, "app/cli.py")).imports;
    assert!(cli_imports.contains(&ImportBinding {
        local: "start_app".into(),
        module: "app".into(),
        symbol: Some("start_app".into()),
    }));
    assert!(cli_imports.contains(&ImportBinding {
        local: "other".into(),
        module: "app.other".into(),
        symbol: None,
    }));

    let user = node(&graph, "app/models.py", "User");
    assert_eq!(
        syntax(&graph, user).bases,
        vec![CallTarget::Bare("Base".to_string())]
    );
    let user_save = member(&graph, "app/models.py", "User", "save");
    assert_eq!(
        syntax(&graph, user_save)
            .calls
            .iter()
            .map(|call| &call.target)
            .collect::<Vec<_>>(),
        vec![
            &CallTarget::SuperMember("save".to_string()),
            &CallTarget::Bare("super".to_string())
        ]
    );

    // Sözdizimi çıkarıcısı olmayan diller sözcüksel kalır.
    let foo = node(&graph, "lib.rs", "foo");
    assert_eq!(graph.graph[foo].facts, ReferenceFacts::Lexical);
    Ok(())
}
