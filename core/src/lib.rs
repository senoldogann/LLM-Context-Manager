pub mod engine;

pub mod eval;
pub mod fixtures;
mod fs_utils;
pub mod git;
pub mod graph;
pub mod hash;

pub mod parser;

pub mod optimize;
pub mod policy;
pub mod rng;
pub mod trajectory;
pub mod vector;

use crate::engine::{CursorPosition, RetrievalEngine};
use crate::fs_utils::{detect_language, read_text_file_limited, FileReadError};
use crate::graph::CodeGraph;
use crate::parser::{CodeParser, SupportedLanguage};
use crate::vector::store::LanceDbStore;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub const INDEX_SCHEMA_VERSION: u32 = 4;
const GENERATIONS_DIRECTORY: &str = ".ccm-generations";
const CURRENT_GENERATION_FILE: &str = "ccm_current";
const ACTIVATION_LOCK_DIRECTORY: &str = ".ccm-activation.lock";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexArtifactPaths {
    pub db_path: PathBuf,
    pub graph_path: PathBuf,
    pub manifest_path: PathBuf,
    pub generation_id: Option<String>,
}

/// Etkin indeks generation'ının tutarlı artifact yollarını çözer.
/// Pointer bulunmayan eski kurulumlar düz yerleşimden okunmaya devam eder.
pub fn resolve_index_artifacts(path: &str, db_path: Option<&str>) -> Result<IndexArtifactPaths> {
    let project_root = std::fs::canonicalize(path).map_err(|error| {
        anyhow::anyhow!("Project root '{}' could not be resolved: {}", path, error)
    })?;
    let requested_db = resolve_requested_db_path(&project_root, db_path);
    let artifact_parent = requested_db
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Invalid DB path '{}': cannot determine parent directory",
                requested_db.display()
            )
        })?;
    let pointer_path = artifact_parent.join(CURRENT_GENERATION_FILE);

    if pointer_path.exists() {
        let generation_id = std::fs::read_to_string(&pointer_path)
            .map_err(|error| {
                anyhow::anyhow!(
                    "Active index pointer '{}' could not be read: {}",
                    pointer_path.display(),
                    error
                )
            })?
            .trim()
            .to_string();
        validate_generation_id(&generation_id)?;
        let generation_root = artifact_parent
            .join(GENERATIONS_DIRECTORY)
            .join(&generation_id);
        if !generation_root.is_dir() {
            anyhow::bail!(
                "Active index generation '{}' is missing at '{}'",
                generation_id,
                generation_root.display()
            );
        }
        return Ok(IndexArtifactPaths {
            db_path: generation_root.join("ccm_db"),
            graph_path: generation_root.join("ccm_graph.json"),
            manifest_path: generation_root.join("ccm_manifest.json"),
            generation_id: Some(generation_id),
        });
    }

    Ok(IndexArtifactPaths {
        db_path: requested_db,
        graph_path: artifact_parent.join("ccm_graph.json"),
        manifest_path: artifact_parent.join("ccm_manifest.json"),
        generation_id: None,
    })
}

pub fn init() {
    tracing::info!("CCM Core Initialized");
}

// Re-export ContextSuggestion for external use
pub use crate::engine::ContextSuggestion;

/// Run a semantic search query against the index.
/// Returns a list of context suggestions.
pub async fn run_query(query: &str, project_path: &str) -> Result<Vec<ContextSuggestion>> {
    tracing::info!(
        query = query,
        project = project_path,
        "Running semantic query"
    );

    // In production, you wouldn't rebuild valid state every time.
    // This assumes the DB exists at project_path/data/ccm_db
    // and the graph is loaded from project_path/data/ccm_graph.json

    let artifacts = resolve_index_artifacts(project_path, None)?;
    let db_path = artifacts.db_path;
    let db_path_str = db_path.to_string_lossy().to_string();

    // Check if DB exists
    if !db_path.exists() {
        return Err(anyhow::anyhow!(
            "Index not found at '{}'. Run: ccm index -p <path>",
            db_path_str
        ));
    }

    let graph_path = artifacts.graph_path;

    let graph = if graph_path.exists() {
        tracing::debug!("Graph file found at {}, loading...", graph_path.display());
        CodeGraph::from_file(&graph_path.to_string_lossy())?
    } else {
        tracing::warn!("Graph file not found, creating new");
        CodeGraph::new()
    };

    let store = LanceDbStore::new(&db_path_str, "code_vectors").await?;
    let engine = RetrievalEngine::new(std::sync::Arc::new(tokio::sync::RwLock::new(graph)), store);

    // If query looks like file:line, do cursor prediction
    if query.contains(':') && !query.contains(' ') {
        let parts: Vec<&str> = query.split(':').collect();
        if parts.len() == 2 {
            if let Ok(line) = parts[1].parse::<usize>() {
                let file_path = parts[0];
                let normalized_path =
                    normalize_file_id(Path::new(project_path), Path::new(file_path))
                        .unwrap_or_else(|| {
                            let mut candidate = file_path.replace('\\', "/");
                            if !candidate.starts_with("./") && !candidate.starts_with("/") {
                                candidate = format!("./{}", candidate);
                            }
                            candidate
                        });
                let cursor = CursorPosition {
                    file_path: normalized_path,
                    line,
                    column: 0,
                };
                return engine.predict_context(&cursor).await;
            }
        }
    }

    // Default: Hybrid Search (Graph + Semantic)
    let results = engine.search_code_hybrid(query, 5).await?;
    Ok(results)
}

fn max_index_issues_recorded() -> usize {
    std::env::var("CCM_MAX_INDEX_ISSUES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(250)
}
const EXCLUDED_DIRECTORY_NAMES: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "vendor",
    "target",
    "dist",
    "build",
    "out",
    ".next",
    ".turbo",
    ".cache",
    "coverage",
];
const EXCLUDED_FILE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "ico", "pdf", "zip", "gz", "tar", "7z", "rar", "jar",
    "exe", "dll", "so", "dylib", "class", "o", "a", "woff", "woff2", "ttf", "eot", "mp3", "mp4",
    "mov", "avi", "bin", "key", "pem", "p12", "pfx",
];
const EXCLUDED_SECRET_FILE_NAMES: &[&str] = &[
    ".npmrc",
    ".pypirc",
    ".htaccess",
    "wp-config.php",
    "docker-compose.override.yml",
    "credentials.json",
    "secrets.json",
    "service-account.json",
    "service_account.json",
    "id_rsa",
    "id_ed25519",
];

