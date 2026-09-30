pub mod engine;

pub mod eval;
pub mod fixtures;
mod fs_utils;
pub mod git;
pub mod graph;
pub mod hash;
pub mod live;

pub mod parser;

pub mod optimize;
pub mod policy;
pub mod rng;
pub mod trajectory;
pub mod vector;
mod watch_filter;

use crate::engine::{CursorPosition, RetrievalEngine};
use crate::fs_utils::{detect_language, read_text_file_limited, FileReadError};
use crate::graph::CodeGraph;
use crate::parser::{CodeParser, SupportedLanguage};
use crate::vector::embedder::{EmbeddingIdentity, EmbeddingIdentityMismatch, EmbeddingSource};
use crate::vector::store::LanceDbStore;
use anyhow::Result;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

pub const INDEX_SCHEMA_VERSION: u32 = 4;
const GENERATIONS_DIRECTORY: &str = ".ccm-generations";
const CURRENT_GENERATION_FILE: &str = "ccm_current";
/// Etkinleştirme kilidinin taşıyıcı dosyası (bkz. `ActivationLock`). Eski
/// sürümlerin kilit dizininden bilerek farklı bir ad taşır; ikisi asla çakışmaz.
const ACTIVATION_LOCK_FILE: &str = ".ccm-activation.flock";
/// Eski sürümlerin mkdir tabanlı kilit dizini. Artık kilit olarak kullanılmaz ve
/// silinmez (başka bir süreç hâlâ kullanıyor olabilir); geride kalmış bir dizinin
/// tarama ve izlemenin dışında tutulması için yalnızca tanınır.
const LEGACY_ACTIVATION_LOCK_DIRECTORY: &str = ".ccm-activation.lock";
/// Öğrenme verisinin dizin adı: trajectory günlüğü (`data/ccm_learn/experiences.jsonl`)
/// ve politika deposu (`<db dizini>/ccm_learn/policies.json`) burada tutulur. Araç
/// durumudur; indeksin girdisi değildir.
const LEARN_DIRECTORY: &str = "ccm_learn";

/// Ham ve kanonik yol varyasyonlarını çıkarır: eğer kanonik form raw'dan
/// farklıysa her ikisini de verir, yoksa raw'ı verir. Symlink'leri yakalar.
fn with_canonical_variants(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths
        .into_iter()
        .flat_map(|path| {
            let raw = path;
            match std::fs::canonicalize(&raw) {
                Ok(canonical) if canonical != raw => vec![raw, canonical],
                _ => vec![raw],
            }
        })
        .collect()
}

/// Indekslemenin taranması sırasında atlanan hazırlama ve generation
/// dizinlerini tanır: `.ccm-generations`, eski sürümlerin `.ccm-activation.lock`
/// kilit dizini, `.ccm-rebuild-*`, `.ccm-backup-*` prefixleri.
fn is_index_staging_dir_name(name: &str) -> bool {
    name == GENERATIONS_DIRECTORY
        || name == LEGACY_ACTIVATION_LOCK_DIRECTORY
        || name.starts_with(".ccm-rebuild-")
        || name.starts_with(".ccm-backup-")
}

/// İndeks artefaktlarının atomik yazımda kullandığı geçici dosya adlarını tanır.
/// Adlar yazan koddaki `format!`/`with_extension` çağrılarıyla birebir aynı desenleri
/// izler: `ccm_current.<generation>.tmp` (etkin işaretçi, generation aktivasyonu),
/// `ccm_manifest.json.<pid>.tmp` (`save_manifest`) ve `ccm_graph.json.<pid>.tmp`
/// (`CodeGraph::save_to_file`). Manifest ve graf geçici dosyaları güncel akışta
/// staging dizininde oluşur; düz yerleşimde artefakt dizininde de görülebileceği
/// için aynı desenler tanınır.
fn is_index_artifact_temp_name(name: &str) -> bool {
    let Some(stem) = name.strip_suffix(".tmp") else {
        return false;
    };
    [
        CURRENT_GENERATION_FILE,
        "ccm_manifest.json",
        "ccm_graph.json",
    ]
    .iter()
    .any(|artifact| {
        stem.strip_prefix(artifact)
            .is_some_and(|rest| rest.starts_with('.'))
    })
}

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
    let requested_db = resolve_requested_db_path(&project_root, db_path)?;
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
pub use watch_filter::{
    build_watch_filter, is_watch_relevant_dir, is_watch_relevant_path, WatchFilter,
};

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

    // İndeks başka bir embedding modeliyle kurulduysa sorgu vektörü tabloyla
    // karşılaştırılamaz; arama semantiği atlayıp graf fallback'ine geçer.
    let recorded = read_index_embedding(&artifacts.manifest_path)?;
    let mismatch = EmbeddingSource::from_env()?.mismatch_with(recorded.as_ref());
    let store = LanceDbStore::new(&db_path_str, "code_vectors")
        .await?
        .with_identity_mismatch(mismatch);
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

/// Tam ve kapsamlı taramaların ortak ayarları: gitignore, `.ignore`,
/// `.ccmignore` ve üst dizinlerin ignore dosyaları uygulanır.
fn project_walk_builder(path: &Path) -> ignore::WalkBuilder {
    let mut builder = ignore::WalkBuilder::new(path);
    builder
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .parents(true)
        .ignore(true)
        .add_custom_ignore_filename(".ccmignore");
    builder
}

fn build_project_walker(path: &Path, excluded_paths: &[PathBuf]) -> ignore::Walk {
    let excluded_paths = with_canonical_variants(excluded_paths.to_vec());
    project_walk_builder(path)
        .filter_entry(move |entry| {
            should_traverse_entry(entry)
                && !excluded_paths
                    .iter()
                    .any(|excluded| entry.path().starts_with(excluded))
        })
        .build()
}

/// Tam taramayla aynı kök ve kurallarla yürür ama yalnızca `scope` yollarının
/// atalarına ve altlarına iner. Bir dosyanın dahil edilip edilmediği tam
/// taramayla birebir aynıdır; yalnızca ziyaret edilen dizinler azalır.
fn build_scoped_project_walker(
    path: &Path,
    excluded_paths: &[PathBuf],
    scope: Vec<PathBuf>,
) -> ignore::Walk {
    let excluded_paths = with_canonical_variants(excluded_paths.to_vec());
    project_walk_builder(path)
        .filter_entry(move |entry| {
            let entry_path = entry.path();
            should_traverse_entry(entry)
                && !excluded_paths
                    .iter()
                    .any(|excluded| entry_path.starts_with(excluded))
                && scope
                    .iter()
                    .any(|target| target.starts_with(entry_path) || entry_path.starts_with(target))
        })
        .build()
}

