//! Canlı indeksin artımlı güncellemeleri, aynı ağacın sıfırdan alınmış tam
//! indeksiyle birebir aynı grafı üretmelidir (düğümler ve (kaynak, hedef, tür)
//! kenar kümesi).

use anyhow::Result;
use ccm_core::graph::CodeGraph;
use ccm_core::live::{LiveIndex, LivePersist, LiveRefresh};
use petgraph::visit::EdgeRef;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

/// (id, tür, ad, içerik, başlangıç satırı, bitiş satırı)
type NodeKey = (String, String, String, String, usize, usize);
/// (kaynak id, hedef id, kenar türü)
type EdgeKey = (String, String, String);

#[derive(Debug, PartialEq, Eq)]
struct GraphSignature {
    nodes: BTreeSet<NodeKey>,
    edges: BTreeSet<EdgeKey>,
}

fn signature(graph: &CodeGraph) -> GraphSignature {
    let nodes = graph
        .graph
        .node_weights()
        .map(|node| {
            (
                node.id.clone(),
                format!("{:?}", node.node_type),
                node.name.clone(),
                node.content.to_string(),
                node.start_line,
                node.end_line,
            )
        })
        .collect();
    let edges = graph
        .graph
        .edge_references()
        .map(|edge| {
            (
                graph.graph[edge.source()].id.clone(),
                graph.graph[edge.target()].id.clone(),
                format!("{:?}", edge.weight()),
            )
        })
        .collect();
    GraphSignature { nodes, edges }
}

/// Proje kaynaklarını (indeks artefaktları hariç) ayna dizine kopyalar ve orada
/// sıfırdan tam indeks alır.
async fn fresh_index_signature(project: &Path) -> Result<GraphSignature> {
    let mirror = tempdir()?;
    copy_sources(project, mirror.path())?;
    let mirror_path = mirror.path().to_string_lossy().to_string();
    ccm_core::index_directory(&mirror_path, None).await?;
    let artifacts = ccm_core::resolve_index_artifacts(&mirror_path, None)?;
    let graph = CodeGraph::from_file(&artifacts.graph_path.to_string_lossy())?;
    Ok(signature(&graph))
}