fn build_project_walker(path: &Path, excluded_paths: &[PathBuf]) -> ignore::Walk {
    use ignore::WalkBuilder;

    let excluded_paths: Vec<PathBuf> = excluded_paths
        .iter()
        .flat_map(|excluded| {
            let raw = excluded.to_path_buf();
            match std::fs::canonicalize(excluded) {
                Ok(canonical) if canonical != raw => vec![raw, canonical],
                _ => vec![raw],
            }
        })
        .collect();

    WalkBuilder::new(path)
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .parents(true)
        .ignore(true)
        .add_custom_ignore_filename(".ccmignore")
        .filter_entry(move |entry| {
            should_traverse_entry(entry)
                && !excluded_paths
                    .iter()
                    .any(|excluded| entry.path().starts_with(excluded))
        })
        .build()
}

fn should_traverse_entry(entry: &ignore::DirEntry) -> bool {
    let Some(name) = entry.file_name().to_str() else {
        return true;
    };
    let file_type = entry.file_type();

    if file_type.map(|ft| ft.is_dir()).unwrap_or(false) {
        if name == GENERATIONS_DIRECTORY
            || name.starts_with(".ccm-rebuild-")
            || name.starts_with(".ccm-backup-")
            || name == ACTIVATION_LOCK_DIRECTORY
        {
            return false;
        }
        return !EXCLUDED_DIRECTORY_NAMES.contains(&name);
    }

    if file_type.map(|ft| ft.is_file()).unwrap_or(false) {
        let ext = Path::new(name)
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase());
        if let Some(extension) = ext {
            return !EXCLUDED_FILE_EXTENSIONS.contains(&extension.as_str());
        }
    }

    true
}

/// Index a directory recursively.
/// Parses all supported files and stores embeddings in the vector database.
pub async fn index_directory(path: &str, db_path: Option<&str>) -> Result<IndexStats> {
    let project_root = std::fs::canonicalize(path).map_err(|error| {
        anyhow::anyhow!("Project root '{}' could not be resolved: {}", path, error)
    })?;
    let final_db_path = resolve_requested_db_path(&project_root, db_path);
    let artifact_parent = final_db_path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "Invalid DB path '{}': cannot determine parent directory",
            final_db_path.display()
        )
    })?;
    std::fs::create_dir_all(artifact_parent)?;
    let activation_generation = read_current_pointer_value(artifact_parent)?;

    let generation_id = format!(
        "{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    let generations_root = artifact_parent.join(GENERATIONS_DIRECTORY);
    std::fs::create_dir_all(&generations_root)?;
    let staging_root = generations_root.join(format!("{}.staging", generation_id));
    let staging_db_path = staging_root.join("ccm_db");
    std::fs::create_dir_all(&staging_root)?;

    let fixture_namespace = fixture_namespace_for_db(&final_db_path);
    let final_graph_path = artifact_parent.join("ccm_graph.json");
    let final_manifest_path = artifact_parent.join("ccm_manifest.json");
    let build = build_index_generation(
        path,
        staging_db_path,
        &staging_root,
        &fixture_namespace,
        &[
            final_db_path.clone(),
            final_graph_path,
            final_manifest_path,
            generations_root.clone(),
            artifact_parent.join(CURRENT_GENERATION_FILE),
            artifact_parent.join(ACTIVATION_LOCK_DIRECTORY),
        ],
    )
    .await;
    let stats = match build {
        Ok(stats) => stats,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&staging_root);
            return Err(error);
        }
    };

    if let Err(error) = install_staged_generation(
        artifact_parent,
        &staging_root,
        &generation_id,
        &activation_generation,
    ) {
        let _ = std::fs::remove_dir_all(&staging_root);
        return Err(error);
    }
    Ok(stats)
}

