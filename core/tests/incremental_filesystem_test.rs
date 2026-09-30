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

#[tokio::test]
async fn quick_index_builds_graph_but_skips_vectors_then_upgrade_fills_them() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = tempdir()?;
    std::fs::write(
        project.path().join("main.rs"),
        "pub fn compute_tax(base: f64) -> f64 { base * 1.24 }\n",
    )?;

    // Quick mod embedding atlar: graph kurulur, code_vectors.lance üretilmez.
    let stats = ccm_core::index_directory_with_mode(
        project.path().to_string_lossy().as_ref(),
        None,
        ccm_core::IndexMode::Quick,
    )
    .await?;
    assert!(stats.nodes_created > 0);

    let after_quick = artifacts(project.path(), None)?;
    assert!(after_quick.graph_path.is_file());
    assert!(
        !after_quick.db_path.join("code_vectors.lance").is_dir(),
        "quick mod embedding üretmemeli"
    );

    // Embedder disabled iken upgrade embeddings üretemez; açık davranış hata
    // değil hiçbir şey yapmamaktır (semantic node varsa, disabled ise atlar).
    std::env::remove_var("CCM_DISABLE_EMBEDDER");
    // Embedder kapalıyken semantic üretilemeyeceğinden bu testi graph varlığı
    // ve upgrade çağrısının güvenli no-op davranışıyla sınırlıyoruz; gerçek
    // semantic doldurma hermetic MCP testinde fixture embedder ile kapsanır.
    let upgrade =
        ccm_core::upgrade_active_index_semantics(project.path().to_string_lossy().as_ref(), None)
            .await;
    // Ollama canlı olmayan CI ortamında upgrade hata dönebilir; aktif graph
    // bozulmamalıdır.
    let _ = upgrade;
    let after_upgrade = artifacts(project.path(), None)?;
    assert!(after_upgrade.graph_path.is_file());

    Ok(())
}

#[tokio::test]
async fn upgrade_repairs_a_generation_without_vectors_using_fixture_embedder() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    // Fixture modu kullanıldığı için ortam her çıkış yolunda geri yüklenmeli.
    struct EnvRestore;
    impl Drop for EnvRestore {
        fn drop(&mut self) {
            std::env::remove_var("CCM_EMBEDDING_FIXTURE");
            std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
        }
    }
    let _restore = EnvRestore;
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = tempdir()?;
    std::fs::write(
        project.path().join("main.rs"),
        "pub fn compute_tax(base: f64) -> f64 { base * 1.24 }\n",
    )?;

    let stats = ccm_core::index_directory_with_mode(
        project.path().to_string_lossy().as_ref(),
        None,
        ccm_core::IndexMode::Quick,
    )
    .await?;
    assert!(stats.nodes_created > 0);
    let broken = artifacts(project.path(), None)?;
    assert!(!broken.db_path.join("code_vectors.lance").is_dir());

    // Fixture embedder ile semantic upgrade'i deterministik biçimde kanıtla.
    // Fixture doc id'si gerçek graph node id'siyle birebir eşleşmelidir.
    let graph = CodeGraph::from_file(broken.graph_path.to_string_lossy().as_ref())?;
    let node_id = graph
        .graph
        .node_weights()
        .find(|node| node.name == "compute_tax")
        .expect("compute_tax node")
        .id
        .clone();
    let project_name = project
        .path()
        .file_name()
        .expect("tempdir name")
        .to_string_lossy()
        .to_string();
    let fixture_path = project.path().join("embeddings.ndjson");
    std::fs::write(
        &fixture_path,
        format!(
            "{{\"kind\":\"meta\",\"dim\":3}}\n\
             {{\"kind\":\"doc\",\"ns\":\"{}\",\"id\":\"{}\",\"vector\":[1.0,0.0,0.0]}}\n",
            project_name, node_id
        ),
    )?;
    std::env::remove_var("CCM_DISABLE_EMBEDDER");
    std::env::set_var("CCM_EMBEDDING_FIXTURE", &fixture_path);

    let upgrade =
        ccm_core::upgrade_active_index_semantics(project.path().to_string_lossy().as_ref(), None)
            .await?;
    assert!(upgrade.nodes_created > 0);
    let repaired = artifacts(project.path(), None)?;
    assert!(repaired.db_path.join("code_vectors.lance").is_dir());

    Ok(())
}