fn should_traverse_entry(entry: &ignore::DirEntry) -> bool {
    let Some(name) = entry.file_name().to_str() else {
        return true;
    };
    let file_type = entry.file_type();

    if file_type.map(|ft| ft.is_dir()).unwrap_or(false) {
        if is_index_staging_dir_name(name) {
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
    index_directory_with_mode(path, db_path, IndexMode::Full).await
}

/// İndeksleme kapsamı. `Quick` embedding atlayıp yalnızca graph üretir;
/// semantic vektörler `upgrade_active_index_semantics` ile arka planda dolar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexMode {
    Full,
    Quick,
}

/// Belirtilen modda sondan sona tam indeksleme yapar.
pub async fn index_directory_with_mode(
    path: &str,
    db_path: Option<&str>,
    mode: IndexMode,
) -> Result<IndexStats> {
    let project_root = std::fs::canonicalize(path).map_err(|error| {
        anyhow::anyhow!("Project root '{}' could not be resolved: {}", path, error)
    })?;
    let final_db_path = resolve_requested_db_path(&project_root, db_path)?;
    record_git_excludes(&project_root);
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
    let build = build_index_generation(
        path,
        staging_db_path,
        &staging_root,
        &fixture_namespace,
        &index_artifact_paths(artifact_parent, &final_db_path),
        mode,
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

/// Paralel indexleme aşamasının tek dosya için ürettiği sonuç.
struct FileIndexOutcome {
    file_path: PathBuf,
    file_id: String,
    fingerprint: std::io::Result<FileFingerprint>,
    graph: Option<CodeGraph>,
    populate_error: Option<PopulateFileError>,
}

async fn build_index_generation(
    path: &str,
    db_path_buf: PathBuf,
    artifact_parent: &Path,
    fixture_namespace: &str,
    excluded_paths: &[PathBuf],
    mode: IndexMode,
) -> Result<IndexStats> {
    use tracing::{info, warn};

    let db_path_str = db_path_buf.to_string_lossy().to_string();
    let project_root = std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path));

    info!(path = path, db_path = %db_path_str, "Starting directory indexing");

    // Manifestin racy penceresi taramadan önceki ana göre hesaplanır.
    let snapshot_started_at = unix_now_secs();

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

    // Aşama 1 (sıralı): indekslenecek adayları topla. Yalnız ucuz metadata
    // kontrolleri yapılır; dosya içeriği paralel aşamada okunur.
    let mut candidates: Vec<(PathBuf, String)> = Vec::new();
    for result in walker {
        match result {
            Ok(entry) => {
                if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                    continue;
                }

                let file_path = entry.path().to_path_buf();
                let Some(file_id) = relative_file_id(&project_root, &file_path) else {
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

                if path_is_policy_excluded(Path::new(&file_id)) {
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

                candidates.push((file_path, file_id));
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

    // Aşama 2 (paralel): her aday için fingerprint + parse ayrı çalışan
    // işçide yapılır; sonuçlar sıralı merge için toplanır. Büyük repolarda
    // CPU-bound Tree-sitter ayrıştırması çekirdek sayısı kadar hızlanır.
    let outcomes: Vec<FileIndexOutcome> = candidates
        .par_iter()
        .map(|(file_path, file_id)| {
            let fingerprint = fingerprint_for_path(file_path);
            let mut file_graph = CodeGraph::new();
            let populate_result = populate_graph_for_file(&mut file_graph, file_path, file_id);
            FileIndexOutcome {
                file_path: file_path.clone(),
                file_id: file_id.clone(),
                fingerprint,
                graph: if populate_result.is_ok() {
                    Some(file_graph)
                } else {
                    None
                },
                populate_error: populate_result.err(),
            }
        })
        .collect();

    // Aşama 3 (sıralı): sonuçları ana grafa/manifeste/istatistiklere birleştir.
    for outcome in outcomes {
        let file_path_str = outcome.file_path.to_string_lossy().to_string();
        // Fingerprint policy kontrolünden SONRA yazılır; build_manifest ile
        // aynı sıralama korunur, yoksa incremental update bu dosyaları her
        // seferinde "silinmiş" sanır.
        let fingerprint = outcome.fingerprint.map_err(|error| {
            anyhow::anyhow!(
                "Full index snapshot could not fingerprint '{}': {}. Active index was preserved.",
                outcome.file_path.display(),
                error
            )
        })?;
        manifest.files.insert(outcome.file_id.clone(), fingerprint);

        match outcome.graph {
            Some(file_graph) => {
                graph.append_graph(&file_graph);
                stats.files_indexed += 1;
            }
            None => {
                let error = outcome.populate_error.expect("populate error");
                // Belirlenimci içerik hataları (çok büyük dosya, binary NUL)
                // dosyanın kalıcı olarak indekslenemez olduğunu gösterir; tek
                // dosya TÜM full index'i bloke etmemelidir (diğer dosyalar için
                // tam index geçerlidir). Geri kalan hatalar (non-UTF8, parse,
                // geçici IO) v0.3.8 güvencesiyle active index'i korur.
                if !matches!(detect_language(&outcome.file_path), SupportedLanguage::Data)
                    && !populate_error_is_unindexable_content(&error)
                {
                    fatal_supported_failures += 1;
                }
                let issue = issue_from_populate_error(&outcome.file_id, error);
                tracing::debug!(
                    file = %file_path_str,
                    reason = %issue.reason.as_str(),
                    detail = %issue.detail,
                    "Failed to index file"
                );
                let permanent = is_permanent_issue(&issue.reason);
                register_issue(&mut stats, issue, permanent);
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

    // Index into vector store. Quick mod embedding'i bilinçli olarak atlar;
    // grafik yine de tam kurulur ve vektörler `upgrade_active_index_semantics`
    // ile arka planda doldurulur.
    use std::sync::Arc;
    let graph_arc = Arc::new(tokio::sync::RwLock::new(graph));
    match mode {
        IndexMode::Full => {
            if stats.nodes_created > 0 {
                let engine = RetrievalEngine::new(graph_arc.clone(), store);
                match engine.index_graph().await {
                    Ok(counts) => {
                        manifest.embedding =
                            written_embedding_identity(&engine.vector_store).await?;
                        stats.embedded_chunks = counts.embedded;
                        stats.reused_chunks = counts.reused;
                        info!(
                            nodes = stats.nodes_created,
                            files = stats.files_indexed,
                            embedded_chunks = counts.embedded,
                            "Indexing completed successfully"
                        )
                    }
                    // Graf-öncelikli: embedding servisi yokken graf araçları yine
                    // kullanılabilir olmalı. Eksik vektörler sonraki update_index'te
                    // (vector health kontrolü) onarılır.
                    Err(error) if crate::vector::remote::is_embedder_unavailable(&error) => {
                        warn!(
                            error = %error,
                            "Embedding service unreachable; activating a graph-only index"
                        );
                        stats.semantic_unavailable = Some(error.to_string());
                    }
                    Err(error) => return Err(error),
                }
            } else {
                warn!("No supported files found to index");
            }
        }
        IndexMode::Quick => {
            info!(
                nodes = stats.nodes_created,
                files = stats.files_indexed,
                "Quick (graph-only) indexing completed; semantic upgrade deferred"
            );
        }
    }

    // Boş sonuç da kalıcılaştırılır; aksi halde önceki dolu graph diskte kalır.
    // Manifest boş graf için de yazılır; artımlı indeksleme ona dayanır.
    let graph_path = artifact_parent.join("ccm_graph.json");
    let manifest_path = artifact_parent.join("ccm_manifest.json");
    manifest.schema_version = INDEX_SCHEMA_VERSION;
    manifest.indexed_commit = current_head_oid(&project_root);
    manifest.indexed_at = Some(snapshot_started_at);
    save_graph_with_manifest(
        &*graph_arc.read().await,
        &graph_path,
        manifest,
        &manifest_path,
    )?;
    info!(path = %graph_path.display(), "Graph saved to disk");

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

/// Etkinleştirme kilidi: artefakt dizinindeki kilit dosyası üzerinde işletim
/// sisteminin danışma (advisory) kilidi (Unix'te `flock`, Windows'ta `LockFileEx`).
/// Kilit, tutan dosya tanıtıcısı kapanınca ya da sahibi süreç ölünce işletim
/// sistemi tarafından bırakılır; bayatlık süresi, kalp atışı ya da sahip belirteci
/// gerekmez. İki bekleyen aynı ölü kilidi birlikte "kırıp" içeri giremez: kilidi
/// her zaman tek bir tanıtıcı alır. Kilit dosyası bırakırken silinmez; başka bir
/// süreç dosyayı açık tutarken silmek güvensizdir (sonraki alıcı yeni bir dosyayı
/// kilitleyip eskisini bekleyenle aynı anda içeri girebilir).
struct ActivationLock {
    path: PathBuf,
    /// Kilidi tutan tanıtıcı; Drop kilidi bırakır, ardından tanıtıcı kapanır.
    file: std::fs::File,
}

/// Etkinleştirme kilidinin bekleme zamanlamaları.
struct LockTiming {
    /// Kilit için en uzun bekleme.
    wait_limit: std::time::Duration,
    /// Kilit doluyken iki deneme arasındaki bekleme.
    poll_interval: std::time::Duration,
}

const ACTIVATION_LOCK_TIMING: LockTiming = LockTiming {
    wait_limit: std::time::Duration::from_secs(60),
    poll_interval: std::time::Duration::from_millis(25),
};

impl ActivationLock {
    fn acquire(artifact_parent: &Path) -> Result<Self> {
        Self::acquire_with(artifact_parent, &ACTIVATION_LOCK_TIMING)
    }

    /// Kilit dosyasını açar (yoksa oluşturur) ve kilidi `wait_limit` dolana kadar
    /// `try_lock` ile dener. Her çağrı kendi tanıtıcısını açar; işletim sistemi
    /// aynı süreçteki iki tanıtıcıyı da birbirine karşı dışlar, bu yüzden aynı
    /// sürecin görevleri ve iş parçacıkları da birbirini bekler.
    fn acquire_with(artifact_parent: &Path, timing: &LockTiming) -> Result<Self> {
        let path = artifact_parent.join(ACTIVATION_LOCK_FILE);
        // Kilit için yazma erişimi gerekir (Windows, NFS); içeriğe hiç yazılmadığı
        // için kırpma kapalıdır.
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|error| {
                anyhow::anyhow!(
                    "Index activation lock file '{}' could not be opened: {}",
                    path.display(),
                    error
                )
            })?;
        let started = std::time::Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(Self { path, file }),
                Err(std::fs::TryLockError::WouldBlock) => {
                    if started.elapsed() >= timing.wait_limit {
                        anyhow::bail!(
                            "Timed out after {:?} waiting for index activation lock '{}' held by another process or task",
                            timing.wait_limit,
                            path.display()
                        );
                    }
                    std::thread::sleep(timing.poll_interval);
                }
                Err(std::fs::TryLockError::Error(error)) => {
                    return Err(anyhow::anyhow!(
                        "Index activation lock '{}' could not be acquired: {} (the filesystem holding the index must support file locks)",
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
        // Windows'ta tanıtıcı kapanırken kilit bırakma gecikebilir; bu yüzden kilit
        // açıkça bırakılır. Bırakma başarısız olsa bile tanıtıcı hemen ardından
        // kapanır ve kilidi işletim sistemi bırakır.
        if let Err(error) = self.file.unlock() {
            tracing::warn!(path = %self.path.display(), error = %error, "Index activation lock could not be released explicitly; closing its handle releases it");
        }
    }
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

fn resolve_requested_db_path(project_root: &Path, db_path: Option<&str>) -> Result<PathBuf> {
    let path = match db_path.map(PathBuf::from) {
        Some(path) if path.is_absolute() => path,
        Some(path) => project_root.join(path),
        None => project_root.join("data/ccm_db"),
    };
    // v0.3.9 kapsama güvencesi: çözülen yol proje kökü dışına kaçarsa
    // (ör. `data` → dış dizin symlink'i, relative `../escape` db-path)
    // hata sessizce yutulmaz; çağıran index'i iptal eder. Fallback yoktur:
    // lexical `starts_with` kontrolü `data` symlink'i kök dışına çözüldüğünde
    // yanlış pozitif üretir ve yazımlar symlink'i izleyerek kök dışına gider.
    // `resolve_artifact_path` var-olmayan iç yolları da doğru çözer, bu yüzden
    // hata doğrudan yükseltilir.
    resolve_artifact_path(project_root, &path)
}

/// Bir artifact yolunu symlink-güvenli biçimde çözer.
///
/// Yolun kendisi henüz var olmayabilir (ilk index), bu yüzden en derin MEVCUT
/// atası `canonicalize` ile çözülür, kalan bileşenler eklenir ve sonucun
/// canonical proje kökü içinde kaldığı doğrulanır. Aksi halde proje içindeki
/// `data` → dış dizin symlink'i tüm index artifact'lerini allowlist dışına
/// yazabilir.
pub fn resolve_artifact_path(root: &Path, path: &Path) -> Result<PathBuf> {
    let canonical_root =
        std::fs::canonicalize(root).unwrap_or_else(|_| normalize_path_lexically(root));
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        canonical_root.join(path)
    };

    // En derin mevcut atayı bul; var olmayan bileşenleri kuyruğa al.
    let mut pending: Vec<std::ffi::OsString> = Vec::new();
    let mut current = absolute.as_path();
    let resolved = loop {
        match std::fs::canonicalize(current) {
            Ok(base) => break base,
            Err(_) => {
                let Some(name) = current.file_name() else {
                    return Err(anyhow::anyhow!(
                        "Cannot resolve artifact path '{}' under '{}'",
                        path.display(),
                        root.display()
                    ));
                };
                pending.push(name.to_os_string());
                let Some(parent) = current.parent() else {
                    return Err(anyhow::anyhow!(
                        "Cannot resolve artifact path '{}' under '{}'",
                        path.display(),
                        root.display()
                    ));
                };
                current = parent;
            }
        }
    };

    // Kalan bileşenleri uygula; `..` gezinmesini lexical olarak temizle.
    let mut result = resolved;
    for component in pending.iter().rev() {
        let component = Path::new(component);
        match component.components().next() {
            Some(std::path::Component::ParentDir) => {
                result.pop();
            }
            Some(std::path::Component::CurDir) => {}
            _ => result.push(component),
        }
    }

    if result.starts_with(&canonical_root) {
        Ok(result)
    } else {
        Err(anyhow::anyhow!(
            "Resolved artifact path '{}' escapes the project root '{}'",
            result.display(),
            canonical_root.display()
        ))
    }
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

    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?
    };
    #[cfg(not(unix))]
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

/// İndeksin kendi yazdığı artefaktlar. Manifest taraması ve dosya izleyici
/// bunları proje dosyası saymaz; aksi halde her yenileme kendini tetikler.
fn index_artifact_paths(artifact_parent: &Path, requested_db_path: &Path) -> Vec<PathBuf> {
    vec![
        requested_db_path.to_path_buf(),
        artifact_parent.join("ccm_graph.json"),
        artifact_parent.join("ccm_manifest.json"),
        artifact_parent.join(CURRENT_GENERATION_FILE),
        artifact_parent.join(GENERATIONS_DIRECTORY),
        artifact_parent.join(ACTIVATION_LOCK_FILE),
        artifact_parent.join(LEGACY_ACTIVATION_LOCK_DIRECTORY),
    ]
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
    let requested_db_path = resolve_requested_db_path(&project_root, db_path)?;
    record_git_excludes(&project_root);
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

    if !active.graph_path.exists() || !active.manifest_path.exists() || !active.db_path.is_dir() {
        info!(
            graph = %active.graph_path.display(),
            vector_db = %active.db_path.display(),
            "Index artifacts are incomplete. Performing full index."
        );
        return index_directory(path, db_path).await;
    }

    // Manifest graftan önce okunur: değişiklik yoksa graf hiç yüklenmez. Canlı
    // yenileme grafı manifestten önce yazdığı için bu sıra, eşzamanlı bir yazımda
    // manifestin graftan yeni görülmesini de önler.
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
        &index_artifact_paths(artifact_parent, &requested_db_path),
        &manifest,
    )?;
    let (changed_rel, deleted_rel) = diff_manifest(&manifest, &new_manifest);
    let unchanged = changed_rel.is_empty() && deleted_rel.is_empty();
    let source = EmbeddingSource::from_env()?;
    if unchanged {
        if let Some(counts) = recorded_semantic_nodes(&manifest, &active.graph_path) {
            let health = vector_index_health(
                &active,
                &requested_db_path,
                embedded_node_count(counts),
                &source,
                manifest.embedding.as_ref(),
            )
            .await;
            return finish_unchanged_index(path, db_path, health).await;
        }
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
    if graph_uses_legacy_paths(&graph) {
        info!("Legacy index detected. Performing full re-index.");
        return index_directory(path, db_path).await;
    }

    let health = vector_index_health(
        &active,
        &requested_db_path,
        semantic_node_count(&graph),
        &source,
        manifest.embedding.as_ref(),
    )
    .await;
    if unchanged {
        return finish_unchanged_index(path, db_path, health).await;
    }

    let changed_files: Vec<PathBuf> = changed_rel
        .iter()
        .chain(deleted_rel.iter())
        .map(|rel| file_id_to_path(&project_root, rel))
        .collect();

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

    // Değişen dosyalarla birlikte bozuk vektör tablosu varsa incremental yol yeni
    // node'ları eklerse de eski node vektörleri eksik kalır. Bu durumda tam yeniden
    // indeksleme hem parse hem embedding'i tek geçişte doğru biçimde tamamlar.
    if health.repair_needed() {
        tracing::warn!(
            vector_table = %health.table_path.display(),
            embedding_change = health.identity_mismatch.as_ref().map(ToString::to_string),
            "Vector index incomplete or built with another embedding model, with pending source changes; performing full re-index."
        );
        return index_directory(path, db_path).await;
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
        {
            // MCP'nin canlı yenilemesi etkin vektör tablosunu yerinde ve aynı kilit
            // altında değiştirir; kopya bu yüzden tutarlı bir tablo sürümü görür.
            let _activation_lock = ActivationLock::acquire(artifact_parent)?;
            let current = read_current_pointer_value(artifact_parent)?;
            if current != active.generation_id {
                anyhow::bail!(
                    "Index changed concurrently while an update was starting (expected {:?}, found {:?}); retry the update",
                    active.generation_id,
                    current
                );
            }
            copy_directory(&active.db_path, &staged_db_path)?;
        }
        // Graf bu fonksiyonun başında aktif generation'dan zaten yüklendi; JSON'u
        // kopyalayıp yeniden ayrıştırmak büyük repolarda ~0,25 sn sürer. Graf ve
        // manifest aşağıda staging'e yeniden yazılır.
        let staged_graph = graph;
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
        let mut committed_manifest =
            restore_retry_files(&manifest, new_manifest, &stats.retry_files);
        // Embedding kapalıyken tablodaki eski vektörler olduğu gibi kalır ve
        // kimlikleri korunur; açıkken tablo yapılandırılmış kaynağın kimliğini
        // taşır (kimliksiz eski indeks bu noktada kimlik kazanır).
        if source != EmbeddingSource::Disabled {
            committed_manifest.embedding =
                written_embedding_identity(&engine.vector_store).await?;
        }

        save_graph_with_manifest(
            &*graph_arc.read().await,
            &staged_graph_path,
            committed_manifest,
            &staged_manifest_path,
        )?;
        Ok(stats)
    }
    .await;

    let stats = match staged_result {
        Ok(stats) => stats,
        Err(error) => {
            let _ = std::fs::remove_dir_all(&staging_root);
            if crate::vector::remote::is_embedder_unavailable(&error) {
                // Artımlı embedding yapılamadı; graf değişikliklerini kaybetmemek için
                // tam yeniden indeksleme graf-yalnız generation'ı aktive eder.
                tracing::warn!(error = %error, "Embedding service unreachable during incremental update; rebuilding graph-only");
                return index_directory(path, db_path).await;
            }
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

/// Graf eski sürümlerin dosya yolu biçimini (mutlak ya da `./` öneksiz) taşıyor
/// mu? Böyle bir indeks artımlı güncellenemez; tam yeniden indeksle taşınır.
fn graph_uses_legacy_paths(graph: &CodeGraph) -> bool {
    graph.graph.node_weights().any(|node| {
        if !matches!(
            node.node_type,
            crate::graph::NodeType::File | crate::graph::NodeType::DataFile
        ) {
            return false;
        }
        let name = node.name.as_str();
        Path::new(name).is_absolute() || !name.starts_with("./")
    })
}

/// Etkin generation'ın vektör tablosunun durumu.
struct VectorHealth {
    /// Embedding açık ve embed edilecek düğüm varsa tablo gereklidir.
    required: bool,
    /// Tablo okunabiliyor ve en az embed edilecek düğüm kadar satır içeriyor mu.
    complete: Result<bool>,
    /// Tablodaki vektörler yapılandırılmış embedder'dan farklı bir kaynaktan
    /// geliyorsa nedeni; vektörler karıştırılmaz, hepsi bir kez yeniden embed edilir.
    identity_mismatch: Option<EmbeddingIdentityMismatch>,
    table_path: PathBuf,
}

impl VectorHealth {
    fn repair_needed(&self) -> bool {
        self.required && (self.identity_mismatch.is_some() || !matches!(self.complete, Ok(true)))
    }
}

async fn vector_index_health(
    active: &IndexArtifactPaths,
    requested_db_path: &Path,
    semantic_nodes: usize,
    source: &EmbeddingSource,
    recorded: Option<&EmbeddingIdentity>,
) -> VectorHealth {
    let required = *source != EmbeddingSource::Disabled && semantic_nodes > 0;
    let identity_mismatch = if required {
        source.mismatch_with(recorded)
    } else {
        None
    };
    let table_path = active.db_path.join("code_vectors.lance");
    let complete = if !required {
        Ok(true)
    } else if !table_path.exists() {
        Ok(false)
    } else {
        match LanceDbStore::new_with_fixture_namespace(
            &active.db_path.to_string_lossy(),
            "code_vectors",
            Some(&fixture_namespace_for_db(requested_db_path)),
        )
        .await
        {
            Ok(store) => store
                .validate_table()
                .await
                .map(|rows| rows >= semantic_nodes),
            Err(error) => Err(error),
        }
    };
    VectorHealth {
        required,
        complete,
        identity_mismatch,
        table_path,
    }
}

/// Kaynak değişikliği yokken `update_index`'in sonucu: yalnızca eksik ya da bozuk
/// vektör tablosu onarılır.
async fn finish_unchanged_index(
    path: &str,
    db_path: Option<&str>,
    health: VectorHealth,
) -> Result<IndexStats> {
    // Değişiklik yokken bile embedding erişilemezliği yüzünden graph-only
    // kalmış generation'ı onarmak gerekir; aksi halde semantic katman sessizce
    // eksik kalır ve `index_project` sahte "up to date" döner.
    if health.repair_needed() {
        tracing::warn!(
            vector_table = %health.table_path.display(),
            embedding_change = health.identity_mismatch.as_ref().map(ToString::to_string),
            error = ?health.complete.err(),
            "Vector index is incomplete, corrupt or built with another embedding model. Re-embedding from the active graph."
        );
        return match upgrade_active_index_semantics(path, db_path).await {
            // Graf zaten güncel; yalnızca semantik katman hâlâ beklemede.
            Err(error) if crate::vector::remote::is_embedder_unavailable(&error) => {
                Ok(IndexStats {
                    semantic_unavailable: Some(error.to_string()),
                    ..IndexStats::default()
                })
            }
            result => result,
        };
    }
    tracing::info!("No changes detected.");
    Ok(IndexStats::default())
}

/// Hazırlanamayan dosyaların (okuma/ayrıştırma hatası) önceki parmak izini geri
/// koyar; sonraki koşu onları yeniden dener. Böyle dosya varsa `indexed_at` ve
/// commit de eski değerlerine döner, racy pencere korunur ve eski dosyalar yeniden
/// hash'lenir.
fn restore_retry_files(
    previous: &IndexManifest,
    next: IndexManifest,
    retry_files: &[String],
) -> IndexManifest {
    if retry_files.is_empty() {
        return next;
    }
    let mut files = next.files;
    for path in retry_files {
        match previous.files.get(path) {
            Some(fingerprint) => {
                files.insert(path.clone(), fingerprint.clone());
            }
            None => {
                files.remove(path);
            }
        }
    }
    IndexManifest {
        indexed_commit: previous.indexed_commit.clone(),
        indexed_at: previous.indexed_at,
        files,
        ..next
    }
}

/// Aktif grafiği kullanarak eksik/eksik-semantik vektör tablosunu arka planda
/// yeniden kurar. Quick modda ya da embedding erişilemezken üretilmiş graph-only
/// indeksleri tek kaynaktan onarır; diskteki graph'i yeniden parse etmez, yalnızca
/// eksik vektörleri doldurur. Yeni bir staged jenerasyon üretip atomik aktifleştirir,
/// böylece devam eden sorgular tutarlı kalır.
pub async fn upgrade_active_index_semantics(
    path: &str,
    db_path: Option<&str>,
) -> Result<IndexStats> {
    use std::sync::Arc;
    use tracing::info;

    let project_root = std::fs::canonicalize(path).map_err(|error| {
        anyhow::anyhow!("Project root '{}' could not be resolved: {}", path, error)
    })?;
    let requested_db_path = resolve_requested_db_path(&project_root, db_path)?;
    let artifact_parent = requested_db_path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "Invalid DB path '{}': cannot determine parent directory",
            requested_db_path.display()
        )
    })?;
    let active = resolve_index_artifacts(path, db_path)?;

    if !active.graph_path.is_file() || !active.manifest_path.is_file() {
        anyhow::bail!(
            "Active index artifacts are incomplete; run a full index before semantic upgrade"
        );
    }

    let activation_generation = read_current_pointer_value(artifact_parent)?;
    let generation_id = new_generation_id();
    let generations_root = artifact_parent.join(GENERATIONS_DIRECTORY);
    std::fs::create_dir_all(&generations_root)?;
    let staging_root = generations_root.join(format!("{}.staging", generation_id));
    std::fs::create_dir_all(&staging_root)?;
    let staged_db_path = staging_root.join("ccm_db");
    let staged_graph_path = staging_root.join("ccm_graph.json");

    // Active generation'dan graph + manifest'i taşı; vektör staging'de sıfırdan kurulur.
    // MCP canlı yenilemesi etkin generation'a önce grafı sonra manifesti yazar;
    // manifest önce kopyalanınca kopyalanan manifest graftan yeni olamaz. Vektörler
    // kopyalanan graftan üretilir, böylece staging grafıyla birebir eşleşir.
    let staged_manifest_path = staging_root.join("ccm_manifest.json");
    let manifest = read_manifest(&active.manifest_path)?;
    std::fs::copy(&active.graph_path, &staged_graph_path)?;
    let graph = CodeGraph::from_file(&staged_graph_path.to_string_lossy())?;
    let semantic_nodes = semantic_node_count(&graph);
    if semantic_nodes == 0 {
        std::fs::remove_dir_all(&staging_root)?;
        info!("No semantic nodes; semantic upgrade has nothing to do");
        return Ok(IndexStats::default());
    }

    let fixture_namespace = fixture_namespace_for_db(&requested_db_path);
    let store = LanceDbStore::new_with_fixture_namespace(
        &staged_db_path.to_string_lossy(),
        "code_vectors",
        Some(&fixture_namespace),
    )
    .await?;
    // Quick/stale generation'da eski vektör tablosu bozuk olabilir; sıfırdan
    // açık bir tabloyla başlansa da reset idempotent olarak çağrılır.
    store.reset_table().await?;

    let graph_arc = Arc::new(tokio::sync::RwLock::new(graph));
    let engine = RetrievalEngine::new(graph_arc.clone(), store);
    let counts = engine.index_graph().await?;
    // Kopyalanan graf ve onun manifesti değişmedi; yalnızca yeni vektörlerin
    // kimliği kaydedilir.
    let embedding = written_embedding_identity(&engine.vector_store).await?;
    save_manifest(
        &staged_manifest_path,
        &IndexManifest {
            embedding,
            ..manifest
        },
    )?;

    info!(
        nodes = semantic_nodes,
        "Semantic upgrade completed; activating generation"
    );
    install_staged_generation(
        artifact_parent,
        &staging_root,
        &generation_id,
        &activation_generation,
    )?;

    Ok(IndexStats {
        nodes_created: semantic_nodes,
        embedded_chunks: counts.embedded,
        reused_chunks: counts.reused,
        ..IndexStats::default()
    })
}

/// Vektör tablosundaki vektörlerin kimliği: yapılandırılmış kaynağın kimliği
/// ve tablonun boyutu. Tablo yoksa ya da embedding kapalıysa `None`.
async fn written_embedding_identity(store: &LanceDbStore) -> Result<Option<EmbeddingIdentity>> {
    match store.vector_dim().await? {
        Some(dim) => Ok(EmbeddingSource::from_env()?.identity(dim)),
        None => Ok(None),
    }
}

pub fn semantic_node_count(graph: &CodeGraph) -> usize {
    embedded_node_count(semantic_node_counts(graph))
}

/// Grafın embedding'e giren düğüm türlerini sayar.
fn semantic_node_counts(graph: &CodeGraph) -> SemanticNodeCounts {
    graph
        .graph
        .node_weights()
        .fold(SemanticNodeCounts::default(), |counts, node| {
            match node.node_type {
                crate::graph::NodeType::Function
                | crate::graph::NodeType::Method
                | crate::graph::NodeType::Class
                | crate::graph::NodeType::Struct => SemanticNodeCounts {
                    symbols: counts.symbols + 1,
                    ..counts
                },
                crate::graph::NodeType::DataFile => SemanticNodeCounts {
                    data_files: counts.data_files + 1,
                    ..counts
                },
                _ => counts,
            }
        })
}

/// Vektör tablosunda bulunması gereken düğüm sayısı; veri dosyaları yalnızca
/// `CCM_EMBED_DATA_FILES` açıkken sayılır.
fn embedded_node_count(counts: SemanticNodeCounts) -> usize {
    let embed_data_files = std::env::var("CCM_EMBED_DATA_FILES")
        .map(|value| matches!(value.to_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false);
    if embed_data_files {
        counts.symbols + counts.data_files
    } else {
        counts.symbols
    }
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
    /// Embedding servisine ulaşılamadığı için graf-yalnız aktive edildiyse nedeni.
    /// Graf araçları çalışır; semantik arama servis gelince yeniden indekslemeyle döner.
    #[serde(default)]
    pub semantic_unavailable: Option<String>,
    /// Bu koşuda embedding servisine gönderilen parça sayısı.
    #[serde(default)]
    pub embedded_chunks: usize,
    /// Metni değişmediği için mevcut vektörü yeniden kullanılan parça sayısı.
    #[serde(default)]
    pub reused_chunks: usize,
    #[serde(skip)]
    pub(crate) retry_files: Vec<String>,
}

enum PopulateFileError {
    Read(FileReadError),
    Parse(anyhow::Error),
    Extract(anyhow::Error),
}

/// Belirlenimci biçimde indekslenemez içerik hatası mı?
///
/// `TooLarge` (dosya her zaman sınırın üstünde) ve `BinaryNul` (gerçek binary
/// dosya) dosyanın kalıcı olarak indekslenemez olduğunu söyler; bu dosyalar
/// atlanıp uyarı kaydedilir ve full index diğer dosyalarla tamamlanır. Buna
/// karşılık non-UTF8 / parse / geçici IO hataları ya editörün yarım yazması
/// (transient) ya da gerçek bir sorun olabilir; v0.3.8 güvencesi gereği bu
/// durumlarda aktif index korunur.
fn populate_error_is_unindexable_content(error: &PopulateFileError) -> bool {
    matches!(
        error,
        PopulateFileError::Read(FileReadError::TooLarge { .. } | FileReadError::BinaryNul { .. })
    )
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

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
struct IndexManifest {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    indexed_commit: Option<String>,
    /// Taramanın başladığı an (unix saniye). Stat önbelleğinin racy penceresi
    /// ve MCP tazelik satırındaki indeks yaşı bu değere dayanır.
    #[serde(default)]
    indexed_at: Option<u64>,
    /// Manifestle birlikte yazılan grafın embedding düğümü sayıları; değişiklik
    /// yokken vektör sağlığı grafı yüklemeden denetlenir. Eski manifestlerde yok.
    #[serde(default)]
    semantic_nodes: Option<SemanticNodeCounts>,
    /// Manifestle birlikte yazılan graf dosyasının bayt uzunluğu. Diskteki graf
    /// bu uzunlukta değilse (bozulma, eski manifest) graf yüklenip doğrulanır.
    #[serde(default)]
    graph_bytes: Option<u64>,
    /// Vektör tablosundaki vektörleri üreten embedding kaynağı. Eski
    /// manifestlerde ve vektörsüz (graf-yalnız) indekslerde yok.
    #[serde(default)]
    embedding: Option<EmbeddingIdentity>,
    files: HashMap<String, FileFingerprint>,
}

/// Embedding'e giren düğüm sayıları. Veri dosyaları yalnızca
/// `CCM_EMBED_DATA_FILES` açıkken embed edildiği için ayrı tutulur; ortam
/// değişkeni okunduğu anda uygulanır.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct SemanticNodeCounts {
    /// Function, Method, Class ve Struct düğümleri.
    symbols: usize,
    /// DataFile düğümleri.
    data_files: usize,
}

/// Kaba zaman çözünürlüklü dosya sistemlerinde (FAT: 2 sn) aynı pencerede
/// yapılan düzenlemeler kaçmasın diye stat önbelleği bu aralığa giren
/// dosyaları yeniden hash'ler (git'in "racy clean" kuralı).
const RACY_WINDOW_SECS: u64 = 2;

/// Şu anki zamanı unix saniye olarak döndürür.
pub fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}

/// Manifestteki embedding kimliğini okur; alan ya da manifest yoksa (eski ya
/// da vektörsüz indeks) `None`.
pub fn read_index_embedding(manifest_path: &Path) -> Result<Option<EmbeddingIdentity>> {
    #[derive(Deserialize)]
    struct ManifestEmbedding {
        #[serde(default)]
        embedding: Option<EmbeddingIdentity>,
    }
    let bytes = match std::fs::read(manifest_path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(anyhow::anyhow!(
                "Index manifest '{}' could not be read: {}",
                manifest_path.display(),
                error
            ))
        }
    };
    let parsed: ManifestEmbedding = serde_json::from_slice(&bytes).map_err(|error| {
        anyhow::anyhow!(
            "Index manifest '{}' could not be parsed: {}",
            manifest_path.display(),
            error
        )
    })?;
    Ok(parsed.embedding)
}

/// Manifestteki indeksleme zamanını okur; alan yoksa (eski manifest) `None`.
/// Dosya listesi ayrıştırılmadan atlandığı için büyük manifestlerde de ucuzdur.
pub fn read_index_timestamp(manifest_path: &Path) -> Result<Option<u64>> {
    #[derive(Deserialize)]
    struct ManifestTimestamp {
        #[serde(default)]
        indexed_at: Option<u64>,
    }
    let file = std::fs::File::open(manifest_path).map_err(|error| {
        anyhow::anyhow!(
            "Index manifest '{}' could not be opened: {}",
            manifest_path.display(),
            error
        )
    })?;
    let parsed: ManifestTimestamp = serde_json::from_reader(std::io::BufReader::new(file))
        .map_err(|error| {
            anyhow::anyhow!(
                "Index manifest '{}' could not be parsed: {}",
                manifest_path.display(),
                error
            )
        })?;
    Ok(parsed.indexed_at)
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
    relative_file_id(root, &abs)
}

/// Kanonik kök altındaki kanonik yoldan dosya kimliği (`./göreli/yol`) üretir.
/// Proje tarayıcısı kanonik kökten başlar ve sembolik bağları izlemez; verdiği
/// yollar zaten kanoniktir, bu yüzden dosya başına `canonicalize` gerekmez.
fn relative_file_id(root: &Path, abs: &Path) -> Option<String> {
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
    // Proje kökü dışındaki yollar indexlenemez; politika göreli yola uygulanır.
    let Some(file_id) = normalize_file_id(project_root, path) else {
        return false;
    };
    if path_is_policy_excluded(Path::new(&file_id)) || is_internal_index_file(&file_id) {
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

/// Proje köküne göre göreli dizin, tam taramanın indiği bir dizin mi? Tarayıcı
/// dışlanan dizin adlarına ve indeks hazırlama dizinlerine inmez (bkz.
/// `should_traverse_entry`); yolun hiçbir bileşeni bunlardan biri olmamalı.
fn is_index_relevant_dir(relative: &Path) -> bool {
    relative.components().all(|component| {
        let name = component.as_os_str().to_string_lossy();
        !EXCLUDED_DIRECTORY_NAMES.contains(&name.as_ref()) && !is_index_staging_dir_name(&name)
    })
}

fn file_id_to_path(project_root: &Path, file_id: &str) -> PathBuf {
    let rel = file_id.trim_start_matches("./");
    project_root.join(rel)
}

/// Metadata'dan mtime'ı (saniye, nanosaniye) çıkarır. Stat önbelleği ve
/// fingerprint karşılaştırmasında tutarlı hesaplama sağlar.
fn modified_parts(meta: &std::fs::Metadata) -> (u64, u32) {
    let modified = meta
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok());
    let modified_sec = modified.as_ref().map(|value| value.as_secs()).unwrap_or(0);
    let modified_nsec = modified
        .as_ref()
        .map(|value| value.subsec_nanos())
        .unwrap_or(0);
    (modified_sec, modified_nsec)
}

fn fingerprint_for_path(path: &Path) -> std::io::Result<FileFingerprint> {
    use std::io::Read;

    let mut file = std::fs::File::open(path)?;
    let meta = file.metadata()?;
    let (modified_sec, modified_nsec) = modified_parts(&meta);
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
        modified_sec,
        modified_nsec,
        size: meta.len(),
        content_hash,
    })
}

/// Önceki fingerprint'in stat bilgisi (mtime + boyut) değişmemişse ve dosya
/// racy pencerenin dışındaysa içerik hash'ini dosyayı okumadan yeniden
/// kullanır. `reuse_before_sec` 0 ise (zaman damgasız eski manifest) her dosya
/// yeniden hash'lenir.
fn fingerprint_reusing_previous(
    path: &Path,
    previous: Option<&FileFingerprint>,
    reuse_before_sec: u64,
) -> std::io::Result<FileFingerprint> {
    let Some(previous) = previous else {
        return fingerprint_for_path(path);
    };
    let meta = std::fs::metadata(path)?;
    let (modified_sec, modified_nsec) = modified_parts(&meta);
    let stat_unchanged = previous.modified_sec == modified_sec
        && previous.modified_nsec == modified_nsec
        && previous.size == meta.len();
    if stat_unchanged && modified_sec < reuse_before_sec {
        return Ok(previous.clone());
    }
    fingerprint_for_path(path)
}

/// Manifesti okur; okunamazsa boş manifest döner ve şema sürümü 0 tam yeniden
/// indekslemeyi tetikler.
fn load_manifest(path: &Path) -> IndexManifest {
    match read_manifest(path) {
        Ok(manifest) => manifest,
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                error = %error,
                "Index manifest is unreadable; the index is treated as outdated"
            );
            IndexManifest::default()
        }
    }
}

/// Manifesti tek seferde okuyup ayrıştırır. `serde_json::from_reader` okuyucuyu
/// tamponlamaz; dosyadan bayt bayt okumak büyük manifestlerde yüzlerce
/// milisaniyelik sistem çağrısı demektir.
fn read_manifest(path: &Path) -> Result<IndexManifest> {
    let bytes = std::fs::read(path).map_err(|error| {
        anyhow::anyhow!(
            "Index manifest '{}' could not be read: {}",
            path.display(),
            error
        )
    })?;
    serde_json::from_slice(&bytes).map_err(|error| {
        anyhow::anyhow!(
            "Index manifest '{}' could not be parsed: {}",
            path.display(),
            error
        )
    })
}

/// Grafı ve ona ait manifesti yazar; manifest grafın düğüm sayılarını ve bayt
/// uzunluğunu taşır. Graf önce yazılır: okuyucular manifesti graftan önce
/// okuduğu için eşzamanlı bir okuma manifesti graftan yeni göremez (graf yeniyse
/// değişiklikler yeniden uygulanır, bu idempotenttir).
fn save_graph_with_manifest(
    graph: &CodeGraph,
    graph_path: &Path,
    manifest: IndexManifest,
    manifest_path: &Path,
) -> Result<()> {
    graph.save_to_file(&graph_path.to_string_lossy())?;
    let graph_bytes = std::fs::metadata(graph_path)
        .map_err(|error| {
            anyhow::anyhow!(
                "Saved graph '{}' could not be inspected: {}",
                graph_path.display(),
                error
            )
        })?
        .len();
    let manifest = IndexManifest {
        semantic_nodes: Some(semantic_node_counts(graph)),
        graph_bytes: Some(graph_bytes),
        ..manifest
    };
    save_manifest(manifest_path, &manifest)
}

/// Manifestin kaydettiği graf olgularını, diskteki graf dosyası manifestle
/// birlikte yazılan dosyaysa döndürür (uzunluk eşleşmesi). Eski manifestlerde ya
/// da uzunluk farkında (bozulma, yarım kalan eşzamanlı yazım) `None` döner ve
/// çağıran grafı yükleyip doğrular.
fn recorded_semantic_nodes(
    manifest: &IndexManifest,
    graph_path: &Path,
) -> Option<SemanticNodeCounts> {
    let recorded_bytes = manifest.graph_bytes?;
    let actual_bytes = std::fs::metadata(graph_path).ok()?.len();
    if recorded_bytes == actual_bytes {
        manifest.semantic_nodes
    } else {
        None
    }
}

fn save_manifest(path: &Path, manifest: &IndexManifest) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp_path = artifact_temp_path(path);
    write_manifest_file(&temp_path, manifest)?;
    std::fs::rename(&temp_path, path)?;
    Ok(())
}

/// Artefaktın atomik yazımda kullanılan geçici dosya yolu
/// (`<ad>.json.<pid>.tmp`; `is_index_artifact_temp_name` bu deseni tanır).
fn artifact_temp_path(path: &Path) -> PathBuf {
    path.with_extension(format!("json.{}.tmp", std::process::id()))
}

/// Manifesti verilen dosyaya yazar ve diske senkronlar; atomik değiştirme
/// çağırana aittir.
fn write_manifest_file(path: &Path, manifest: &IndexManifest) -> Result<()> {
    #[cfg(unix)]
    let file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?
    };
    #[cfg(not(unix))]
    let file = std::fs::File::create(path)?;
    let mut writer = std::io::BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, manifest)?;
    use std::io::Write;
    writer.flush()?;
    writer.get_ref().sync_all()?;
    Ok(())
}

fn build_manifest(
    project_root: &Path,
    excluded_paths: &[PathBuf],
    previous: &IndexManifest,
) -> Result<IndexManifest> {
    // Zaman damgası tarama başlamadan alınır; tarama sırasında değişen
    // dosyalar bir sonraki koşuda racy pencereye düşer ve yeniden hash'lenir.
    let indexed_at = unix_now_secs();
    let files = fingerprint_walk(
        build_project_walker(project_root, excluded_paths),
        project_root,
        previous,
    )?;
    Ok(IndexManifest {
        schema_version: INDEX_SCHEMA_VERSION,
        indexed_commit: current_head_oid(project_root),
        indexed_at: Some(indexed_at),
        semantic_nodes: None,
        graph_bytes: None,
        // Tarama vektör tablosunu değiştirmez; tablonun kimliği aynen taşınır.
        embedding: previous.embedding.clone(),
        files,
    })
}

/// Manifestin yalnızca `scope` yollarına düşen dosyalarını tam taramanın
/// kurallarıyla tarar (bkz. `build_scoped_project_walker`).
fn scan_manifest_scope(
    project_root: &Path,
    excluded_paths: &[PathBuf],
    previous: &IndexManifest,
    scope: &[PathBuf],
) -> Result<HashMap<String, FileFingerprint>> {
    fingerprint_walk(
        build_scoped_project_walker(project_root, excluded_paths, scope.to_vec()),
        project_root,
        previous,
    )
}

/// Tarayıcının verdiği indekslenebilir dosyaların parmak izleri. Stat bilgisi
/// önceki manifestle aynı ve racy pencerenin dışındaysa hash yeniden kullanılır.
fn fingerprint_walk(
    walker: ignore::Walk,
    project_root: &Path,
    previous: &IndexManifest,
) -> Result<HashMap<String, FileFingerprint>> {
    let reuse_before_sec = racy_reuse_boundary(previous);
    let mut files = HashMap::new();
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
        let Some(file_id) = relative_file_id(project_root, file_path) else {
            continue;
        };
        if is_internal_index_file(&file_id) {
            continue;
        }
        if path_is_policy_excluded(Path::new(&file_id)) {
            continue;
        }

        let fingerprint =
            fingerprint_reusing_previous(file_path, previous.files.get(&file_id), reuse_before_sec)
                .map_err(|error| {
                    anyhow::anyhow!(
                        "Project snapshot could not read '{}': {}. Existing index was preserved.",
                        file_path.display(),
                        error
                    )
                })?;
        files.insert(file_id, fingerprint);
    }
    Ok(files)
}

/// Stat önbelleğinin güvendiği mtime sınırı: bundan önce değişmiş ve stat bilgisi
/// aynı kalmış dosyanın hash'i yeniden kullanılır. Manifestteki her parmak izi ya
/// bu sınırdan güvenli ya da `indexed_at`'ten sonra hesaplanmıştır. Zaman
/// damgasız eski manifestte 0 döner; her dosya yeniden hash'lenir.
fn racy_reuse_boundary(previous: &IndexManifest) -> u64 {
    previous
        .indexed_at
        .map(|value| value.saturating_sub(RACY_WINDOW_SECS))
        .unwrap_or(0)
}

/// Kapsamlı taramayı manifestin aynı kapsamdaki eski girdileriyle karşılaştırır;
/// `diff_manifest` kuralının kapsamla sınırlı hâli.
fn diff_manifest_scope(
    previous: &IndexManifest,
    scope_ids: &[String],
    scanned: &HashMap<String, FileFingerprint>,
) -> (Vec<String>, Vec<String>) {
    let changed = scanned
        .iter()
        .filter(|(file_id, fingerprint)| previous.files.get(*file_id) != Some(*fingerprint))
        .map(|(file_id, _)| file_id.clone())
        .collect();
    let deleted = previous
        .files
        .keys()
        .filter(|file_id| file_id_in_scope(file_id, scope_ids) && !scanned.contains_key(*file_id))
        .cloned()
        .collect();
    (changed, deleted)
}

/// Dosya kimliği kapsam kimliklerinden birine eşit ya da onun altında mı?
fn file_id_in_scope(file_id: &str, scope_ids: &[String]) -> bool {
    scope_ids.iter().any(|scope| {
        file_id
            .strip_prefix(scope.as_str())
            .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    })
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

/// `file_path` proje köküne göreli olmalıdır (ör. `./src/main.rs`); mutlak yol
/// verilirse proje kökünün üst dizinleri de ("/build/app") politikaya takılır.
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

/// İndeks artefaktlarını projeyi içeren git deposunun `info/exclude` dosyasına
/// ekler; kullanıcının `git status`'u ve `.gitignore`'u kirlenmez. Yalnızca
/// eksik desenler eklenir (idempotent). Depo yoksa hiçbir şey yapılmaz.
fn ensure_git_excludes(project_root: &Path) -> Result<()> {
    let Ok(repo) = git2::Repository::discover(project_root) else {
        return Ok(());
    };
    let Some(workdir) = repo.workdir() else {
        return Ok(());
    };
    let workdir = std::fs::canonicalize(workdir)?;
    let Ok(relative_root) = project_root.strip_prefix(&workdir) else {
        return Ok(());
    };
    let prefix = relative_root.to_string_lossy().replace('\\', "/");
    let anchor = if prefix.is_empty() {
        "/".to_string()
    } else {
        format!("/{prefix}/")
    };
    let patterns: Vec<String> = ["data/ccm_*", "data/.ccm-*", ".ccm/"]
        .iter()
        .map(|pattern| format!("{anchor}{pattern}"))
        .collect();

    let exclude_path = repo.commondir().join("info").join("exclude");
    let existing = match std::fs::read_to_string(&exclude_path) {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let missing = missing_exclude_patterns(&existing, &patterns);
    if missing.is_empty() {
        return Ok(());
    }
    if let Some(parent) = exclude_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut addition = String::new();
    if !existing.is_empty() && !existing.ends_with('\n') {
        addition.push('\n');
    }
    addition.push_str("# CCM index artifacts\n");
    for pattern in missing {
        addition.push_str(&pattern);
        addition.push('\n');
    }
    use std::io::Write;
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&exclude_path)?
        .write_all(addition.as_bytes())?;
    Ok(())
}

fn missing_exclude_patterns(existing: &str, patterns: &[String]) -> Vec<String> {
    let present: HashSet<&str> = existing.lines().map(str::trim).collect();
    patterns
        .iter()
        .filter(|pattern| !present.contains(pattern.as_str()))
        .cloned()
        .collect()
}

/// Exclude yazımı indekslemenin ana işi değildir; başarısızlık görünür biçimde
/// loglanır ama indeksi durdurmaz.
fn record_git_excludes(project_root: &Path) {
    if let Err(error) = ensure_git_excludes(project_root) {
        tracing::warn!(project = %project_root.display(), error = %error, "Could not add CCM artifacts to .git/info/exclude");
    }
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

/// Aynı içerik her denemede aynı sonucu verir (çok büyük, ikili): dosya
/// değişene kadar yeniden denenmez, atlanmış sayılır. G/Ç hataları ve geçersiz
/// UTF-8 (yazımın ortasında okunan dosya) geçicidir, yeniden denenir.
pub(crate) fn is_permanent_issue(reason: &IndexIssueReason) -> bool {
    matches!(
        reason,
        IndexIssueReason::FileTooLarge | IndexIssueReason::BinaryFile
    )
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
            indexed_at: None,
            semantic_nodes: None,
            graph_bytes: None,
            embedding: None,
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
            indexed_at: None,
            semantic_nodes: None,
            graph_bytes: None,
            embedding: None,
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

#[cfg(test)]
mod activation_lock_tests {
    use super::{
        ActivationLock, LockTiming, ACTIVATION_LOCK_FILE, LEGACY_ACTIVATION_LOCK_DIRECTORY,
    };
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, Stdio};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    /// Yardımcı sürece kilidin alınacağı dizini bildiren ortam değişkeni.
    const HOLDER_DIRECTORY_ENV: &str = "CCM_TEST_ACTIVATION_LOCK_HOLDER_DIR";
    /// Yardımcı süreç kilidi aldığında yazdığı işaret dosyası.
    const HOLDER_READY_FILE: &str = "holder-ready";

    fn timing(wait_limit: Duration) -> LockTiming {
        LockTiming {
            wait_limit,
            poll_interval: Duration::from_millis(2),
        }
    }

    #[test]
    fn contending_threads_are_never_inside_together() -> anyhow::Result<()> {
        const THREADS: usize = 4;
        const ROUNDS: usize = 25;
        let parent = tempfile::tempdir()?;
        let timing = timing(Duration::from_secs(60));
        let inside = AtomicUsize::new(0);
        let completed = AtomicUsize::new(0);

        std::thread::scope(|scope| -> anyhow::Result<()> {
            let workers: Vec<_> = (0..THREADS)
                .map(|_| {
                    scope.spawn(|| -> anyhow::Result<()> {
                        for _ in 0..ROUNDS {
                            let lock = ActivationLock::acquire_with(parent.path(), &timing)?;
                            // Kilit içindeyken başka bir sahip görülürse karşılıklı
                            // dışlama bozulmuştur.
                            let overlapped = inside.fetch_add(1, Ordering::SeqCst) != 0;
                            // İhlal edecek bir iş parçacığına içeri girme fırsatı ver.
                            std::thread::sleep(Duration::from_millis(1));
                            inside.fetch_sub(1, Ordering::SeqCst);
                            drop(lock);
                            anyhow::ensure!(
                                !overlapped,
                                "another holder was inside the activation lock at the same time"
                            );
                            completed.fetch_add(1, Ordering::SeqCst);
                        }
                        Ok(())
                    })
                })
                .collect();
            for worker in workers {
                worker
                    .join()
                    .map_err(|_| anyhow::anyhow!("a contending thread panicked"))??;
            }
            Ok(())
        })?;
        assert_eq!(completed.load(Ordering::SeqCst), THREADS * ROUNDS);
        Ok(())
    }

    #[test]
    fn acquire_gives_up_with_a_clear_error_when_the_lock_stays_held() -> anyhow::Result<()> {
        let parent = tempfile::tempdir()?;
        let lock_path = parent.path().join(ACTIVATION_LOCK_FILE);
        let held = ActivationLock::acquire_with(parent.path(), &timing(Duration::from_secs(5)))?;

        let wait_limit = Duration::from_millis(200);
        let started = Instant::now();
        let error = match ActivationLock::acquire_with(parent.path(), &timing(wait_limit)) {
            Ok(_) => anyhow::bail!("the lock was granted while another holder had it"),
            Err(error) => error,
        };
        let waited = started.elapsed();
        assert!(waited >= wait_limit, "gave up early, after {waited:?}");
        let message = error.to_string();
        assert!(
            message.contains("Timed out after 200ms waiting for index activation lock"),
            "{message}"
        );
        assert!(
            message.contains(&lock_path.display().to_string()),
            "{message}"
        );

        // Bırakılan kilit yeniden alınabilir; kilit dosyası yerinde kalır.
        drop(held);
        drop(ActivationLock::acquire_with(
            parent.path(),
            &timing(Duration::from_secs(5)),
        )?);
        assert!(lock_path.exists(), "the lock file must stay in place");
        Ok(())
    }

    #[test]
    fn a_leftover_lock_directory_from_older_versions_is_left_alone() -> anyhow::Result<()> {
        let parent = tempfile::tempdir()?;
        let legacy = parent.path().join(LEGACY_ACTIVATION_LOCK_DIRECTORY);
        std::fs::create_dir(&legacy)?;
        std::fs::write(legacy.join("owner"), "older-version-holder")?;

        let started = Instant::now();
        let lock = ActivationLock::acquire_with(parent.path(), &timing(Duration::from_secs(10)))?;
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "an inert leftover directory must not delay the lock"
        );
        drop(lock);

        assert_eq!(
            std::fs::read_to_string(legacy.join("owner"))?,
            "older-version-holder",
            "the leftover directory and its content must stay untouched"
        );
        Ok(())
    }

    #[test]
    fn a_holder_killed_without_dropping_does_not_block_the_next_acquirer() -> anyhow::Result<()> {
        let parent = tempfile::tempdir()?;
        let mut holder = spawn_holder(parent.path())?;
        wait_until_holder_is_ready(&mut holder, &parent.path().join(HOLDER_READY_FILE))?;

        // Sahip başka bir süreçte yaşarken kilit dolu.
        let blocked =
            ActivationLock::acquire_with(parent.path(), &timing(Duration::from_millis(300)));
        anyhow::ensure!(
            blocked.is_err(),
            "the lock was granted while another process held it"
        );

        // Sahip Drop çalıştırmadan ölür (SIGKILL / TerminateProcess); kilidi işletim
        // sistemi bırakır ve sonraki alıcı beklemeden içeri girer.
        holder.0.kill()?;
        holder.0.wait()?;
        let started = Instant::now();
        let lock = ActivationLock::acquire_with(parent.path(), &timing(Duration::from_secs(10)))?;
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "a dead holder's lock must not delay the next acquirer"
        );
        drop(lock);
        Ok(())
    }

    /// Alt süreç olarak çalıştırılan yardımcı: kilidi alır, hazır işaretini yazar ve
    /// öldürülene kadar bekler. Ortam değişkeni yoksa (normal test koşusu) hiçbir
    /// şey yapmaz.
    #[test]
    fn lock_holder_helper() -> anyhow::Result<()> {
        let Some(directory) = std::env::var_os(HOLDER_DIRECTORY_ENV) else {
            return Ok(());
        };
        let directory = PathBuf::from(directory);
        let _lock = ActivationLock::acquire_with(&directory, &timing(Duration::from_secs(30)))?;
        std::fs::write(directory.join(HOLDER_READY_FILE), "ready")?;
        // Sahip Drop çalıştırmadan öldürülür; bu bekleme yalnızca bir üst sınırdır.
        std::thread::sleep(Duration::from_secs(120));
        Ok(())
    }

    /// Test başarısız olsa bile yardımcı süreç geride kalmasın.
    struct HolderProcess(Child);

    impl Drop for HolderProcess {
        fn drop(&mut self) {
            // Süreç zaten bitmiş olabilir; temizlikte kill hatası önemsizdir.
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    /// Test ikilisini yalnızca `lock_holder_helper` testini çalıştıracak şekilde alt
    /// süreç olarak başlatır.
    fn spawn_holder(directory: &Path) -> anyhow::Result<HolderProcess> {
        let child = Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "activation_lock_tests::lock_holder_helper",
                "--nocapture",
            ])
            .env(HOLDER_DIRECTORY_ENV, directory)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()?;
        Ok(HolderProcess(child))
    }

    fn wait_until_holder_is_ready(
        holder: &mut HolderProcess,
        ready_file: &Path,
    ) -> anyhow::Result<()> {
        let deadline = Instant::now() + Duration::from_secs(60);
        while !ready_file.exists() {
            if let Some(status) = holder.0.try_wait()? {
                anyhow::bail!("the lock holder helper exited before taking the lock: {status}");
            }
            anyhow::ensure!(
                Instant::now() < deadline,
                "the lock holder helper never took the lock"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    }
}