async fn build_index_generation(
    path: &str,
    db_path_buf: PathBuf,
    artifact_parent: &Path,
    fixture_namespace: &str,
    excluded_paths: &[PathBuf],
) -> Result<IndexStats> {
    use tracing::{info, warn};

    let db_path_str = db_path_buf.to_string_lossy().to_string();
    let project_root = std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path));

    info!(path = path, db_path = %db_path_str, "Starting directory indexing");

    let mut graph = CodeGraph::new();
    let store = LanceDbStore::new_with_fixture_namespace(
        &db_path_str,
        "code_vectors",
        Some(fixture_namespace),
    )
    .await?;
    store.reset_table().await?;

    let mut stats = IndexStats::default();
    let mut manifest = IndexManifest::default();
    let mut fatal_supported_failures = 0usize;

    // Create a walker that respects gitignore + ccmignore and skips heavy noise paths.
    let walker = build_project_walker(&project_root, excluded_paths);

    // Walk directory recursively
    for result in walker {
        match result {
            Ok(entry) => {
                if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                    continue;
                }

                let file_path = entry.path();
                let file_path_str = file_path.to_string_lossy().to_string();
                let Some(file_id) = normalize_file_id_with_root(&project_root, file_path) else {
                    continue;
                };
                if is_internal_index_file(&file_id) {
                    let issue = IndexIssue {
                        path: file_id,
                        reason: IndexIssueReason::InternalIndexFile,
                        detail: "Internal CCM data file".to_string(),
                        suggested_ignore: None,
                    };
                    register_issue(&mut stats, issue, true);
                    continue;
                }

                if path_is_policy_excluded(file_path) {
                    let issue = IndexIssue {
                        path: file_id,
                        reason: IndexIssueReason::SkippedByPolicy,
                        detail: "Skipped by default exclude policy".to_string(),
                        suggested_ignore: suggestion_for_issue(
                            &file_path.to_string_lossy(),
                            &IndexIssueReason::SkippedByPolicy,
                        ),
                    };
                    register_issue(&mut stats, issue, true);
                    continue;
                }

                // Fingerprint policy kontrolünden SONRA yazılır; build_manifest
                // ile aynı sıralama korunur, yoksa incremental update bu dosyaları
                // her seferinde "silinmiş" sanır.
                let fingerprint = fingerprint_for_path(file_path).map_err(|error| {
                    anyhow::anyhow!(
                        "Full index snapshot could not fingerprint '{}': {}. Active index was preserved.",
                        file_path.display(),
                        error
                    )
                })?;
                manifest.files.insert(file_id.clone(), fingerprint);

                match populate_graph_for_file(&mut graph, file_path, &file_id) {
                    Ok(_) => {
                        stats.files_indexed += 1;
                    }
                    Err(e) => {
                        if !matches!(detect_language(file_path), SupportedLanguage::Data) {
                            fatal_supported_failures += 1;
                        }
                        let issue = issue_from_populate_error(&file_id, e);
                        tracing::debug!(
                            file = %file_path_str,
                            reason = %issue.reason.as_str(),
                            detail = %issue.detail,
                            "Failed to index file"
                        );
                        register_issue(&mut stats, issue, false);
                    }
                }
            }
            Err(err) => {
                let issue = IndexIssue {
                    path: path.to_string(),
                    reason: IndexIssueReason::WalkError,
                    detail: err.to_string(),
                    suggested_ignore: None,
                };
                register_issue(&mut stats, issue, false);
                warn!(error = %err, "Error during directory traversal");
            }
        }
    }

    let fatal_snapshot_failures = [
        IndexIssueReason::WalkError.as_str(),
        IndexIssueReason::MetadataError.as_str(),
        IndexIssueReason::ReadError.as_str(),
    ]
    .iter()
    .map(|reason| stats.reason_counts.get(*reason).copied().unwrap_or(0))
    .sum::<usize>();
    if fatal_snapshot_failures > 0 || fatal_supported_failures > 0 {
        anyhow::bail!(
            "Full index snapshot was incomplete ({} filesystem error(s), {} supported source failure(s)); active index was preserved",
            fatal_snapshot_failures,
            fatal_supported_failures
        );
    }

    let reference_edges = graph.rebuild_reference_edges();
    info!(
        edges = reference_edges,
        "Rebuilt deterministic cross-file reference graph"
    );

    // Count nodes
    stats.nodes_created = graph.graph.node_count();

    // Index into vector store
    use std::sync::Arc;
    let graph_arc = Arc::new(tokio::sync::RwLock::new(graph));
    if stats.nodes_created > 0 {
        let engine = RetrievalEngine::new(graph_arc.clone(), store);
        engine.index_graph().await?;

        info!(
            nodes = stats.nodes_created,
            files = stats.files_indexed,
            "Indexing completed successfully"
        );
    } else {
        warn!("No supported files found to index");
    }

    // Boş sonuç da kalıcılaştırılır; aksi halde önceki dolu graph diskte kalır.
    let graph_path = artifact_parent.join("ccm_graph.json");
    graph_arc
        .read()
        .await
        .save_to_file(&graph_path.to_string_lossy())?;
    info!(path = %graph_path.display(), "Graph saved to disk");

    // Save manifest for incremental indexing (even if no nodes were created).
    let manifest_path = artifact_parent.join("ccm_manifest.json");
    manifest.schema_version = INDEX_SCHEMA_VERSION;
    manifest.indexed_commit = current_head_oid(&project_root);
    save_manifest(&manifest_path, &manifest)?;

    Ok(stats)
}

fn fixture_namespace_for_db(db_path: &Path) -> String {
    db_path
        .parent()
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "default".to_string())
}

fn install_staged_generation(
    artifact_parent: &Path,
    staging_root: &Path,
    generation_id: &str,
    expected_generation: &Option<String>,
) -> Result<()> {
    let _activation_lock = ActivationLock::acquire(artifact_parent)?;
    let actual = read_current_pointer_value(artifact_parent)?;
    if &actual != expected_generation {
        anyhow::bail!(
            "Index changed concurrently while a generation was being prepared (expected {:?}, found {:?}); retry the update",
            expected_generation,
            actual
        );
    }
    validate_generation_id(generation_id)?;
    for required in ["ccm_db", "ccm_graph.json", "ccm_manifest.json"] {
        let path = staging_root.join(required);
        if !path.exists() {
            anyhow::bail!(
                "Staged index generation is incomplete; '{}' is missing",
                path.display()
            );
        }
    }

    let generations_root = artifact_parent.join(GENERATIONS_DIRECTORY);
    let generation_root = generations_root.join(generation_id);
    std::fs::rename(staging_root, &generation_root).map_err(|error| {
        anyhow::anyhow!(
            "Staged index generation '{}' could not be finalized: {}",
            staging_root.display(),
            error
        )
    })?;
    sync_directory(&generations_root)?;

    let pointer_path = artifact_parent.join(CURRENT_GENERATION_FILE);
    let pointer_temp =
        artifact_parent.join(format!("{}.{}.tmp", CURRENT_GENERATION_FILE, generation_id));
    write_synced_file(&pointer_temp, generation_id.as_bytes())?;
    if let Err(error) = replace_file_atomically(&pointer_temp, &pointer_path) {
        let _ = std::fs::remove_file(&pointer_temp);
        let _ = std::fs::remove_dir_all(&generation_root);
        return Err(anyhow::anyhow!(
            "Active index pointer '{}' could not be replaced: {}",
            pointer_path.display(),
            error
        ));
    }
    sync_directory(artifact_parent)?;
    cleanup_generations(&generations_root, generation_id);
    Ok(())
}

struct ActivationLock {
    path: PathBuf,
}

impl ActivationLock {
    fn acquire(artifact_parent: &Path) -> Result<Self> {
        let path = artifact_parent.join(ACTIVATION_LOCK_DIRECTORY);
        let started = std::time::Instant::now();
        loop {
            match std::fs::create_dir(&path) {
                Ok(()) => return Ok(Self { path }),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                    if lock_is_stale(&path) {
                        let _ = std::fs::remove_dir(&path);
                        continue;
                    }
                    if started.elapsed() >= std::time::Duration::from_secs(60) {
                        anyhow::bail!(
                            "Timed out waiting for index activation lock '{}'",
                            path.display()
                        );
                    }
                    std::thread::sleep(std::time::Duration::from_millis(25));
                }
                Err(error) => {
                    return Err(anyhow::anyhow!(
                        "Index activation lock '{}' could not be created: {}",
                        path.display(),
                        error
                    ));
                }
            }
        }
    }
}