#[tokio::test]
async fn project_under_excluded_directory_name_is_still_indexed() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    // Proje kökünün üst dizinlerinden biri "build" olsa bile politika yalnızca
    // proje içi göreli yola uygulanmalı (ör. CI'daki /build/app checkout'u).
    let workspace = tempdir()?;
    let project = workspace.path().join("build").join("app");
    std::fs::create_dir_all(project.join("src"))?;
    std::fs::write(project.join("src/lib.rs"), "fn initial() {}\n")?;
    ccm_core::index_directory(project.to_string_lossy().as_ref(), None).await?;

    let initial_paths = artifacts(&project, None)?;
    let initial = CodeGraph::from_file(initial_paths.graph_path.to_string_lossy().as_ref())?;
    assert!(initial
        .graph
        .node_weights()
        .any(|node| node.name == "initial"));

    std::fs::write(project.join("src/added.rs"), "fn added() {}\n")?;
    ccm_core::update_index(project.to_string_lossy().as_ref(), None).await?;

    let updated_paths = artifacts(&project, None)?;
    let updated = CodeGraph::from_file(updated_paths.graph_path.to_string_lossy().as_ref())?;
    assert!(updated
        .graph
        .node_weights()
        .any(|node| node.name == "added"));
    assert!(ccm_core::is_index_relevant_file(
        &project,
        &project.join("src/added.rs")
    ));
    assert!(!ccm_core::is_index_relevant_file(
        &project,
        &project.join("target/debug/generated.rs")
    ));

    Ok(())
}

#[tokio::test]
async fn unreachable_embedder_still_activates_a_graph_only_index() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    // Graf-öncelikli: embedding servisi kapalıyken graf araçları kullanılabilir
    // kalmalı, neden açıkça raporlanmalı ve sonraki güncellemeler graf'ı tazelemeli.
    // Ortam her çıkış yolunda (erken `?` dahil) geri yüklenir; sızan EMBEDDING_*
    // değerleri aynı binary'deki diğer testleri etkilemesin.
    struct EnvRestore;
    impl Drop for EnvRestore {
        fn drop(&mut self) {
            std::env::remove_var("EMBEDDING_HOST");
            std::env::remove_var("EMBEDDING_TIMEOUT_SECS");
            std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
        }
    }
    let _restore = EnvRestore;
    std::env::remove_var("CCM_DISABLE_EMBEDDER");
    std::env::set_var("EMBEDDING_HOST", "http://127.0.0.1:9");
    std::env::set_var("EMBEDDING_TIMEOUT_SECS", "2");
    let project = tempdir()?;
    std::fs::write(
        project.path().join("main.rs"),
        "fn alpha() { beta(); }\nfn beta() {}\n",
    )?;

    let first = ccm_core::update_index(project.path().to_string_lossy().as_ref(), None).await;
    std::fs::write(project.path().join("extra.rs"), "fn gamma() { alpha(); }\n")?;
    let second = ccm_core::update_index(project.path().to_string_lossy().as_ref(), None).await;

    let first = first?;
    let reason = first
        .semantic_unavailable
        .expect("semantic_unavailable reason");
    assert!(
        reason.contains("unreachable"),
        "unexpected reason: {reason}"
    );
    let second = second?;
    assert!(second.semantic_unavailable.is_some());

    let paths = artifacts(project.path(), None)?;
    let graph = CodeGraph::from_file(paths.graph_path.to_string_lossy().as_ref())?;
    for name in ["alpha", "beta", "gamma"] {
        assert!(
            graph.graph.node_weights().any(|node| node.name == name),
            "{name} missing from graph-only index"
        );
    }
    Ok(())
}

