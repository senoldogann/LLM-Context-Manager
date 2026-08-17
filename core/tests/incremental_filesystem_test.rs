use anyhow::Result;
use ccm_core::graph::CodeGraph;
use ccm_core::{resolve_index_artifacts, IndexArtifactPaths};
use tempfile::tempdir;

static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn artifacts(project: &std::path::Path, db_path: Option<&str>) -> Result<IndexArtifactPaths> {
    resolve_index_artifacts(project.to_string_lossy().as_ref(), db_path)
}

#[tokio::test]
async fn update_index_only_applies_added_changed_and_deleted_files() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = tempdir()?;
    std::fs::write(project.path().join("untouched.rs"), "fn untouched() {}\n")?;
    std::fs::write(project.path().join("deleted.rs"), "fn deleted() {}\n")?;
    std::fs::write(project.path().join("changed.rs"), "fn before() {}\n")?;
    ccm_core::index_directory(project.path().to_string_lossy().as_ref(), None).await?;

    let initial_paths = artifacts(project.path(), None)?;
    let initial = CodeGraph::from_file(initial_paths.graph_path.to_string_lossy().as_ref())?;
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

    let updated_paths = artifacts(project.path(), None)?;
    let updated = CodeGraph::from_file(updated_paths.graph_path.to_string_lossy().as_ref())?;
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

#[tokio::test]
async fn full_reindex_persists_an_empty_graph_after_sources_are_removed() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = tempdir()?;
    let source_path = project.path().join("main.rs");

    std::fs::write(&source_path, "fn stale() {}\n")?;
    ccm_core::index_directory(project.path().to_string_lossy().as_ref(), None).await?;
    let initial_paths = artifacts(project.path(), None)?;
    let initial = CodeGraph::from_file(initial_paths.graph_path.to_string_lossy().as_ref())?;
    assert!(initial
        .graph
        .node_weights()
        .any(|node| node.name == "stale"));

    std::fs::remove_file(source_path)?;
    ccm_core::index_directory(project.path().to_string_lossy().as_ref(), None).await?;

    let empty_paths = artifacts(project.path(), None)?;
    let empty = CodeGraph::from_file(empty_paths.graph_path.to_string_lossy().as_ref())?;
    let remaining: Vec<String> = empty
        .graph
        .node_weights()
        .map(|node| node.name.clone())
        .collect();
    assert!(remaining.is_empty(), "remaining nodes: {remaining:?}");

    Ok(())
}

#[tokio::test]
async fn transient_invalid_content_preserves_previous_file_state_and_retries() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = tempdir()?;
    let source_path = project.path().join("main.rs");

    std::fs::write(&source_path, "fn alpha() {}\n")?;
    ccm_core::index_directory(project.path().to_string_lossy().as_ref(), None).await?;

    std::fs::write(&source_path, [0xff, 0xfe, 0xfd])?;
    let failed = ccm_core::update_index(project.path().to_string_lossy().as_ref(), None).await?;
    assert_eq!(failed.files_failed, 1);
    let preserved_paths = artifacts(project.path(), None)?;
    let preserved = CodeGraph::from_file(preserved_paths.graph_path.to_string_lossy().as_ref())?;
    assert!(preserved
        .graph
        .node_weights()
        .any(|node| node.name == "alpha"));

    std::fs::write(&source_path, "fn beta() {}\n")?;
    ccm_core::update_index(project.path().to_string_lossy().as_ref(), None).await?;
    let repaired_paths = artifacts(project.path(), None)?;
    let repaired = CodeGraph::from_file(repaired_paths.graph_path.to_string_lossy().as_ref())?;
    assert!(repaired
        .graph
        .node_weights()
        .any(|node| node.name == "beta"));
    assert!(!repaired
        .graph
        .node_weights()
        .any(|node| node.name == "alpha"));

    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn incomplete_snapshot_aborts_without_deleting_existing_nodes() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    use std::os::unix::fs::PermissionsExt;

    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = tempdir()?;
    let private_dir = project.path().join("private");
    std::fs::create_dir(&private_dir)?;
    std::fs::write(private_dir.join("hidden.rs"), "fn retained() {}\n")?;
    ccm_core::index_directory(project.path().to_string_lossy().as_ref(), None).await?;

    std::fs::set_permissions(&private_dir, std::fs::Permissions::from_mode(0o000))?;
    let update = ccm_core::update_index(project.path().to_string_lossy().as_ref(), None).await;
    std::fs::set_permissions(&private_dir, std::fs::Permissions::from_mode(0o700))?;
    assert!(update.is_err());

    let preserved_paths = artifacts(project.path(), None)?;
    let preserved = CodeGraph::from_file(preserved_paths.graph_path.to_string_lossy().as_ref())?;
    assert!(preserved
        .graph
        .node_weights()
        .any(|node| node.name == "retained"));

    Ok(())
}