impl Drop for ActivationLock {
    fn drop(&mut self) {
        if let Err(error) = std::fs::remove_dir(&self.path) {
            tracing::warn!(path = %self.path.display(), error = %error, "Index activation lock could not be removed");
        }
    }
}

fn lock_is_stale(path: &Path) -> bool {
    std::fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.elapsed().ok())
        .is_some_and(|age| age >= std::time::Duration::from_secs(600))
}

fn read_current_pointer_value(artifact_parent: &Path) -> Result<Option<String>> {
    let pointer_path = artifact_parent.join(CURRENT_GENERATION_FILE);
    if !pointer_path.exists() {
        return Ok(None);
    }
    Ok(Some(
        std::fs::read_to_string(&pointer_path)?.trim().to_string(),
    ))
}

fn cleanup_generations(generations_root: &Path, active_generation: &str) {
    let mut finalized = match std::fs::read_dir(generations_root) {
        Ok(entries) => entries
            .filter_map(std::result::Result::ok)
            .filter(|entry| entry.path().is_dir())
            .filter(|entry| !entry.file_name().to_string_lossy().ends_with(".staging"))
            .collect::<Vec<_>>(),
        Err(error) => {
            tracing::warn!(path = %generations_root.display(), error = %error, "Index generations could not be listed");
            return;
        }
    };
    finalized.sort_by_key(|entry| {
        std::cmp::Reverse(
            entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .unwrap_or(UNIX_EPOCH),
        )
    });

    let previous = finalized
        .iter()
        .find(|entry| entry.file_name().to_string_lossy() != active_generation)
        .map(|entry| entry.path());
    for entry in finalized {
        if entry.file_name().to_string_lossy() == active_generation
            || previous.as_ref().is_some_and(|path| path == &entry.path())
        {
            continue;
        }
        if let Err(error) = std::fs::remove_dir_all(entry.path()) {
            tracing::warn!(path = %entry.path().display(), error = %error, "Old index generation could not be removed");
        }
    }

    let stale_after = std::time::Duration::from_secs(24 * 60 * 60);
    if let Ok(entries) = std::fs::read_dir(generations_root) {
        for entry in entries.filter_map(std::result::Result::ok) {
            if !entry.file_name().to_string_lossy().ends_with(".staging") {
                continue;
            }
            let stale = entry
                .metadata()
                .and_then(|metadata| metadata.modified())
                .ok()
                .and_then(|modified| modified.elapsed().ok())
                .is_some_and(|age| age >= stale_after);
            if stale {
                let _ = std::fs::remove_dir_all(entry.path());
            }
        }
    }
}

fn resolve_requested_db_path(project_root: &Path, db_path: Option<&str>) -> PathBuf {
    let path = match db_path.map(PathBuf::from) {
        Some(path) if path.is_absolute() => path,
        Some(path) => project_root.join(path),
        None => project_root.join("data/ccm_db"),
    };
    std::fs::canonicalize(&path).unwrap_or_else(|_| normalize_path_lexically(&path))
}

fn normalize_path_lexically(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                normalized.pop();
            }
            std::path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            std::path::Component::RootDir => normalized.push(component.as_os_str()),
            std::path::Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

fn validate_generation_id(generation_id: &str) -> Result<()> {
    let valid = !generation_id.is_empty()
        && generation_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'));
    if !valid {
        anyhow::bail!("Invalid active index generation id '{}'", generation_id);
    }
    Ok(())
}

fn write_synced_file(path: &Path, content: &[u8]) -> Result<()> {
    use std::io::Write;

    let mut file = std::fs::File::create(path)?;
    file.write_all(content)?;
    file.flush()?;
    file.sync_all()?;
    Ok(())
}

#[cfg(not(windows))]
fn replace_file_atomically(source: &Path, destination: &Path) -> std::io::Result<()> {
    std::fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_file_atomically(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::iter::once;
    use std::os::windows::ffi::OsStrExt;

    const MOVEFILE_REPLACE_EXISTING: u32 = 0x1;
    const MOVEFILE_WRITE_THROUGH: u32 = 0x8;

    #[link(name = "Kernel32")]
    extern "system" {
        fn MoveFileExW(
            existing_file_name: *const u16,
            new_file_name: *const u16,
            flags: u32,
        ) -> i32;
    }

    let source_wide = source
        .as_os_str()
        .encode_wide()
        .chain(once(0))
        .collect::<Vec<_>>();
    let destination_wide = destination
        .as_os_str()
        .encode_wide()
        .chain(once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            source_wide.as_ptr(),
            destination_wide.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        std::fs::File::open(path)?.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}

fn copy_directory(source: &Path, destination: &Path) -> Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let target = destination.join(entry.file_name());
        if file_type.is_dir() {
            copy_directory(&entry.path(), &target)?;
        } else if file_type.is_file() {
            std::fs::copy(entry.path(), target)?;
        } else {
            anyhow::bail!(
                "Index artifact '{}' contains an unsupported symlink or special file",
                entry.path().display()
            );
        }
    }
    Ok(())
}