fn copy_sources(source: &Path, destination: &Path) -> Result<()> {
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        if entry.file_name() == "data" {
            continue;
        }
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            std::fs::create_dir_all(&target)?;
            copy_sources(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

async fn live_signature(live: &LiveIndex) -> GraphSignature {
    signature(&*live.engine().graph.read().await)
}

async fn assert_paths_step(
    live: &LiveIndex,
    project: &Path,
    step: &str,
    touched: &[PathBuf],
) -> Result<()> {
    let refresh = live.apply_paths(touched).await?;
    assert!(
        matches!(refresh, LiveRefresh::Applied(_)),
        "{step}: live refresh was not applied: {refresh:?}"
    );
    assert_eq!(
        live_signature(live).await,
        fresh_index_signature(project).await?,
        "{step}: incremental graph differs from a fresh full index"
    );
    Ok(())
}

#[tokio::test]
async fn live_incremental_updates_match_a_fresh_full_index() -> Result<()> {
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = tempdir()?;
    let root = std::fs::canonicalize(project.path())?;
    let write = |relative: &str, content: &str| -> Result<PathBuf> {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("parent directory"))?;
        std::fs::write(&path, content)?;
        Ok(path)
    };

    // Aynı adlı iki impl bloğu, struct yokken aynı dosyadaki `Shape` referansını
    // belirsiz (ImportAmbiguous) kenarlara bağlar.
    write(
        "src/shapes.rs",
        "use std::fmt::{Display, Formatter, Result};\n\n\
         impl Shape {\n    pub fn area(&self) -> f64 {\n        0.0\n    }\n}\n\n\
         impl Display for Shape {\n    fn fmt(&self, f: &mut Formatter) -> Result {\n        Ok(())\n    }\n}\n\n\
         pub fn describe(shape: &Shape) -> f64 {\n    let copy: Shape = rebuild(shape);\n    copy.area()\n}\n",
    )?;
    write(
        "src/util.rs",
        "pub fn helper() -> u32 {\n    1\n}\n\npub fn unused() {}\n",
    )?;
    let main_rs = write(
        "src/main.rs",
        "mod util;\n\nfn main() {\n    let total = helper();\n    run(total);\n}\n\nfn run(value: u32) {\n    helper();\n}\n",
    )?;
    write(
        "app/service.py",
        "def compute(x):\n    return transform(x)\n\n\ndef transform(x):\n    return x * 2\n",
    )?;
    write(
        "app/client.py",
        "from service import compute\n\n\ndef call():\n    return compute(3)\n",
    )?;
    let readme = write("README.md", "# Parity fixture\n")?;
    let project_path = root.to_string_lossy().to_string();
    ccm_core::index_directory(&project_path, None).await?;

    let live = LiveIndex::load(&project_path, None, None).await?;
    assert!(matches!(
        live.apply_rescan().await?,
        LiveRefresh::Applied(_)
    ));
    assert_eq!(
        live_signature(&live).await,
        fresh_index_signature(&root).await?
    );

    // Yeni dosya + Rust struct ile impl blokları aynı adda: `Shape` artık struct'a bağlanır.
    let model = write(
        "src/model.rs",
        "pub struct Shape {\n    pub width: f64,\n}\n",
    )?;
    assert_paths_step(&live, &root, "struct added", std::slice::from_ref(&model)).await?;

    // Sembol yeniden adlandırma: çağıranların eski hedefi kaybolur.
    let util = write(
        "src/util.rs",
        "pub fn assist() -> u32 {\n    1\n}\n\npub fn unused() {}\n",
    )?;
    assert_paths_step(&live, &root, "symbol renamed", std::slice::from_ref(&util)).await?;

    // Dosyalar arası çağıranlar yeni ada geçer.
    write(
        "src/main.rs",
        "mod util;\n\nfn main() {\n    let total = assist();\n    run(total);\n}\n\nfn run(value: u32) {\n    assist();\n}\n",
    )?;
    assert_paths_step(
        &live,
        &root,
        "callers updated",
        std::slice::from_ref(&main_rs),
    )
    .await?;

    // Başka dosyada aynı adlı sembol: dosyalar arası çağrı belirsizleşir ve düşer.
    let other = write("app/other.py", "def compute(y):\n    return y\n")?;
    assert_paths_step(
        &live,
        &root,
        "ambiguous symbol added",
        std::slice::from_ref(&other),
    )
    .await?;

    // Sembol silme (çağıranı olan ve olmayan).
    write("src/util.rs", "pub fn assist() -> u32 {\n    1\n}\n")?;
    let service = write(
        "app/service.py",
        "def compute(x):\n    return transform(x)\n",
    )?;
    assert_paths_step(&live, &root, "symbols deleted", &[util.clone(), service]).await?;

    // Yarıda kalan uygulama: vektörler değiştikten sonra, graf değişmeden önce
    // hata. Graf olduğu gibi kalmalıdır; yeni olay gelmeden yapılan sonraki tur
    // dosyaları diskten yeniden kurar. Silinen dosyanın önceki adı (`compute`)
    // belirsizliği kaldırır; çağıranın kenarı ancak bu ad hesaba katılırsa gelir.
    let before_failure = live_signature(&live).await;
    std::fs::remove_file(&other)?;
    write(
        "src/main.rs",
        "mod util;\n\nfn main() {\n    let total = assist();\n    run(total);\n    run(total);\n}\n\nfn run(value: u32) {\n    assist();\n}\n",
    )?;
    std::env::set_var("CCM_INTERNAL_LIVE_TEST_FAIL_BEFORE_GRAPH_SWAP", "1");
    let failed = live.apply_paths(&[other, main_rs.clone()]).await;
    std::env::remove_var("CCM_INTERNAL_LIVE_TEST_FAIL_BEFORE_GRAPH_SWAP");
    assert!(
        failed.is_err(),
        "the injected failure must surface: {failed:?}"
    );
    assert_eq!(
        live_signature(&live).await,
        before_failure,
        "a failed apply must leave the graph unchanged"
    );
    assert_paths_step(&live, &root, "retry after a failed apply", &[]).await?;

    // Dosya yeniden adlandırma: struct yeni dosyadan bağlanır.
    let types = root.join("src/types.rs");
    std::fs::rename(&model, &types)?;
    assert_paths_step(&live, &root, "file renamed", &[model, types]).await?;

    // Dizin silme: altındaki tüm dosyalar kalkar.
    std::fs::remove_dir_all(root.join("app"))?;
    assert_paths_step(&live, &root, "directory deleted", &[root.join("app")]).await?;

    // Tam karşılaştırma yolu.
    std::fs::write(&readme, "# Parity fixture\n\nUpdated.\n")?;
    assert!(matches!(
        live.apply_rescan().await?,
        LiveRefresh::Applied(_)
    ));
    let fresh = fresh_index_signature(&root).await?;
    assert_eq!(
        live_signature(&live).await,
        fresh,
        "rescan differs from a fresh full index"
    );

    // Kalıcılaştırılan graf da aynıdır ve `update_index` değişiklik görmez.
    assert_eq!(live.persist().await?, LivePersist::Persisted);
    let active = ccm_core::resolve_index_artifacts(&project_path, None)?;
    assert_eq!(active.generation_id.as_deref(), live.generation_id());
    let persisted = CodeGraph::from_file(&active.graph_path.to_string_lossy())?;
    assert_eq!(
        signature(&persisted),
        fresh,
        "persisted graph differs from a fresh full index"
    );
    let update = ccm_core::update_index(&project_path, None).await?;
    assert_eq!(
        update.files_indexed, 0,
        "update_index must see the persisted state as current"
    );
    assert_eq!(
        ccm_core::resolve_index_artifacts(&project_path, None)?,
        active,
        "a no-change update must not install a new generation"
    );
    Ok(())
}

/// İki crate'li Rust çalışma alanı (rust_references_test fikstürünün özü).
const RUST_WORKSPACE: &[(&str, &str)] = &[
    ("Cargo.toml", "[workspace]\nmembers = [\"app_core\", \"app_cli\"]\n"),
    (
        "app_core/Cargo.toml",
        "[package]\nname = \"app-core\"\nversion = \"0.1.0\"\n",
    ),
    (
        "app_core/src/lib.rs",
        "pub mod engine;\npub mod other;\npub mod util;\n\npub use engine::Engine;\n",
    ),
    (
        "app_core/src/util.rs",
        "pub fn helper() -> u32 {\n    1\n}\n\npub enum Mode {\n    Fast,\n    Slow,\n}\n",
    ),
    ("app_core/src/other.rs", "pub fn helper() -> u32 {\n    2\n}\n"),
    (
        "app_core/src/engine.rs",
        "use crate::util::helper;\n\npub struct Engine {}\n\nimpl Engine {\n    pub fn new() -> Self {\n        Engine {}\n    }\n\n    pub fn start(&self) -> u32 {\n        self.stop();\n        helper()\n    }\n\n    fn stop(&self) {}\n}\n",
    ),
    (
        "app_cli/Cargo.toml",
        "[package]\nname = \"app-cli\"\nversion = \"0.1.0\"\n",
    ),
    (
        "app_cli/src/main.rs",
        "use app_core::util::{self, Mode};\nuse app_core::Engine;\n\nfn main() {\n    let engine = Engine::new();\n    engine.start();\n    util::helper();\n    let _mode = Mode::Slow;\n}\n",
    ),
];

#[tokio::test]
async fn rust_incremental_edges_equal_a_full_rebuild() -> Result<()> {
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = tempdir()?;
    let root = std::fs::canonicalize(project.path())?;
    let write = |relative: &str, content: &str| -> Result<PathBuf> {
        let path = root.join(relative);
        std::fs::create_dir_all(path.parent().expect("parent directory"))?;
        std::fs::write(&path, content)?;
        Ok(path)
    };
    for (path, content) in RUST_WORKSPACE {
        write(path, content)?;
    }
    let project_path = root.to_string_lossy().to_string();
    ccm_core::index_directory(&project_path, None).await?;
    let live = LiveIndex::load(&project_path, None, None).await?;
    assert_eq!(
        live_signature(&live).await,
        fresh_index_signature(&root).await?
    );

    // Yeniden dışa aktarma değişir: `app_core::Engine` artık çözülmez.
    let lib = write(
        "app_core/src/lib.rs",
        "pub mod engine;\npub mod other;\npub mod util;\n\npub use other::helper as engine_helper;\n",
    )?;
    assert_paths_step(
        &live,
        &root,
        "re-export changed",
        std::slice::from_ref(&lib),
    )
    .await?;

    // Başka dosyadaki fonksiyon yeniden adlandırılır.
    let util = write(
        "app_core/src/util.rs",
        "pub fn assist() -> u32 {\n    1\n}\n\npub enum Mode {\n    Fast,\n    Slow,\n}\n",
    )?;
    assert_paths_step(
        &live,
        &root,
        "function renamed",
        std::slice::from_ref(&util),
    )
    .await?;

    // Paket adı değişir: `app_core::…` yolları dış crate olur.
    let manifest = write(
        "app_core/Cargo.toml",
        "[package]\nname = \"app-kernel\"\nversion = \"0.1.0\"\n",
    )?;
    assert_paths_step(
        &live,
        &root,
        "crate renamed",
        std::slice::from_ref(&manifest),
    )
    .await?;

    // Aynı crate'te ikinci `impl Engine` bloğu: `self.stop()` iki adaya çıkar.
    let other = write(
        "app_core/src/other.rs",
        "use crate::engine::Engine;\n\npub fn helper() -> u32 {\n    2\n}\n\nimpl Engine {\n    fn stop(&self) {}\n}\n",
    )?;
    assert_paths_step(
        &live,
        &root,
        "second impl block",
        std::slice::from_ref(&other),
    )
    .await?;
    Ok(())
}

/// Fikstürü yazar, indeksler ve canlı indeksi yükler.
async fn live_rust_project(
    files: &[(&str, &str)],
) -> Result<(tempfile::TempDir, PathBuf, LiveIndex)> {
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = tempdir()?;
    let root = std::fs::canonicalize(project.path())?;
    for (path, content) in files {
        let full = root.join(path);
        std::fs::create_dir_all(full.parent().expect("parent directory"))?;
        std::fs::write(&full, content)?;
    }
    let project_path = root.to_string_lossy().to_string();
    ccm_core::index_directory(&project_path, None).await?;
    let live = LiveIndex::load(&project_path, None, None).await?;
    Ok((project, root, live))
}

#[tokio::test]
async fn rust_const_and_static_edits_refresh_their_users() -> Result<()> {
    let (_project, root, live) = live_rust_project(&[
        ("src/lib.rs", "pub mod config;\npub mod user;\n"),
        ("src/config.rs", "pub fn other() {}\n"),
        (
            "src/user.rs",
            "use crate::config::MAX;\n\npub fn limit() -> usize {\n    crate::config::LIMIT + MAX\n}\n",
        ),
    ])
    .await?;
    let config = root.join("src/config.rs");
    std::fs::write(
        &config,
        "pub fn other() {}\n\npub const LIMIT: usize = 1;\npub static MAX: usize = 2;\n",
    )?;
    assert_paths_step(
        &live,
        &root,
        "const and static added",
        std::slice::from_ref(&config),
    )
    .await?;
    Ok(())
}

#[tokio::test]
async fn rust_module_declarations_move_files_between_crates() -> Result<()> {
    // `cli.rs` yalnız kütüphanedeki `helper`'ı anar: `mod cli;` kalkınca dosya
    // ikiliden kütüphaneye geçer ve çağrı ancak paket yeniden çözülürse bağlanır.
    let (_project, root, live) = live_rust_project(&[
        ("Cargo.toml", "[package]\nname = \"app\"\n"),
        ("src/lib.rs", "pub fn helper() -> u32 {\n    1\n}\n"),
        (
            "src/main.rs",
            "mod cli;\n\nfn main() {\n    cli::start();\n}\n",
        ),
        (
            "src/cli.rs",
            "pub fn start() -> u32 {\n    crate::helper()\n}\n",
        ),
    ])
    .await?;
    let main = root.join("src/main.rs");
    std::fs::write(&main, "fn main() {}\n")?;
    assert_paths_step(
        &live,
        &root,
        "mod declaration removed",
        std::slice::from_ref(&main),
    )
    .await?;
    std::fs::write(&main, "mod cli;\n\nfn main() {\n    cli::start();\n}\n")?;
    assert_paths_step(
        &live,
        &root,
        "mod declaration restored",
        std::slice::from_ref(&main),
    )
    .await?;
    Ok(())
}