#[tokio::test]
async fn failed_full_rebuild_keeps_the_active_generation() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    let project = tempdir()?;
    let source_path = project.path().join("main.rs");

    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    std::fs::write(&source_path, "fn alpha() {}\n")?;
    ccm_core::index_directory(project.path().to_string_lossy().as_ref(), None).await?;
    let initial_paths = artifacts(project.path(), None)?;
    let graph = CodeGraph::from_file(initial_paths.graph_path.to_string_lossy().as_ref())?;
    let alpha_id = graph
        .graph
        .node_weights()
        .find(|node| node.name == "alpha")
        .expect("alpha node")
        .id
        .clone();
    let namespace = project
        .path()
        .file_name()
        .expect("project directory name")
        .to_string_lossy();
    let fixture_path = project.path().join("fixture.ndjson");
    std::fs::write(
        &fixture_path,
        format!(
            "{{\"kind\":\"meta\",\"dim\":2}}\n{{\"kind\":\"doc\",\"ns\":\"{}\",\"id\":\"{}\",\"vector\":[1.0,0.0]}}\n",
            namespace, alpha_id
        ),
    )?;

    std::env::remove_var("CCM_DISABLE_EMBEDDER");
    std::env::set_var("CCM_EMBEDDING_FIXTURE", &fixture_path);
    ccm_core::index_directory(project.path().to_string_lossy().as_ref(), None).await?;
    let vector_paths = artifacts(project.path(), None)?;
    assert!(vector_paths.db_path.join("code_vectors.lance").exists());

    std::fs::write(&source_path, "fn bravo() {}\n")?;
    let failed = ccm_core::index_directory(project.path().to_string_lossy().as_ref(), None).await;
    std::env::remove_var("CCM_EMBEDDING_FIXTURE");
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    assert!(failed.is_err());

    let preserved_paths = artifacts(project.path(), None)?;
    let preserved = CodeGraph::from_file(preserved_paths.graph_path.to_string_lossy().as_ref())?;
    assert!(preserved
        .graph
        .node_weights()
        .any(|node| node.name == "alpha"));
    assert!(!preserved
        .graph
        .node_weights()
        .any(|node| node.name == "bravo"));
    assert!(preserved_paths.db_path.join("code_vectors.lance").exists());

    Ok(())
}

#[tokio::test]
async fn custom_database_inside_project_is_never_indexed_as_source() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = tempdir()?;
    let custom_db = project.path().join(".ccm/db");
    std::fs::write(project.path().join("main.rs"), "fn business_code() {}\n")?;

    for _ in 0..2 {
        ccm_core::index_directory(
            project.path().to_string_lossy().as_ref(),
            Some(custom_db.to_string_lossy().as_ref()),
        )
        .await?;
    }

    let custom_db_str = custom_db.to_string_lossy().to_string();
    let active_paths = artifacts(project.path(), Some(&custom_db_str))?;
    let graph = CodeGraph::from_file(active_paths.graph_path.to_string_lossy().as_ref())?;
    assert!(graph
        .graph
        .node_weights()
        .any(|node| node.name == "business_code"));
    assert!(!graph
        .graph
        .node_weights()
        .any(|node| { node.id.contains("/.ccm/") || node.id.starts_with("./.ccm/") }));

    Ok(())
}