/// Updates an existing index incrementally (using Git or filesystem snapshots).
/// If the index or graph does not exist, it falls back to a full index.
pub async fn update_index(path: &str, db_path: Option<&str>) -> Result<IndexStats> {
    use tracing::info;

    let project_root = std::fs::canonicalize(path).map_err(|error| {
        anyhow::anyhow!("Project root '{}' could not be resolved: {}", path, error)
    })?;
    let requested_db_path = resolve_requested_db_path(&project_root, db_path);
    let artifact_parent = requested_db_path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "Invalid DB path '{}': cannot determine parent directory",
            requested_db_path.display()
        )
    })?;
    let active = match resolve_index_artifacts(path, db_path) {
        Ok(active) => active,
        Err(error) => {
            tracing::warn!(
                error = %error,
                "Active index pointer is invalid. Performing staged full re-index."
            );
            return index_directory(path, db_path).await;
        }
    };

    let embedder_disabled = std::env::var("CCM_DISABLE_EMBEDDER")
        .or_else(|_| std::env::var("EMBEDDING_DISABLED"))
        .map(|value| matches!(value.to_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false);
    let fixture_enabled = std::env::var("CCM_EMBEDDING_FIXTURE")
        .ok()
        .is_some_and(|value| !value.trim().is_empty());
    if !active.graph_path.exists() || !active.manifest_path.exists() || !active.db_path.is_dir() {
        info!(
            graph = %active.graph_path.display(),
            vector_db = %active.db_path.display(),
            "Index artifacts are incomplete. Performing full index."
        );
        return index_directory(path, db_path).await;
    }

    // Bozuk graph sessiz boş sonuca dönüşmez; kontrollü full rebuild ile onarılır.
    let graph = match CodeGraph::from_file(&active.graph_path.to_string_lossy()) {
        Ok(graph) => graph,
        Err(error) => {
            tracing::warn!(
                path = %active.graph_path.display(),
                error = %error,
                "Existing graph is unreadable. Performing staged full re-index."
            );
            return index_directory(path, db_path).await;
        }
    };
    let legacy_paths = graph.graph.node_weights().any(|node| {
        if !matches!(
            node.node_type,
            crate::graph::NodeType::File | crate::graph::NodeType::DataFile
        ) {
            return false;
        }
        let name = node.name.as_str();
        let is_abs = Path::new(name).is_absolute();
        let has_prefix = name.starts_with("./");
        is_abs || !has_prefix
    });
    if legacy_paths {
        info!("Legacy index detected. Performing full re-index.");
        return index_directory(path, db_path).await;
    }

    let semantic_nodes = semantic_node_count(&graph);
    let vector_table_required = (fixture_enabled || !embedder_disabled) && semantic_nodes > 0;
    let vector_table_path = active.db_path.join("code_vectors.lance");
    if vector_table_required {
        let fixture_namespace = fixture_namespace_for_db(&requested_db_path);
        let vector_health = if vector_table_path.exists() {
            match LanceDbStore::new_with_fixture_namespace(
                &active.db_path.to_string_lossy(),
                "code_vectors",
                Some(&fixture_namespace),
            )
            .await
            {
                Ok(store) => store
                    .validate_table()
                    .await
                    .map(|rows| rows >= semantic_nodes),
                Err(error) => Err(error),
            }
        } else {
            Ok(false)
        };
        if !matches!(vector_health, Ok(true)) {
            tracing::warn!(
                vector_table = %vector_table_path.display(),
                error = ?vector_health.err(),
                "Vector index is incomplete or corrupt. Performing full index."
            );
            return index_directory(path, db_path).await;
        }
    }

    let manifest = load_manifest(&active.manifest_path);
    if manifest.schema_version != INDEX_SCHEMA_VERSION {
        info!(
            found = manifest.schema_version,
            expected = INDEX_SCHEMA_VERSION,
            "Index schema changed. Performing full re-index."
        );
        return index_directory(path, db_path).await;
    }

    let new_manifest = build_manifest(
        &project_root,
        &[
            requested_db_path.clone(),
            artifact_parent.join("ccm_graph.json"),
            artifact_parent.join("ccm_manifest.json"),
            artifact_parent.join(CURRENT_GENERATION_FILE),
            artifact_parent.join(GENERATIONS_DIRECTORY),
            artifact_parent.join(ACTIVATION_LOCK_DIRECTORY),
        ],
    )?;
    let (changed_rel, deleted_rel) = diff_manifest(&manifest, &new_manifest);

    if changed_rel.is_empty() && deleted_rel.is_empty() {
        info!("No changes detected.");
        return Ok(IndexStats::default());
    }

    let changed_files: Vec<PathBuf> = changed_rel
        .iter()
        .chain(deleted_rel.iter())
        .map(|rel| file_id_to_path(&project_root, rel))
        .collect();
    let mut committed_manifest = new_manifest;

    let changed_files: Vec<PathBuf> = changed_files
        .into_iter()
        .filter(|p| {
            normalize_file_id_with_root(&project_root, p)
                .map(|id| !is_internal_index_file(&id))
                .unwrap_or(true)
        })
        .collect();

    // Nothing left to process after filtering — index is already up to date.
    if changed_files.is_empty() {
        info!("No actionable changes detected after filtering. Index is up to date.");
        return Ok(IndexStats::default());
    }

    let generation_id = new_generation_id();
    let generations_root = artifact_parent.join(GENERATIONS_DIRECTORY);
    std::fs::create_dir_all(&generations_root)?;
    let staging_root = generations_root.join(format!("{}.staging", generation_id));
    std::fs::create_dir_all(&staging_root)?;
    let staged_db_path = staging_root.join("ccm_db");
    let staged_graph_path = staging_root.join("ccm_graph.json");
    let staged_manifest_path = staging_root.join("ccm_manifest.json");

    let staged_result: Result<IndexStats> = async {
        copy_directory(&active.db_path, &staged_db_path)?;
        std::fs::copy(&active.graph_path, &staged_graph_path)?;
        std::fs::copy(&active.manifest_path, &staged_manifest_path)?;

        let staged_graph = CodeGraph::from_file(&staged_graph_path.to_string_lossy())?;
        let store = LanceDbStore::new_with_fixture_namespace(
            &staged_db_path.to_string_lossy(),
            "code_vectors",
            Some(&fixture_namespace_for_db(&requested_db_path)),
        )
        .await?;
        let graph_arc = std::sync::Arc::new(tokio::sync::RwLock::new(staged_graph));
        let engine = RetrievalEngine::new(graph_arc.clone(), store);

        info!("Starting incremental indexing for {}", path);
        let stats = engine.incremental_index_paths(path, &changed_files).await?;

        // Hazırlanamayan dosyaların eski fingerprint'i korunur; sonraki koşu yeniden dener.
        if !stats.retry_files.is_empty() {
            committed_manifest.indexed_commit = manifest.indexed_commit.clone();
            for path in &stats.retry_files {
                match manifest.files.get(path) {
                    Some(previous) => {
                        committed_manifest
                            .files
                            .insert(path.clone(), previous.clone());
                    }
                    None => {
                        committed_manifest.files.remove(path);
                    }
                }
            }
        }

        graph_arc
            .read()
            .await
            .save_to_file(&staged_graph_path.to_string_lossy())?;
        save_manifest(&staged_manifest_path, &committed_manifest)?;
        Ok(stats)
    }
    .await;

    let stats = match staged_result {
        Ok(stats) => stats,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&staging_root);
            return Err(error);
        }
    };
    if let Err(error) = install_staged_generation(
        artifact_parent,
        &staging_root,
        &generation_id,
        &active.generation_id,
    ) {
        let _ = std::fs::remove_dir_all(&staging_root);
        return Err(error);
    }
    Ok(stats)
}

