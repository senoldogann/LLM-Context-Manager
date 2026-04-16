pub mod engine;

pub mod eval;
mod fs_utils;
pub mod git;
pub mod graph;

pub mod parser;

pub mod storage;
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

    let db_path = std::path::Path::new(project_path).join("data/ccm_db");
    let db_path_str = db_path.to_string_lossy().to_string();

    // Check if DB exists
    if !db_path.exists() {
        return Err(anyhow::anyhow!(
            "Index not found. Please run indexing first."
        ));
    }

    // Load graph (for simplicity in this prototype refactor we create new,
    // but ideally we load from disk)
    let graph_path = std::path::Path::new(project_path).join("data/ccm_graph.json");

    let graph = if graph_path.exists() {
        // Load graph logic would go here
        // For now, we initialize empty and warn, as full persistence refactor is next
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

const MAX_INDEX_ISSUES_RECORDED: usize = 250;
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
    "mov", "avi", "bin",
];

fn build_project_walker(path: &Path) -> ignore::Walk {
    use ignore::WalkBuilder;

    WalkBuilder::new(path)
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .parents(true)
        .ignore(true)
        .add_custom_ignore_filename(".ccmignore")
        .filter_entry(should_traverse_entry)
        .build()
}

fn should_traverse_entry(entry: &ignore::DirEntry) -> bool {
    let Some(name) = entry.file_name().to_str() else {
        return true;
    };
    let file_type = entry.file_type();

    if file_type.map(|ft| ft.is_dir()).unwrap_or(false) {
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
    use tracing::{error, info, warn};

    // If db_path is provided, use it. Otherwise default to path/data/ccm_db
    let default_db_path = std::path::Path::new(path).join("data/ccm_db");
    let db_path_buf = db_path
        .map(std::path::PathBuf::from)
        .unwrap_or(default_db_path);
    let db_path_str = db_path_buf.to_string_lossy().to_string();
    let project_root = std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path));

    info!(path = path, db_path = %db_path_str, "Starting directory indexing");

    let mut graph = CodeGraph::new();
    let store = LanceDbStore::new(&db_path_str, "code_vectors").await?;
    store.reset_table().await?;

    let mut stats = IndexStats::default();
    let mut manifest = IndexManifest::default();

    let parent_dir = db_path_buf.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "Invalid DB path '{}': cannot determine parent directory",
            db_path_str
        )
    })?;

    // Create a walker that respects gitignore + ccmignore and skips heavy noise paths.
    let walker = build_project_walker(Path::new(path));

    // Walk directory recursively
    for result in walker {
        match result {
            Ok(entry) => {
                if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                    continue;
                }

                let file_path = entry.path();
                let file_path_str = file_path.to_string_lossy().to_string();
                let Some(file_id) = normalize_file_id(&project_root, file_path) else {
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

                if let Some(fp) = fingerprint_for_path(file_path) {
                    manifest.files.insert(file_id.clone(), fp);
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

                match populate_graph_for_file(&mut graph, file_path, &file_id) {
                    Ok(_) => {
                        stats.files_indexed += 1;
                    }
                    Err(e) => {
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

    // Count nodes
    stats.nodes_created = graph.graph.node_count();

    // Index into vector store
    if stats.nodes_created > 0 {
        use std::sync::Arc;
        let graph_arc = Arc::new(tokio::sync::RwLock::new(graph.clone())); // Still need clone because graph is used below for save_to_file
        let engine = RetrievalEngine::new(graph_arc, store);
        engine.index_graph().await?;

        info!(
            nodes = stats.nodes_created,
            files = stats.files_indexed,
            "Indexing completed successfully"
        );

        // PERSISTENCE: Save graph to disk
        let graph_path = parent_dir.join("ccm_graph.json");
        match graph.save_to_file(&graph_path.to_string_lossy()) {
            Ok(_) => info!(path = %graph_path.display(), "Graph saved to disk"),
            Err(e) => error!(error = %e, "Failed to save graph"),
        }
    } else {
        warn!("No supported files found to index");
    }

    // Save manifest for incremental indexing (even if no nodes were created).
    let manifest_path = parent_dir.join("ccm_manifest.json");
    if let Err(e) = save_manifest(&manifest_path, &manifest) {
        error!(error = %e, "Failed to save manifest");
    }

    Ok(stats)
}

/// Updates an existing index incrementally (using Git or filesystem snapshots).
/// If the index or graph does not exist, it falls back to a full index.
pub async fn update_index(path: &str, db_path: Option<&str>) -> Result<IndexStats> {
    use tracing::{info, warn};

    // Determine paths
    let default_db_path = std::path::Path::new(path).join("data/ccm_db");
    let db_path_buf = db_path
        .map(std::path::PathBuf::from)
        .unwrap_or(default_db_path);
    let db_path_str = db_path_buf.to_string_lossy().to_string();
    let project_root = std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path));

    let parent_dir = db_path_buf.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "Invalid DB path '{}': cannot determine parent directory",
            db_path_str
        )
    })?;

    let graph_path = parent_dir.join("ccm_graph.json");
    let manifest_path = parent_dir.join("ccm_manifest.json");

    if !graph_path.exists() {
        info!(
            "Graph not found at {}, performing full index",
            graph_path.display()
        );
        return index_directory(path, db_path).await;
    }

    // Load graph
    let graph = CodeGraph::from_file(&graph_path.to_string_lossy())?;
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
    let store = LanceDbStore::new(&db_path_str, "code_vectors").await?;
    let graph_arc = std::sync::Arc::new(tokio::sync::RwLock::new(graph));

    let engine = RetrievalEngine::new(graph_arc.clone(), store);

    let mut manifest = load_manifest(&manifest_path);
    let mut changed_files: Vec<PathBuf> = Vec::new();
    let mut used_manifest_diff = false;

    match crate::git::GitIntegrator::new(&project_root) {
        Ok(git) => match git.get_changed_files() {
            Ok(files) => {
                if files.is_empty() {
                    info!("No changes detected.");
                    if manifest.files.is_empty() {
                        manifest = build_manifest(&project_root)?;
                        if let Err(e) = save_manifest(&manifest_path, &manifest) {
                            warn!(error = %e, "Failed to save manifest");
                        }
                    }
                    return Ok(IndexStats::default());
                }
                changed_files = files;
            }
            Err(e) => {
                warn!(
                    "Git change detection failed: {}. Falling back to filesystem scan.",
                    e
                );
                used_manifest_diff = true;
            }
        },
        Err(e) => {
            warn!(
                "Git repository not detected: {}. Falling back to filesystem scan.",
                e
            );
            used_manifest_diff = true;
        }
    }

    if used_manifest_diff {
        let new_manifest = build_manifest(&project_root)?;
        let (changed_rel, deleted_rel) = diff_manifest(&manifest, &new_manifest);

        if changed_rel.is_empty() && deleted_rel.is_empty() {
            info!("No changes detected.");
            return Ok(IndexStats::default());
        }

        changed_files = changed_rel
            .iter()
            .chain(deleted_rel.iter())
            .map(|rel| file_id_to_path(&project_root, rel))
            .collect();

        manifest = new_manifest;
    }

    // Run incremental index
    info!("Starting incremental indexing for {}", path);
    let stats = engine.incremental_index_paths(path, &changed_files).await?;

    // Save graph back to disk
    let updated_graph = graph_arc.read().await;
    match updated_graph.save_to_file(&graph_path.to_string_lossy()) {
        Ok(_) => info!(path = %graph_path.display(), "Graph updated on disk"),
        Err(e) => warn!(error = %e, "Failed to save updated graph"),
    }

    // Update manifest
    if !used_manifest_diff {
        if manifest.files.is_empty() {
            manifest = build_manifest(&project_root)?;
        } else {
            update_manifest_for_paths(&mut manifest, &project_root, &changed_files);
        }
    }
    if let Err(e) = save_manifest(&manifest_path, &manifest) {
        warn!(error = %e, "Failed to save manifest");
    }

    Ok(stats)
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
            content: content.clone(),
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

    // PASS 1: Extract definitions (Files, Functions, Classes, etc.)
    let mut extractor = Extractor::new(content.clone(), lang);
    extractor
        .extract(&tree, graph, file_id)
        .map_err(PopulateFileError::Extract)?;

    // PASS 2: Extract references (Function Calls -> Calls edges)
    let edges_created = extractor
        .extract_references(&tree, graph, file_id)
        .map_err(PopulateFileError::Extract)?;
    if edges_created > 0 {
        tracing::debug!("Linked {} call edges", edges_created);
    }

    Ok(())
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct IndexManifest {
    files: HashMap<String, FileFingerprint>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct FileFingerprint {
    modified_sec: u64,
    size: u64,
}

pub(crate) fn normalize_file_id(project_root: &Path, path: &Path) -> Option<String> {
    let root = std::fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_path_buf());
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let abs = std::fs::canonicalize(&abs).unwrap_or(abs);
    let rel = abs.strip_prefix(&root).ok()?;
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

fn file_id_to_path(project_root: &Path, file_id: &str) -> PathBuf {
    let rel = file_id.trim_start_matches("./");
    project_root.join(rel)
}

fn fingerprint_for_path(path: &Path) -> Option<FileFingerprint> {
    let meta = std::fs::metadata(path).ok()?;
    let modified_sec = meta
        .modified()
        .ok()
        .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);
    Some(FileFingerprint {
        modified_sec,
        size: meta.len(),
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
    let file = std::fs::File::create(path)?;
    let writer = std::io::BufWriter::new(file);
    serde_json::to_writer_pretty(writer, manifest)?;
    Ok(())
}

fn build_manifest(project_root: &Path) -> Result<IndexManifest> {
    let mut manifest = IndexManifest::default();
    let walker = build_project_walker(project_root);

    for result in walker {
        let entry = match result {
            Ok(entry) => entry,
            Err(_) => continue,
        };

        if !entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
            continue;
        }

        let file_path = entry.path();
        let Some(file_id) = normalize_file_id(project_root, file_path) else {
            continue;
        };
        if is_internal_index_file(&file_id) {
            continue;
        }
        if path_is_policy_excluded(file_path) {
            continue;
        }

        if let Some(fp) = fingerprint_for_path(file_path) {
            manifest.files.insert(file_id, fp);
        }
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

fn update_manifest_for_paths(manifest: &mut IndexManifest, project_root: &Path, paths: &[PathBuf]) {
    for path in paths {
        let abs = if path.is_absolute() {
            path.clone()
        } else {
            project_root.join(path)
        };
        let Some(file_id) = normalize_file_id(project_root, &abs) else {
            continue;
        };
        if is_internal_index_file(&file_id) {
            manifest.files.remove(&file_id);
            continue;
        }

        if abs.exists() {
            if path_is_policy_excluded(&abs) {
                manifest.files.remove(&file_id);
                continue;
            }
            if let Some(fp) = fingerprint_for_path(&abs) {
                manifest.files.insert(file_id, fp);
            }
        } else {
            manifest.files.remove(&file_id);
        }
    }
}

pub(crate) fn path_is_policy_excluded(file_path: &Path) -> bool {
    for component in file_path.components() {
        let text = component.as_os_str().to_string_lossy().to_string();
        if EXCLUDED_DIRECTORY_NAMES.contains(&text.as_str()) {
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
        if stats.skipped_files.len() < MAX_INDEX_ISSUES_RECORDED {
            stats.skipped_files.push(issue);
        }
        return;
    }

    stats.files_failed += 1;
    if stats.failed_files.len() < MAX_INDEX_ISSUES_RECORDED {
        stats.failed_files.push(issue);
    }
}