#[tokio::test]
async fn corrupt_graph_is_rebuilt_instead_of_becoming_an_empty_index() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = tempdir()?;
    std::fs::write(project.path().join("main.rs"), "fn recovered() {}\n")?;
    ccm_core::index_directory(project.path().to_string_lossy().as_ref(), None).await?;

    let corrupt_paths = artifacts(project.path(), None)?;
    std::fs::write(&corrupt_paths.graph_path, "{broken")?;
    ccm_core::update_index(project.path().to_string_lossy().as_ref(), None).await?;

    let repaired_paths = artifacts(project.path(), None)?;
    let graph = CodeGraph::from_file(repaired_paths.graph_path.to_string_lossy().as_ref())?;
    assert!(graph
        .graph
        .node_weights()
        .any(|node| node.name == "recovered"));

    Ok(())
}

#[tokio::test]
async fn incremental_embedding_failure_keeps_active_generation_unchanged() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    let project = tempdir()?;
    let fixture_dir = tempdir()?;
    let source_path = project.path().join("main.rs");

    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    std::fs::write(&source_path, "fn alpha() {}\n")?;
    ccm_core::index_directory(project.path().to_string_lossy().as_ref(), None).await?;
    let initial = artifacts(project.path(), None)?;
    let graph = CodeGraph::from_file(initial.graph_path.to_string_lossy().as_ref())?;
    let alpha_id = graph
        .graph
        .node_weights()
        .find(|node| node.name == "alpha")
        .expect("alpha node")
        .id
        .clone();
    let namespace = project
        .path()
        .file_name()
        .expect("project directory name")
        .to_string_lossy();
    let fixture_path = fixture_dir.path().join("fixture.ndjson");
    std::fs::write(
        &fixture_path,
        format!(
            "{{\"kind\":\"meta\",\"dim\":2}}\n{{\"kind\":\"doc\",\"ns\":\"{}\",\"id\":\"{}\",\"vector\":[1.0,0.0]}}\n",
            namespace, alpha_id
        ),
    )?;
    std::env::remove_var("CCM_DISABLE_EMBEDDER");
    std::env::set_var("CCM_EMBEDDING_FIXTURE", &fixture_path);
    ccm_core::index_directory(project.path().to_string_lossy().as_ref(), None).await?;
    let active_before = artifacts(project.path(), None)?;

    std::fs::write(&source_path, "fn bravo() {}\n")?;
    let failed = ccm_core::update_index(project.path().to_string_lossy().as_ref(), None).await;
    assert!(failed.is_err());
    let active_after = artifacts(project.path(), None)?;
    assert_eq!(active_before, active_after);
    let preserved = CodeGraph::from_file(active_after.graph_path.to_string_lossy().as_ref())?;
    assert!(preserved
        .graph
        .node_weights()
        .any(|node| node.name == "alpha"));
    assert!(active_after.db_path.join("code_vectors.lance").exists());

    std::env::remove_var("CCM_EMBEDDING_FIXTURE");
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    Ok(())
}