pub fn semantic_node_count(graph: &CodeGraph) -> usize {
    let embed_data_files = std::env::var("CCM_EMBED_DATA_FILES")
        .map(|value| matches!(value.to_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false);
    graph
        .graph
        .node_weights()
        .filter(|node| {
            matches!(
                node.node_type,
                crate::graph::NodeType::Function
                    | crate::graph::NodeType::Method
                    | crate::graph::NodeType::Class
                    | crate::graph::NodeType::Struct
            ) || (embed_data_files && matches!(node.node_type, crate::graph::NodeType::DataFile))
        })
        .count()
}

fn new_generation_id() -> String {
    format!(
        "{}.{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    )
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum IndexIssueReason {
    WalkError,
    SkippedByPolicy,
    InternalIndexFile,
    FileTooLarge,
    BinaryFile,
    NonUtf8File,
    MetadataError,
    ReadError,
    ParseError,
    ExtractError,
}

impl IndexIssueReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::WalkError => "walk_error",
            Self::SkippedByPolicy => "skipped_by_policy",
            Self::InternalIndexFile => "internal_index_file",
            Self::FileTooLarge => "file_too_large",
            Self::BinaryFile => "binary_file",
            Self::NonUtf8File => "non_utf8_file",
            Self::MetadataError => "metadata_error",
            Self::ReadError => "read_error",
            Self::ParseError => "parse_error",
            Self::ExtractError => "extract_error",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexIssue {
    pub path: String,
    pub reason: IndexIssueReason,
    pub detail: String,
    pub suggested_ignore: Option<String>,
}

/// Statistics from an indexing operation
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct IndexStats {
    pub files_indexed: usize,
    pub files_failed: usize,
    pub files_skipped: usize,
    pub nodes_created: usize,
    pub failed_files: Vec<IndexIssue>,
    pub skipped_files: Vec<IndexIssue>,
    pub reason_counts: HashMap<String, usize>,
    pub suggested_ignores: Vec<String>,
    #[serde(skip)]
    pub(crate) retry_files: Vec<String>,
}

enum PopulateFileError {
    Read(FileReadError),
    Parse(anyhow::Error),
    Extract(anyhow::Error),
}

impl std::fmt::Display for PopulateFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(err) => write!(f, "{}", err),
            Self::Parse(err) => write!(f, "{}", err),
            Self::Extract(err) => write!(f, "{}", err),
        }
    }
}

impl std::fmt::Debug for PopulateFileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read(err) => f.debug_tuple("Read").field(err).finish(),
            Self::Parse(err) => f.debug_tuple("Parse").field(err).finish(),
            Self::Extract(err) => f.debug_tuple("Extract").field(err).finish(),
        }
    }
}

impl std::error::Error for PopulateFileError {}

impl From<FileReadError> for PopulateFileError {
    fn from(value: FileReadError) -> Self {
        Self::Read(value)
    }
}

fn populate_graph_for_file(
    graph: &mut CodeGraph,
    file_path: &Path,
    file_id: &str,
) -> std::result::Result<(), PopulateFileError> {
    use crate::vector::extractor::Extractor;

    let content = read_text_file_limited(file_path).map_err(|error| {
        match error.downcast::<FileReadError>() {
            Ok(file_error) => PopulateFileError::Read(file_error),
            Err(other) => PopulateFileError::Read(FileReadError::Read {
                path: file_path.to_string_lossy().to_string(),
                source: std::io::Error::other(other.to_string()),
            }),
        }
    })?;

    let lang = detect_language(file_path);

    // If it's a Data file, we bypass the AST parser and just create a file-level node
    if matches!(lang, SupportedLanguage::Data) {
        use crate::graph::CodeNode;
        use crate::graph::NodeType;

        let node = CodeNode {
            id: file_id.to_string(),
            node_type: NodeType::DataFile,
            name: file_id.to_string(),
            content: content.as_str().into(),
            start_line: 1,
            end_line: content.lines().count().max(1),
        };
        graph.add_node(node);
        return Ok(());
    }

    // Parse AST
    let mut parser = CodeParser::new();
    let tree = parser
        .parse_tree(&content, lang)
        .map_err(PopulateFileError::Parse)?;

    // Tanımları çıkar (Files, Functions, Classes, vb.).
    // Calls/Imports kenarları indexleme sonunda rebuild_reference_edges ile
    // deterministik olarak yeniden üretildiği için ayrı bir pass gerekmez.
    let mut extractor = Extractor::new(content.clone(), lang);
    extractor
        .extract(&tree, graph, file_id)
        .map_err(PopulateFileError::Extract)?;

    Ok(())
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct IndexManifest {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    indexed_commit: Option<String>,
    files: HashMap<String, FileFingerprint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct FileFingerprint {
    modified_sec: u64,
    #[serde(default)]
    modified_nsec: u32,
    size: u64,
    #[serde(default)]
    content_hash: u64,
}

pub(crate) fn normalize_file_id(project_root: &Path, path: &Path) -> Option<String> {
    let root = std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
    normalize_file_id_with_root(&root, path)
}

/// Önceden canonicalize edilmiş kök ile çalışır; sıcak döngülerde root'un
/// her çağrıda yeniden canonicalize edilmesini (ekstra syscall) önler.
pub(crate) fn normalize_file_id_with_root(root: &Path, path: &Path) -> Option<String> {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let abs = std::fs::canonicalize(&abs).unwrap_or(abs);
    let rel = abs.strip_prefix(root).ok()?;
    let mut rel_str = rel.to_string_lossy().to_string();
    if rel_str.is_empty() {
        rel_str = ".".to_string();
    }
    rel_str = rel_str.replace('\\', "/");
    if !rel_str.starts_with("./") {
        rel_str = format!("./{}", rel_str);
    }
    Some(rel_str)
}

pub(crate) fn normalize_node_id(id: &str) -> String {
    id.split('#').next().unwrap_or(id).to_string()
}

fn is_internal_index_file(file_id: &str) -> bool {
    file_id == "./data/ccm_graph.json"
        || file_id == "./data/ccm_manifest.json"
        || file_id.starts_with("./data/ccm_db/")
}

/// Bir dosya değişikliğinin indexleyici için ilgili olup olmadığını bildirir.
/// CLI watch modu bunu kullanır; indexleyici ile aynı politikayı tek kaynaktan
/// uygular (policy exclusion + internal artifact + binary uzantı filtresi).
pub fn is_index_relevant_file(project_root: &Path, path: &Path) -> bool {
    if path_is_policy_excluded(path) {
        return false;
    }
    if normalize_file_id(project_root, path).is_some_and(|file_id| is_internal_index_file(&file_id))
    {
        return false;
    }
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    match extension {
        Some(ext) => !EXCLUDED_FILE_EXTENSIONS.contains(&ext.as_str()),
        None => true,
    }
}

fn file_id_to_path(project_root: &Path, file_id: &str) -> PathBuf {
    let rel = file_id.trim_start_matches("./");
    project_root.join(rel)
}

fn fingerprint_for_path(path: &Path) -> std::io::Result<FileFingerprint> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)?;
    let meta = file.metadata()?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok());
    let mut content_hash = 0xcbf29ce484222325u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        for byte in &buffer[..read] {
            content_hash ^= u64::from(*byte);
            content_hash = content_hash.wrapping_mul(0x100000001b3);
        }
    }

    Ok(FileFingerprint {
        modified_sec: modified.as_ref().map(|value| value.as_secs()).unwrap_or(0),
        modified_nsec: modified
            .as_ref()
            .map(|value| value.subsec_nanos())
            .unwrap_or(0),
        size: meta.len(),
        content_hash,
    })
}