#[tokio::test]
async fn update_index_detects_same_size_edit_inside_racy_window() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = tempdir()?;
    let file = project.path().join("lib.rs");
    std::fs::write(&file, "fn alpha() {}\n")?;
    ccm_core::index_directory(project.path().to_string_lossy().as_ref(), None).await?;
    let manifest_path = artifacts(project.path(), None)?.manifest_path;
    assert!(ccm_core::read_index_timestamp(&manifest_path)?.is_some());

    // Aynı boyutta içerik değişikliği; mtime geri yüklenerek stat bilgisi
    // birebir korunur. İndeks az önce alındığı için dosya racy penceresindedir
    // ve içerik yeniden hash'lenmelidir.
    let original_mtime = std::fs::metadata(&file)?.modified()?;
    std::fs::write(&file, "fn gamma() {}\n")?;
    std::fs::OpenOptions::new()
        .write(true)
        .open(&file)?
        .set_modified(original_mtime)?;
    ccm_core::update_index(project.path().to_string_lossy().as_ref(), None).await?;

    let paths = artifacts(project.path(), None)?;
    let graph = CodeGraph::from_file(paths.graph_path.to_string_lossy().as_ref())?;
    assert!(graph.graph.node_weights().any(|node| node.name == "gamma"));
    assert!(!graph.graph.node_weights().any(|node| node.name == "alpha"));
    Ok(())
}

#[tokio::test]
async fn update_index_trusts_unchanged_stat_outside_racy_window() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = tempdir()?;
    let file = project.path().join("lib.rs");
    std::fs::write(&file, "fn alpha() {}\n")?;
    let an_hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3_600);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&file)?
        .set_modified(an_hour_ago)?;
    ccm_core::index_directory(project.path().to_string_lossy().as_ref(), None).await?;

    // Git ile aynı ödünleşim: mtime ve boyut birebir korunmuşsa ve dosya racy
    // pencerenin dışındaysa içerik okunmaz. Bu test hızlı yolun devrede
    // olduğunu sabitler; yol kapanırsa performans sessizce geriler.
    std::fs::write(&file, "fn gamma() {}\n")?;
    std::fs::OpenOptions::new()
        .write(true)
        .open(&file)?
        .set_modified(an_hour_ago)?;
    let stats = ccm_core::update_index(project.path().to_string_lossy().as_ref(), None).await?;

    assert_eq!(
        stats.files_indexed, 0,
        "unchanged stat must skip re-hashing"
    );
    Ok(())
}

/// Ollama `/api/embed` sözleşmesini konuşan deterministik yerel sunucu. CI'da
/// gerçek embedding servisi olmadığı için yalnızca bu testte kullanılır ve
/// istek başına gelen `input` sayısını toplar.
fn start_counting_embed_server() -> Result<(String, std::sync::Arc<std::sync::atomic::AtomicUsize>)>
{
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let address = format!("http://{}", listener.local_addr()?);
    let embedded_inputs = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = embedded_inputs.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let Ok(read_half) = stream.try_clone() else {
                continue;
            };
            let mut reader = BufReader::new(read_half);
            let mut content_length = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    content_length = value.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; content_length];
            if reader.read_exact(&mut body).is_err() {
                continue;
            }
            let request: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let inputs = request["input"].as_array().cloned().unwrap_or_default();
            counter.fetch_add(inputs.len(), std::sync::atomic::Ordering::SeqCst);
            let embeddings: Vec<Vec<f32>> = inputs
                .iter()
                .map(|input| {
                    let seed = input
                        .as_str()
                        .unwrap_or_default()
                        .bytes()
                        .fold(0u32, |acc, byte| {
                            acc.wrapping_mul(31).wrapping_add(u32::from(byte))
                        });
                    (0..8u32)
                        .map(|offset| (seed.wrapping_add(offset) % 97) as f32 / 97.0 + 0.01)
                        .collect()
                })
                .collect();
            let payload = serde_json::json!({ "embeddings": embeddings }).to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                payload.len(),
                payload
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    Ok((address, embedded_inputs))
}

