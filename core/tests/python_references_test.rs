//! M1: Python referanslarının sözdiziminden çıkarılması ve çözümü.

use anyhow::Result;
use ccm_core::engine::RetrievalEngine;
use ccm_core::graph::{
    usages_of, CallTarget, CodeGraph, EdgeType, ImportBinding, ReferenceFacts, UsageError,
    UsageRelation,
};
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
    (
        "app/views.py",
        "from app.models import User\n\n\ndef show(x):\n    if isinstance(x, User):\n        return User.objects\n    return None\n",
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

#[tokio::test]
async fn python_calls_resolve_through_scopes_and_imports() -> Result<()> {
    let (_dir, engine) = index_fixture(FIXTURE).await?;
    let graph = engine.graph.read().await;
    let calls = vec![EdgeType::Calls];
    let ambiguous = vec![EdgeType::CallAmbiguous];
    let none: Vec<EdgeType> = Vec::new();

    let start = member(&graph, "app/core.py", "Engine", "start");
    let stop = member(&graph, "app/core.py", "Engine", "stop");
    let util_helper = node(&graph, "app/util.py", "helper");
    let other_helper = node(&graph, "app/other.py", "helper");
    let other_start = node(&graph, "app/other.py", "start");
    let engine_class = node(&graph, "app/core.py", "Engine");
    let run = node(&graph, "app/core.py", "run");
    let main = node(&graph, "app/cli.py", "main");
    let persist = node(&graph, "app/cli.py", "persist");
    let base = node(&graph, "app/models.py", "Base");
    let user = node(&graph, "app/models.py", "User");
    let base_save = member(&graph, "app/models.py", "Base", "save");
    let user_save = member(&graph, "app/models.py", "User", "save");
    let shim_flask = node(&graph, "app/shim.py", "Flask");

    assert_eq!(edge_types(&graph, start, stop), calls, "self.stop()");
    assert_eq!(
        edge_types(&graph, start, util_helper),
        calls,
        "imported helper"
    );
    assert_eq!(
        edge_types(&graph, start, other_helper),
        none,
        "same name, other module"
    );
    assert_eq!(
        edge_types(&graph, run, engine_class),
        calls,
        "Engine() constructor"
    );
    assert_eq!(
        edge_types(&graph, run, start),
        ambiguous,
        "engine.start(): receiver unknown"
    );
    assert_eq!(
        edge_types(&graph, run, other_start),
        none,
        "engine.start(): a module-level function is not an attribute of an object"
    );
    assert_eq!(
        edge_types(&graph, run, util_helper),
        none,
        "comment and string are not calls"
    );
    assert_eq!(
        edge_types(&graph, main, other_helper),
        calls,
        "module alias other.helper()"
    );
    assert_eq!(edge_types(&graph, main, util_helper), none);
    assert_eq!(
        edge_types(&graph, main, shim_flask),
        none,
        "flask is outside the project"
    );
    assert_eq!(
        edge_types(&graph, main, run),
        calls,
        "re-exported start_app"
    );
    assert_eq!(edge_types(&graph, persist, base_save), ambiguous);
    assert_eq!(edge_types(&graph, persist, user_save), ambiguous);
    assert_eq!(edge_types(&graph, user, base), vec![EdgeType::Inherits]);
    assert_eq!(
        edge_types(&graph, user_save, base_save),
        calls,
        "super().save()"
    );
    assert_eq!(
        edge_types(&graph, file_node(&graph, "app/core.py"), util_helper),
        vec![EdgeType::Imports]
    );
    assert_eq!(
        edge_types(&graph, file_node(&graph, "app/__init__.py"), run),
        vec![EdgeType::Imports]
    );
    assert_eq!(
        edge_types(&graph, file_node(&graph, "app/cli.py"), run),
        vec![EdgeType::Imports]
    );
    // Python dışı diller değişmez.
    assert_eq!(
        edge_types(
            &graph,
            node(&graph, "lib.rs", "foo"),
            node(&graph, "lib.rs", "bar")
        ),
        calls
    );
    Ok(())
}

#[tokio::test]
async fn root_relative_module_wins_over_a_suffix_match() -> Result<()> {
    let (_dir, engine) = index_fixture(&[
        ("app/util.py", "def helper():\n    return 1\n"),
        ("tests/app/util.py", "def helper():\n    return 2\n"),
        (
            "app/core.py",
            "from app.util import helper\n\n\ndef run():\n    return helper()\n",
        ),
        ("src/pkg/mod.py", "def work():\n    return 1\n"),
        (
            "src/pkg/use.py",
            "from pkg.mod import work\n\n\ndef go():\n    return work()\n",
        ),
    ])
    .await?;
    let graph = engine.graph.read().await;
    let run = node(&graph, "app/core.py", "run");
    assert_eq!(
        edge_types(&graph, run, node(&graph, "app/util.py", "helper")),
        vec![EdgeType::Calls]
    );
    assert!(edge_types(&graph, run, node(&graph, "tests/app/util.py", "helper")).is_empty());
    let go = node(&graph, "src/pkg/use.py", "go");
    assert_eq!(
        edge_types(&graph, go, node(&graph, "src/pkg/mod.py", "work")),
        vec![EdgeType::Calls]
    );
    Ok(())
}

#[tokio::test]
async fn a_method_name_shared_by_many_classes_produces_no_edge() -> Result<()> {
    let mut source = String::new();
    for index in 0..6 {
        source.push_str(&format!(
            "class C{index}:\n    def get(self):\n        return {index}\n\n\n"
        ));
    }
    source.push_str("def use(x):\n    return x.get()\n");
    let (_dir, engine) = index_fixture(&[("app/many.py", source.as_str())]).await?;
    let graph = engine.graph.read().await;
    let use_idx = node(&graph, "app/many.py", "use");
    let outgoing = graph
        .graph
        .edges_directed(use_idx, Direction::Outgoing)
        .filter(|edge| !matches!(edge.weight(), EdgeType::Contains))
        .count();
    assert_eq!(outgoing, 0, "x.get() with 6 candidates must not link");
    Ok(())
}

#[tokio::test]
async fn incremental_updates_follow_import_and_reexport_changes() -> Result<()> {
    let (dir, engine) = index_fixture(FIXTURE).await?;
    let root = dir.path().to_string_lossy().to_string();

    std::fs::write(
        dir.path().join("app/cli.py"),
        CLI.replace("import app.other as other", "import app.util as other"),
    )?;
    engine
        .incremental_index_paths(&root, &[PathBuf::from("app/cli.py")])
        .await?;
    {
        let graph = engine.graph.read().await;
        let main = node(&graph, "app/cli.py", "main");
        assert_eq!(
            edge_types(&graph, main, node(&graph, "app/util.py", "helper")),
            vec![EdgeType::Calls]
        );
        assert!(edge_types(&graph, main, node(&graph, "app/other.py", "helper")).is_empty());
    }

    std::fs::write(
        dir.path().join("app/__init__.py"),
        "from .other import start as start_app\n",
    )?;
    engine
        .incremental_index_paths(&root, &[PathBuf::from("app/__init__.py")])
        .await?;
    let graph = engine.graph.read().await;
    let main = node(&graph, "app/cli.py", "main");
    assert_eq!(
        edge_types(&graph, main, node(&graph, "app/other.py", "start")),
        vec![EdgeType::Calls]
    );
    assert!(edge_types(&graph, main, node(&graph, "app/core.py", "run")).is_empty());
    Ok(())
}

#[tokio::test]
async fn usages_report_relations_and_a_missing_node() -> Result<()> {
    let (_dir, engine) = index_fixture(FIXTURE).await?;
    let graph = engine.graph.read().await;

    let base_save = member(&graph, "app/models.py", "Base", "save");
    let report = usages_of(&graph, &graph.graph[base_save].id)?;
    let seen: Vec<(String, UsageRelation)> = report
        .usages
        .iter()
        .map(|usage| (usage.node.name.clone(), usage.relation))
        .collect();
    assert_eq!(
        seen,
        vec![
            ("save".to_string(), UsageRelation::Calls),
            ("persist".to_string(), UsageRelation::MayCall)
        ]
    );

    let run = node(&graph, "app/core.py", "run");
    let relations: Vec<UsageRelation> = usages_of(&graph, &graph.graph[run].id)?
        .usages
        .iter()
        .map(|usage| usage.relation)
        .collect();
    assert_eq!(
        relations,
        vec![
            UsageRelation::Calls,
            UsageRelation::Imports,
            UsageRelation::Imports
        ]
    );

    // Kararlı kimliği artık olmayan sembol, dosyadaki tek fonksiyona bulanık
    // eşleşmemeli; boş liste yerine açık hata dönmeli.
    let gone = "./app/util.py:function_definition:symbol:ffffffffffffffff:0";
    assert_eq!(
        usages_of(&graph, gone).unwrap_err(),
        UsageError::NodeNotFound(gone.to_string())
    );
    Ok(())
}

/// Grafın `Contains` dışı kenarları (kaynak kimliği, hedef kimliği, tür).
fn edge_triples(graph: &CodeGraph) -> std::collections::BTreeSet<(String, String, String)> {
    graph
        .graph
        .edge_indices()
        .filter_map(|edge| {
            let (source, target) = graph.graph.edge_endpoints(edge)?;
            let weight = &graph.graph[edge];
            (!matches!(weight, EdgeType::Contains)).then(|| {
                (
                    graph.graph[source].id.clone(),
                    graph.graph[target].id.clone(),
                    format!("{weight:?}"),
                )
            })
        })
        .collect()
}

/// Dosyaları değiştirip artımlı indeksler; kenarları aynı son durumun taze
/// indeksiyle karşılaştırır.
async fn assert_incremental_matches_fresh(
    initial: &[(&str, &str)],
    changes: &[(&str, &str)],
) -> Result<()> {
    let (dir, engine) = index_fixture(initial).await?;
    let root = dir.path().to_string_lossy().to_string();
    let mut changed = Vec::new();
    for (path, content) in changes {
        std::fs::write(dir.path().join(path), content)?;
        changed.push(PathBuf::from(path));
    }
    engine.incremental_index_paths(&root, &changed).await?;
    let mut final_files: Vec<(&str, &str)> = initial
        .iter()
        .filter(|(path, _)| !changes.iter().any(|(changed, _)| changed == path))
        .copied()
        .collect();
    final_files.extend(changes.iter().copied());
    let (_fresh_dir, fresh) = index_fixture(&final_files).await?;
    let incremental_edges = edge_triples(&*engine.graph.read().await);
    let fresh_edges = edge_triples(&*fresh.graph.read().await);
    assert_eq!(incremental_edges, fresh_edges);
    Ok(())
}

#[tokio::test]
async fn impact_of_change_reports_python_importers_and_users() -> Result<()> {
    let (_dir, engine) = index_fixture(FIXTURE).await?;
    let (start_id, show_id) = {
        let graph = engine.graph.read().await;
        (
            graph.graph[member(&graph, "app/core.py", "Engine", "start")]
                .id
                .clone(),
            graph.graph[node(&graph, "app/views.py", "show")].id.clone(),
        )
    };
    let ids = |results: Vec<ccm_core::engine::ContextSuggestion>| -> Vec<String> {
        results
            .into_iter()
            .filter_map(|result| result.node_id)
            .collect()
    };
    let util = ids(engine.impact_of_change("./app/util.py", 50).await);
    assert!(
        util.contains(&"./app/core.py".to_string()),
        "module importer: {util:?}"
    );
    assert!(util.contains(&start_id), "caller of helper: {util:?}");
    let models = ids(engine.impact_of_change("./app/models.py", 50).await);
    assert!(
        models.contains(&"./app/views.py".to_string()),
        "module importer: {models:?}"
    );
    assert!(
        models.contains(&show_id),
        "function using User without calling it: {models:?}"
    );
    Ok(())
}

#[tokio::test]
async fn incremental_refresh_follows_reexport_chains() -> Result<()> {
    assert_incremental_matches_fresh(
        &[
            ("app/__init__.py", "from .core import run as start_app\n"),
            ("app/core.py", "from .impl import execute as run\n"),
            ("app/impl.py", "def other():\n    return 0\n"),
            (
                "app/cli.py",
                "from . import start_app\n\n\ndef main():\n    return start_app()\n",
            ),
        ],
        &[(
            "app/impl.py",
            "def other():\n    return 0\n\n\ndef execute():\n    return 1\n",
        )],
    )
    .await
}

#[tokio::test]
async fn incremental_refresh_follows_base_class_changes() -> Result<()> {
    assert_incremental_matches_fresh(
        &[
            ("app/mixin.py", "class Mixin:\n    def x(self):\n        return 1\n"),
            ("app/base.py", "class Base:\n    pass\n"),
            (
                "app/sub.py",
                "from app.base import Base\n\n\nclass Sub(Base):\n    def f(self):\n        return self.x()\n",
            ),
        ],
        &[(
            "app/base.py",
            "from app.mixin import Mixin\n\n\nclass Base(Mixin):\n    pass\n",
        )],
    )
    .await
}

#[tokio::test]
async fn star_reexports_in_a_package_resolve() -> Result<()> {
    let (_dir, engine) = index_fixture(&[
        ("pkg/__init__.py", "from .aggregates import *\nfrom .forms import *\n"),
        ("pkg/aggregates.py", "class Sum:\n    pass\n"),
        ("pkg/forms.py", "class Form:\n    pass\n"),
        (
            "app/use.py",
            "import pkg\nfrom pkg import Sum\n\n\ndef total():\n    return Sum()\n\n\nclass F(pkg.Form):\n    pass\n",
        ),
    ])
    .await?;
    let graph = engine.graph.read().await;
    let sum = node(&graph, "pkg/aggregates.py", "Sum");
    assert_eq!(
        edge_types(&graph, node(&graph, "app/use.py", "total"), sum),
        vec![EdgeType::Calls]
    );
    assert_eq!(
        edge_types(&graph, file_node(&graph, "app/use.py"), sum),
        vec![EdgeType::Imports]
    );
    assert_eq!(
        edge_types(
            &graph,
            node(&graph, "app/use.py", "F"),
            node(&graph, "pkg/forms.py", "Form")
        ),
        vec![EdgeType::Inherits]
    );
    Ok(())
}

#[tokio::test]
async fn standard_library_imports_do_not_resolve_into_project_subpackages() -> Result<()> {
    let (_dir, engine) = index_fixture(&[
        ("src/flask/__init__.py", "VERSION = 1\n"),
        (
            "src/flask/json/__init__.py",
            "def dumps(obj):\n    return str(obj)\n",
        ),
        (
            "src/flask/json/provider.py",
            "import json\n\n\ndef dumps(obj):\n    return json.dumps(obj)\n",
        ),
    ])
    .await?;
    let graph = engine.graph.read().await;
    let provider_dumps = node(&graph, "src/flask/json/provider.py", "dumps");
    let package_dumps = node(&graph, "src/flask/json/__init__.py", "dumps");
    assert!(
        edge_types(&graph, provider_dumps, package_dumps).is_empty(),
        "`import json` is the standard library"
    );
    Ok(())
}

#[tokio::test]
async fn recursion_does_not_link_to_a_same_named_function_elsewhere() -> Result<()> {
    let (_dir, engine) = index_fixture(&[
        ("app/a.py", "def walk(n):\n    return walk(n - 1)\n"),
        ("app/b.py", "def walk():\n    return 0\n"),
    ])
    .await?;
    let graph = engine.graph.read().await;
    let walk = node(&graph, "app/a.py", "walk");
    let outgoing = graph
        .graph
        .edges_directed(walk, Direction::Outgoing)
        .filter(|edge| !matches!(edge.weight(), EdgeType::Contains))
        .count();
    assert_eq!(outgoing, 0, "a recursive call is not a call to b.walk");
    Ok(())
}

#[tokio::test]
async fn usages_label_non_call_uses_and_ambiguous_imports() -> Result<()> {
    let (_dir, engine) = index_fixture(FIXTURE).await?;
    let graph = engine.graph.read().await;
    let user = node(&graph, "app/models.py", "User");
    let relations: Vec<(String, UsageRelation)> = usages_of(&graph, &graph.graph[user].id)?
        .usages
        .iter()
        .map(|usage| (usage.node.name.clone(), usage.relation))
        .collect();
    assert!(
        relations.contains(&("show".to_string(), UsageRelation::References)),
        "isinstance(x, User) and User.objects: {relations:?}"
    );
    drop(graph);

    let (_dir, engine) = index_fixture(&[
        ("a/utils.py", "def helper():\n    return 1\n"),
        ("b/utils.py", "def helper():\n    return 2\n"),
        ("app/x.py", "from utils import helper\n"),
    ])
    .await?;
    let graph = engine.graph.read().await;
    let helper = node(&graph, "a/utils.py", "helper");
    let relations: Vec<UsageRelation> = usages_of(&graph, &graph.graph[helper].id)?
        .usages
        .iter()
        .map(|usage| usage.relation)
        .collect();
    assert_eq!(relations, vec![UsageRelation::MayImport]);
    Ok(())
}