fn load_manifest(path: &Path) -> IndexManifest {
    if let Ok(file) = std::fs::File::open(path) {
        if let Ok(manifest) = serde_json::from_reader(file) {
            return manifest;
        }
    }
    IndexManifest::default()
}

fn save_manifest(path: &Path, manifest: &IndexManifest) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp_path = path.with_extension(format!("json.{}.tmp", std::process::id()));
    let file = std::fs::File::create(&temp_path)?;
    let mut writer = std::io::BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, manifest)?;
    use std::io::Write;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    std::fs::rename(&temp_path, path)?;
    Ok(())
}

fn build_manifest(project_root: &Path, excluded_paths: &[PathBuf]) -> Result<IndexManifest> {
    let mut manifest = IndexManifest {
        schema_version: INDEX_SCHEMA_VERSION,
        indexed_commit: current_head_oid(project_root),
        files: HashMap::new(),
    };
    let walker = build_project_walker(project_root, excluded_paths);

    for result in walker {
        let entry = result.map_err(|error| {
            anyhow::anyhow!(
                "Project snapshot could not be completed for '{}': {}. Existing index was preserved.",
                project_root.display(),
                error
            )
        })?;

        if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            continue;
        }

        let file_path = entry.path();
        let Some(file_id) = normalize_file_id_with_root(project_root, file_path) else {
            continue;
        };
        if is_internal_index_file(&file_id) {
            continue;
        }
        if path_is_policy_excluded(file_path) {
            continue;
        }

        let fingerprint = fingerprint_for_path(file_path).map_err(|error| {
            anyhow::anyhow!(
                "Project snapshot could not read '{}': {}. Existing index was preserved.",
                file_path.display(),
                error
            )
        })?;
        manifest.files.insert(file_id, fingerprint);
    }

    Ok(manifest)
}

fn diff_manifest(
    old_manifest: &IndexManifest,
    new_manifest: &IndexManifest,
) -> (Vec<String>, Vec<String>) {
    let mut changed = Vec::new();
    let mut deleted = Vec::new();

    for (path, new_fp) in &new_manifest.files {
        match old_manifest.files.get(path) {
            Some(old_fp) if old_fp == new_fp => {}
            _ => changed.push(path.clone()),
        }
    }

    for path in old_manifest.files.keys() {
        if !new_manifest.files.contains_key(path) {
            deleted.push(path.clone());
        }
    }

    (changed, deleted)
}

pub(crate) fn path_is_policy_excluded(file_path: &Path) -> bool {
    // Yalnızca dizin adları politika kapsamındadır; "build" veya "out" adlı
    // bir dosya kendi adından dolayı dışlanmamalı.
    let parent_components = file_path
        .parent()
        .map(|parent| parent.components())
        .into_iter()
        .flatten();
    for component in parent_components {
        let text = component.as_os_str().to_string_lossy();
        if EXCLUDED_DIRECTORY_NAMES.contains(&text.as_ref()) {
            return true;
        }
    }

    let file_name = file_path
        .file_name()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    if let Some(name) = file_name {
        if (name == ".env" || name.starts_with(".env."))
            && name != ".env.example"
            && name != ".env.sample"
        {
            return true;
        }
        if EXCLUDED_SECRET_FILE_NAMES.contains(&name.as_str())
            || name.starts_with("service-account-")
            || name.starts_with("service_account_")
        {
            return true;
        }
    }

    let extension = file_path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase());
    if let Some(ext) = extension {
        return EXCLUDED_FILE_EXTENSIONS.contains(&ext.as_str());
    }

    false
}

fn current_head_oid(project_root: &Path) -> Option<String> {
    let repo = git2::Repository::open(project_root).ok()?;
    let oid = repo.head().ok()?.target().map(|value| value.to_string());
    oid
}

fn issue_from_read_error(path: &str, error: FileReadError) -> IndexIssue {
    let (reason, detail) = match error {
        FileReadError::TooLarge {
            size_bytes,
            limit_bytes,
            ..
        } => (
            IndexIssueReason::FileTooLarge,
            format!(
                "File is too large ({} bytes > {} bytes)",
                size_bytes, limit_bytes
            ),
        ),
        FileReadError::BinaryNul { .. } => (
            IndexIssueReason::BinaryFile,
            "Binary file detected (contains NUL bytes)".to_string(),
        ),
        FileReadError::NonUtf8 { source, .. } => (
            IndexIssueReason::NonUtf8File,
            format!("File is not UTF-8 text: {}", source),
        ),
        FileReadError::Metadata { source, .. } => (
            IndexIssueReason::MetadataError,
            format!("Failed to read file metadata: {}", source),
        ),
        FileReadError::Read { source, .. } => (
            IndexIssueReason::ReadError,
            format!("Failed to read file content: {}", source),
        ),
    };

    IndexIssue {
        path: path.to_string(),
        suggested_ignore: suggestion_for_issue(path, &reason),
        reason,
        detail,
    }
}