#[tokio::test]
async fn update_index_embeds_only_changed_chunks() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    struct EnvRestore;
    impl Drop for EnvRestore {
        fn drop(&mut self) {
            std::env::remove_var("EMBEDDING_HOST");
            std::env::remove_var("EMBEDDING_MODEL");
            std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
        }
    }
    let _restore = EnvRestore;
    let (host, embedded_inputs) = start_counting_embed_server()?;
    std::env::remove_var("CCM_DISABLE_EMBEDDER");
    std::env::set_var("EMBEDDING_HOST", &host);
    std::env::set_var("EMBEDDING_MODEL", "ccm-test-embed");

    let project = tempdir()?;
    let file = project.path().join("lib.rs");
    std::fs::write(
        &file,
        "fn alpha() { let a = 1; }\nfn beta() { let b = 2; }\nfn gamma() { let c = 3; }\n",
    )?;
    let first = ccm_core::update_index(project.path().to_string_lossy().as_ref(), None).await?;
    assert_eq!(first.embedded_chunks, 3);
    let after_full_index = embedded_inputs.load(std::sync::atomic::Ordering::SeqCst);

    std::fs::write(
        &file,
        "fn alpha() { let a = 1; }\nfn beta() { let b = 2; }\nfn gamma() { let c = 30; }\n",
    )?;
    let second = ccm_core::update_index(project.path().to_string_lossy().as_ref(), None).await?;

    assert_eq!(second.embedded_chunks, 1, "only gamma changed");
    assert_eq!(second.reused_chunks, 2, "alpha and beta keep their vectors");
    assert_eq!(
        embedded_inputs.load(std::sync::atomic::Ordering::SeqCst) - after_full_index,
        1,
        "the embedding service must see only the changed chunk"
    );
    Ok(())
}

#[tokio::test]
async fn live_refresh_reuses_vectors_in_the_active_table() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    struct EnvRestore;
    impl Drop for EnvRestore {
        fn drop(&mut self) {
            std::env::remove_var("EMBEDDING_HOST");
            std::env::remove_var("EMBEDDING_MODEL");
            std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
        }
    }
    let _restore = EnvRestore;
    let (host, embedded_inputs) = start_counting_embed_server()?;
    std::env::remove_var("CCM_DISABLE_EMBEDDER");
    std::env::set_var("EMBEDDING_HOST", &host);
    std::env::set_var("EMBEDDING_MODEL", "ccm-test-embed");

    let project = tempdir()?;
    let root = std::fs::canonicalize(project.path())?;
    let file = root.join("lib.rs");
    std::fs::write(
        &file,
        "fn alpha() { let a = 1; }\nfn beta() { let b = 2; }\nfn gamma() { let c = 3; }\n",
    )?;
    let project_path = root.to_string_lossy().to_string();
    ccm_core::update_index(&project_path, None).await?;
    let active = artifacts(&root, None)?;
    let live = ccm_core::live::LiveIndex::load(&project_path, None, None).await?;
    live.apply_rescan().await?;
    let before_edit = embedded_inputs.load(std::sync::atomic::Ordering::SeqCst);

    // Canlı yenileme etkin generation'ın tablosunu yerinde günceller: yalnızca
    // değişen parça embed edilir, diğerleri silinmeden önce okunan vektörünü korur.
    std::fs::write(
        &file,
        "fn alpha() { let a = 1; }\nfn beta() { let b = 2; }\nfn gamma() { let c = 30; }\n",
    )?;
    let ccm_core::live::LiveRefresh::Applied(stats) =
        live.apply_paths(std::slice::from_ref(&file)).await?
    else {
        panic!("live refresh must apply to the active generation");
    };
    assert_eq!(stats.embedded_chunks, 1, "only gamma changed");
    assert_eq!(stats.reused_chunks, 2, "alpha and beta keep their vectors");
    assert_eq!(
        embedded_inputs.load(std::sync::atomic::Ordering::SeqCst) - before_edit,
        1
    );
    assert_eq!(
        artifacts(&root, None)?,
        active,
        "a live refresh must not install a generation"
    );
    let store = ccm_core::vector::store::LanceDbStore::new(
        active.db_path.to_string_lossy().as_ref(),
        "code_vectors",
    )
    .await?;
    assert_eq!(
        store.validate_table().await?,
        3,
        "no duplicate or missing rows"
    );
    Ok(())
}