#[tokio::test]
async fn relative_custom_database_is_project_relative_and_excluded() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = tempdir()?;
    std::fs::write(project.path().join("main.rs"), "fn business() {}\n")?;

    ccm_core::index_directory(project.path().to_string_lossy().as_ref(), Some(".ccm/db")).await?;
    let active = artifacts(project.path(), Some(".ccm/db"))?;
    assert!(active.db_path.starts_with(project.path().canonicalize()?));
    let graph = CodeGraph::from_file(active.graph_path.to_string_lossy().as_ref())?;
    assert!(graph
        .graph
        .node_weights()
        .any(|node| node.name == "business"));
    assert!(!graph
        .graph
        .node_weights()
        .any(|node| node.id.contains(".ccm-generations")));
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn symlinked_data_dir_never_writes_outside_the_project() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let outside = tempdir()?;
    let project = tempdir()?;
    std::fs::write(project.path().join("main.rs"), "fn business() {}\n")?;
    std::fs::remove_dir_all(project.path().join("data")).ok();
    std::os::unix::fs::symlink(outside.path(), project.path().join("data"))?;

    let failed = ccm_core::index_directory(project.path().to_string_lossy().as_ref(), None).await;
    assert!(
        failed.is_err(),
        "data symlink'i kök dışına yazan index kabul edilmemeli"
    );
    let leaked: Vec<_> = std::fs::read_dir(outside.path())?
        .filter_map(Result::ok)
        .map(|entry| entry.file_name())
        .collect();
    assert!(
        leaked.is_empty(),
        "kök dışına artifact yazıldı: {:?}",
        leaked
    );
    Ok(())
}

#[tokio::test]
async fn orphan_artifact_directories_are_never_indexed() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = tempdir()?;
    let orphan = project.path().join("data/.ccm-rebuild-orphan");
    std::fs::create_dir_all(&orphan)?;
    std::fs::write(orphan.join("leak.rs"), "fn leaked_staging() {}\n")?;
    std::fs::write(project.path().join("main.rs"), "fn business() {}\n")?;

    ccm_core::index_directory(project.path().to_string_lossy().as_ref(), None).await?;
    let active = artifacts(project.path(), None)?;
    let graph = CodeGraph::from_file(active.graph_path.to_string_lossy().as_ref())?;
    assert!(!graph
        .graph
        .node_weights()
        .any(|node| node.name == "leaked_staging"));
    Ok(())
}

#[tokio::test]
async fn full_rebuild_invalid_supported_source_preserves_active_generation() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = tempdir()?;
    let source_path = project.path().join("main.rs");
    std::fs::write(&source_path, "fn alpha() {}\n")?;
    ccm_core::index_directory(project.path().to_string_lossy().as_ref(), None).await?;
    let active_before = artifacts(project.path(), None)?;

    std::fs::write(&source_path, [0xff, 0xfe, 0xfd])?;
    let failed = ccm_core::index_directory(project.path().to_string_lossy().as_ref(), None).await;
    assert!(failed.is_err());
    let active_after = artifacts(project.path(), None)?;
    assert_eq!(active_before, active_after);
    let graph = CodeGraph::from_file(active_after.graph_path.to_string_lossy().as_ref())?;
    assert!(graph.graph.node_weights().any(|node| node.name == "alpha"));
    Ok(())
}

#[tokio::test]
async fn full_rebuild_skips_oversized_supported_file_instead_of_aborting() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = tempdir()?;
    std::fs::write(project.path().join("main.rs"), "fn alpha() {}\n")?;
    // 2MB üstü tek bir kaynak dosyası full index'i TÜMÜYLE iptal etmemeli;
    // deterministik TooLarge dosyaları atlanıp uyarı kaydedilir.
    let oversized = project.path().join("generated_bundle.rs");
    let mut content = String::from("// generated bundle\n");
    content.push_str(&"// padding line\n".repeat(140_000));
    assert!(content.len() > 2 * 1024 * 1024);
    std::fs::write(&oversized, content)?;

    let stats = ccm_core::index_directory(project.path().to_string_lossy().as_ref(), None).await?;
    assert_eq!(
        stats.files_indexed, 1,
        "oversized dosya atlanmalı, main.rs indexlenmeli"
    );
    assert_eq!(stats.files_failed, 1);
    assert!(
        stats
            .failed_files
            .iter()
            .any(|issue| issue.path.contains("generated_bundle.rs")),
        "TooLarge dosyası uyarı olarak kaydedilmeli"
    );

    let artifacts = artifacts(project.path(), None)?;
    let graph = CodeGraph::from_file(artifacts.graph_path.to_string_lossy().as_ref())?;
    assert!(
        graph.graph.node_weights().any(|node| node.name == "alpha"),
        "main.rs node'ları index'te olmalı"
    );
    Ok(())
}