fn issue_from_populate_error(path: &str, error: PopulateFileError) -> IndexIssue {
    match error {
        PopulateFileError::Read(read_error) => issue_from_read_error(path, read_error),
        PopulateFileError::Parse(parse_error) => IndexIssue {
            path: path.to_string(),
            reason: IndexIssueReason::ParseError,
            detail: parse_error.to_string(),
            suggested_ignore: suggestion_for_issue(path, &IndexIssueReason::ParseError),
        },
        PopulateFileError::Extract(extract_error) => IndexIssue {
            path: path.to_string(),
            reason: IndexIssueReason::ExtractError,
            detail: extract_error.to_string(),
            suggested_ignore: suggestion_for_issue(path, &IndexIssueReason::ExtractError),
        },
    }
}

pub(crate) fn suggestion_for_issue(path: &str, reason: &IndexIssueReason) -> Option<String> {
    let normalized = path.replace('\\', "/");
    let first_segment = normalized
        .trim_start_matches("./")
        .split('/')
        .next()
        .map(|value| value.to_string());

    if let Some(segment) = first_segment {
        if EXCLUDED_DIRECTORY_NAMES.contains(&segment.as_str()) {
            return Some(format!("./{}/**", segment));
        }
    }

    if matches!(
        reason,
        IndexIssueReason::FileTooLarge
            | IndexIssueReason::BinaryFile
            | IndexIssueReason::NonUtf8File
            | IndexIssueReason::ParseError
    ) {
        let extension = Path::new(&normalized)
            .extension()
            .and_then(|value| value.to_str())
            .map(|value| value.to_ascii_lowercase());
        if let Some(ext) = extension {
            return Some(format!("**/*.{}", ext));
        }
    }

    None
}

pub(crate) fn register_issue(stats: &mut IndexStats, issue: IndexIssue, skipped: bool) {
    let reason_key = issue.reason.as_str().to_string();
    *stats.reason_counts.entry(reason_key).or_insert(0) += 1;

    if let Some(pattern) = &issue.suggested_ignore {
        if !stats.suggested_ignores.contains(pattern) {
            stats.suggested_ignores.push(pattern.clone());
        }
    }

    if skipped {
        stats.files_skipped += 1;
        if stats.skipped_files.len() < max_index_issues_recorded() {
            stats.skipped_files.push(issue);
        }
        return;
    }

    stats.files_failed += 1;
    stats.retry_files.push(issue.path.clone());
    if stats.failed_files.len() < max_index_issues_recorded() {
        stats.failed_files.push(issue);
    }
}

#[cfg(test)]
mod policy_tests {
    use super::{
        diff_manifest, fingerprint_for_path, install_staged_generation, path_is_policy_excluded,
        FileFingerprint, IndexManifest, CURRENT_GENERATION_FILE, GENERATIONS_DIRECTORY,
    };
    use std::collections::HashMap;
    use std::path::Path;

    #[test]
    fn secret_files_are_excluded_but_examples_remain_indexable() {
        assert!(path_is_policy_excluded(Path::new("/repo/.env")));
        assert!(path_is_policy_excluded(Path::new("/repo/.env.production")));
        assert!(path_is_policy_excluded(Path::new("/repo/credentials.json")));
        assert!(path_is_policy_excluded(Path::new("/repo/private.key")));
        assert!(!path_is_policy_excluded(Path::new("/repo/.env.example")));
    }

    #[test]
    fn manifest_diff_detects_clean_checkout_content_changes() {
        let old = IndexManifest {
            schema_version: 2,
            indexed_commit: Some("old".to_string()),
            files: HashMap::from([(
                "./src/lib.rs".to_string(),
                FileFingerprint {
                    modified_sec: 1,
                    modified_nsec: 0,
                    size: 10,
                    content_hash: 1,
                },
            )]),
        };
        let new = IndexManifest {
            schema_version: 2,
            indexed_commit: Some("new".to_string()),
            files: HashMap::from([(
                "./src/lib.rs".to_string(),
                FileFingerprint {
                    modified_sec: 2,
                    modified_nsec: 0,
                    size: 12,
                    content_hash: 2,
                },
            )]),
        };

        let (changed, deleted) = diff_manifest(&old, &new);
        assert_eq!(changed, vec!["./src/lib.rs"]);
        assert!(deleted.is_empty());
    }

    #[test]
    fn fingerprint_detects_same_size_content_changes() {
        let directory = tempfile::tempdir().expect("temp directory");
        let path = directory.path().join("same-size.rs");
        std::fs::write(&path, "alpha").expect("initial content");
        let before = fingerprint_for_path(&path).expect("initial fingerprint");

        std::fs::write(&path, "bravo").expect("replacement content");
        let after = fingerprint_for_path(&path).expect("replacement fingerprint");

        assert_eq!(before.size, after.size);
        assert_ne!(before.content_hash, after.content_hash);
    }

    #[test]
    fn activation_rejects_a_stale_incremental_generation() {
        let directory = tempfile::tempdir().expect("temp directory");
        let generations = directory.path().join(GENERATIONS_DIRECTORY);
        let current = generations.join("current");
        std::fs::create_dir_all(current.join("ccm_db")).expect("current db");
        std::fs::write(current.join("ccm_graph.json"), "{}").expect("current graph");
        std::fs::write(current.join("ccm_manifest.json"), "{}").expect("current manifest");
        std::fs::write(directory.path().join(CURRENT_GENERATION_FILE), "current")
            .expect("current pointer");

        let staged = generations.join("candidate.staging");
        std::fs::create_dir_all(staged.join("ccm_db")).expect("staged db");
        std::fs::write(staged.join("ccm_graph.json"), "{}").expect("staged graph");
        std::fs::write(staged.join("ccm_manifest.json"), "{}").expect("staged manifest");

        let result = install_staged_generation(
            directory.path(),
            &staged,
            "candidate",
            &Some("stale".to_string()),
        );
        assert!(result.is_err());
        assert_eq!(
            std::fs::read_to_string(directory.path().join(CURRENT_GENERATION_FILE))
                .expect("pointer"),
            "current"
        );
        assert!(staged.exists());
        assert!(!generations.join("candidate").exists());
    }
}