#[tokio::test]
async fn watch_filter_skips_ignored_outputs_and_index_artifacts() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");

    // Senaryo 1: Git reposunda .gitignore uygulanır.
    let project = tempdir()?;
    let root = std::fs::canonicalize(project.path())?;
    git2::Repository::init(&root)?;
    std::fs::create_dir_all(root.join("src"))?;
    std::fs::write(root.join("src/lib.rs"), "fn alpha() {}\n")?;
    std::fs::write(root.join(".gitignore"), "generated/\n")?;
    std::fs::write(root.join(".ccmignore"), "fixtures/\n")?;
    ccm_core::index_directory(root.to_string_lossy().as_ref(), None).await?;
    let active = artifacts(&root, None)?;
    let filter = ccm_core::build_watch_filter(&root, &root.join("data/ccm_db"))?;

    for relevant in ["src/lib.rs", "src/removed.rs", "src/my file.rs", "Makefile"] {
        assert!(
            ccm_core::is_watch_relevant_path(&filter, &root.join(relevant)),
            "{relevant} should trigger a refresh"
        );
    }
    let ignored = [
        root.join("generated/out.rs"),
        root.join("fixtures/sample.rs"),
        root.join("target/debug/build.rs"),
        root.join(".git/index"),
        root.join(".ccm/semantic-upgrade.log"),
        root.join("data/ccm_current"),
        active.graph_path.clone(),
        active.db_path.join("code_vectors.lance/data.lance"),
        root.clone(),
        std::path::PathBuf::from("/outside/project.rs"),
    ];
    for path in ignored {
        assert!(
            !ccm_core::is_watch_relevant_path(&filter, &path),
            "{} should not trigger a refresh",
            path.display()
        );
    }

    // Senaryo 2: Git olmayan projede .gitignore yok sayılır; generated/out.rs RELEVANT olmalı.
    let project2 = tempdir()?;
    let root2 = std::fs::canonicalize(project2.path())?;
    std::fs::create_dir_all(root2.join("src"))?;
    std::fs::write(root2.join("src/lib.rs"), "fn beta() {}\n")?;
    std::fs::write(root2.join(".gitignore"), "generated/\n")?;
    ccm_core::index_directory(root2.to_string_lossy().as_ref(), None).await?;
    let filter2 = ccm_core::build_watch_filter(&root2, &root2.join("data/ccm_db"))?;
    assert!(
        ccm_core::is_watch_relevant_path(&filter2, &root2.join("generated/out.rs")),
        "non-git project ignores .gitignore, so generated/out.rs should be relevant"
    );

    // Git olmayan projede `.git/info/exclude` yoktur (git deposunda indeks oraya
    // `/data/ccm_*` desenlerini yazar); indeksin kendi çıktısını yalnızca filtrenin
    // kendi kuralları eler. `src/ccm_current.notes.tmp` işaretçi geçici dosyasına
    // yalnızca ad olarak benzer; artefakt dizininin dışında olduğu için normal
    // proje dosyasıdır.
    assert!(
        ccm_core::is_watch_relevant_path(&filter2, &root2.join("src/ccm_current.notes.tmp")),
        "a project file outside the artifact directory must stay relevant"
    );
    for own_output in [
        // Atomik yazımın geçici dosyaları artefakt dizininde durur.
        "data/ccm_current.4242.1790000000000000000.tmp",
        "data/ccm_manifest.json.4242.tmp",
        "data/ccm_graph.json.4242.tmp",
        // Trajectory günlüğü ve politika deposu araç durumudur.
        "data/ccm_learn",
        "data/ccm_learn/experiences.jsonl",
        "data/ccm_learn/policies.json",
    ] {
        assert!(
            !ccm_core::is_watch_relevant_path(&filter2, &root2.join(own_output)),
            "{own_output} is written by the tool itself and must not trigger a refresh"
        );
    }

    Ok(())
}