#[tokio::test]
async fn empty_semantic_corpus_is_stable_without_a_vector_table() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    let project = tempdir()?;
    let fixture_dir = tempdir()?;
    let fixture_path = fixture_dir.path().join("empty.ndjson");
    std::fs::write(&fixture_path, "{\"kind\":\"meta\",\"dim\":2}\n")?;
    std::env::remove_var("CCM_DISABLE_EMBEDDER");
    std::env::set_var("CCM_EMBEDDING_FIXTURE", &fixture_path);

    ccm_core::index_directory(project.path().to_string_lossy().as_ref(), None).await?;
    let first = artifacts(project.path(), None)?;
    assert!(!first.db_path.join("code_vectors.lance").exists());
    ccm_core::update_index(project.path().to_string_lossy().as_ref(), None).await?;
    let second = artifacts(project.path(), None)?;
    assert_eq!(first, second);

    std::env::remove_var("CCM_EMBEDDING_FIXTURE");
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    Ok(())
}

#[tokio::test]
async fn immutable_generations_keep_only_current_and_previous() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = tempdir()?;
    let source_path = project.path().join("main.rs");
    let mut previous_path: Option<std::path::PathBuf> = None;

    for version in 0..4 {
        std::fs::write(&source_path, format!("fn version_{version}() {{}}\n"))?;
        ccm_core::index_directory(project.path().to_string_lossy().as_ref(), None).await?;
        let active = artifacts(project.path(), None)?;
        assert!(active.generation_id.is_some());
        if let Some(previous) = previous_path.take() {
            assert!(previous.exists());
        }
        previous_path = Some(
            active
                .graph_path
                .parent()
                .expect("generation root")
                .to_path_buf(),
        );
    }

    let generations = std::fs::read_dir(project.path().join("data/.ccm-generations"))?
        .filter_map(std::result::Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter(|entry| !entry.file_name().to_string_lossy().ends_with(".staging"))
        .count();
    assert_eq!(generations, 2);
    Ok(())
}

#[tokio::test]
async fn legacy_flat_index_migrates_on_first_incremental_mutation() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = tempdir()?;
    let source_path = project.path().join("main.rs");
    std::fs::write(&source_path, "fn alpha() {}\n")?;
    ccm_core::index_directory(project.path().to_string_lossy().as_ref(), None).await?;
    let generated = artifacts(project.path(), None)?;

    let data = project.path().join("data");
    copy_directory_for_test(&generated.db_path, &data.join("ccm_db"))?;
    std::fs::copy(&generated.graph_path, data.join("ccm_graph.json"))?;
    std::fs::copy(&generated.manifest_path, data.join("ccm_manifest.json"))?;
    std::fs::remove_file(data.join("ccm_current"))?;
    std::fs::remove_dir_all(data.join(".ccm-generations"))?;
    let legacy = artifacts(project.path(), None)?;
    assert!(legacy.generation_id.is_none());

    std::fs::write(&source_path, "fn beta() {}\n")?;
    ccm_core::update_index(project.path().to_string_lossy().as_ref(), None).await?;
    let migrated = artifacts(project.path(), None)?;
    assert!(migrated.generation_id.is_some());
    let graph = CodeGraph::from_file(migrated.graph_path.to_string_lossy().as_ref())?;
    assert!(graph.graph.node_weights().any(|node| node.name == "beta"));
    Ok(())
}

fn copy_directory_for_test(source: &std::path::Path, destination: &std::path::Path) -> Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_directory_for_test(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}
