//! MCP Server request handling logic.

use anyhow::Result;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::protocol::{
    create_error_response, create_success_response, JsonRpcRequest, JsonRpcResponse,
    ResourcesCapability, ServerCapabilities, ServerInfo, ToolDefinition, ToolsCapability,
};
use crate::tools;

use ccm_core::engine::RetrievalEngine;
use ccm_core::graph::CodeGraph;
use ccm_core::vector::store::LanceDbStore;

const LATEST_PROTOCOL_VERSION: &str = "2025-11-25";
const SUPPORTED_PROTOCOL_VERSIONS: [&str; 3] =
    [LATEST_PROTOCOL_VERSION, "2025-06-18", "2025-03-26"];

/// Holds the server's shared state.
pub struct ServerState {
    pub default_engine: RwLock<Arc<RetrievalEngine>>,
    pub engines: RwLock<EngineCache>,
    index_locks:
        std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<tokio::sync::Mutex<()>>>>,
    pub(crate) index_jobs: std::sync::Mutex<std::collections::HashMap<String, IndexJob>>,
    pub(crate) next_index_job_id: std::sync::atomic::AtomicU64,
    default_project_root: Option<PathBuf>,
    default_db_path: PathBuf,
    allowed_roots: Vec<PathBuf>,
    require_allowed_roots: bool,
}

#[derive(Clone)]
pub(crate) struct IndexJob {
    pub id: u64,
    pub receiver: tokio::sync::watch::Receiver<Option<crate::protocol::ToolResult>>,
}

const DEFAULT_ENGINE_CACHE_SIZE: usize = 8;

fn engine_cache_size() -> usize {
    std::env::var("CCM_MCP_ENGINE_CACHE_SIZE")
        .ok()
        .and_then(|val| val.parse::<usize>().ok())
        .map(|val| val.max(1))
        .unwrap_or(DEFAULT_ENGINE_CACHE_SIZE)
}

use lru::LruCache;
use std::num::NonZeroUsize;

pub struct EngineCache {
    cache: LruCache<String, Arc<RetrievalEngine>>,
}

impl EngineCache {
    fn new(max: usize) -> Self {
        let cap = NonZeroUsize::new(max).unwrap_or_else(|| NonZeroUsize::new(1).unwrap());
        Self {
            cache: LruCache::new(cap),
        }
    }

    fn get(&mut self, key: &str) -> Option<Arc<RetrievalEngine>> {
        self.cache.get(key).cloned()
    }

    #[allow(dead_code)]
    fn peek(&self, key: &str) -> Option<Arc<RetrievalEngine>> {
        self.cache.peek(key).cloned()
    }

    fn insert(&mut self, key: String, engine: Arc<RetrievalEngine>) -> Arc<RetrievalEngine> {
        self.cache.put(key, engine.clone());
        engine
    }
}

impl ServerState {
    pub(crate) fn project_index_lock(
        &self,
        project_path: &str,
    ) -> std::sync::Arc<tokio::sync::Mutex<()>> {
        let canonical_path = canonicalize_project_path(Path::new(project_path));
        let cache_key = canonical_path.to_string_lossy().to_string();
        let mut locks = self.index_locks.lock().unwrap();
        locks
            .entry(cache_key)
            .or_insert_with(|| std::sync::Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    pub(crate) fn index_job_in_progress(&self, project_path: &str) -> bool {
        let canonical_path = canonicalize_project_path(Path::new(project_path));
        let cache_key = canonical_path.to_string_lossy().to_string();
        self.index_jobs
            .lock()
            .unwrap()
            .get(&cache_key)
            .is_some_and(|job| job.receiver.borrow().is_none())
    }

    pub(crate) fn remove_index_job_if_id(&self, job_key: &str, job_id: u64) {
        let mut jobs = self.index_jobs.lock().unwrap();
        if jobs.get(job_key).is_some_and(|job| job.id == job_id) {
            jobs.remove(job_key);
        }
    }

    pub(crate) fn release_index_lock(&self, job_key: &str) {
        self.index_locks.lock().unwrap().remove(job_key);
    }

    pub(crate) fn project_db_path(&self, project_path: &str) -> Result<PathBuf> {
        let canonical_path = canonicalize_project_path(Path::new(project_path));
        if self
            .default_project_root
            .as_ref()
            .is_some_and(|root| canonical_path == *root)
        {
            return Ok(self.default_db_path.clone());
        }
        let candidate = canonical_path.join("data/ccm_db");
        ccm_core::resolve_artifact_path(&canonical_path, &candidate)
    }

    fn project_artifacts(&self, project_path: &str) -> Result<ccm_core::IndexArtifactPaths> {
        let requested_db = self.project_db_path(project_path)?;
        ccm_core::resolve_index_artifacts(
            project_path,
            Some(requested_db.to_string_lossy().as_ref()),
        )
    }

    pub async fn new() -> Result<Self> {
        tracing::info!("Initializing CCM Core Engine for MCP...");

        // Use CCM_PROJECT_ROOT env var if available, otherwise default to CWD
        let project_root = std::env::var("CCM_PROJECT_ROOT").ok().or_else(|| {
            // If we are in / (root), don't default to it as it's often read-only
            if let Ok(cwd) = std::env::current_dir() {
                if cwd.to_string_lossy() == "/" {
                    return None;
                }
                return Some(cwd.to_string_lossy().to_string());
            }
            None
        });
        let default_project_root = project_root
            .as_deref()
            .map(Path::new)
            .map(canonicalize_project_path);

        // Prefer the selected project's shared index. A home-directory fallback is
        // only used when no project root is available.
        let db_path = if let Ok(path) = std::env::var("CCM_DB_PATH") {
            path
        } else if let Some(root) = &default_project_root {
            let candidate = root.join("data/ccm_db");
            // v0.3.9 kapsama güvencesi: `data` dış dizine symlink ise ya da
            // yol kök dışına çözülürse sessizce dış dizine bağlanma; server
            // başlatılamaz (tool çağrıları da aynı hatayı görür).
            ccm_core::resolve_artifact_path(root, &candidate)?
                .to_string_lossy()
                .to_string()
        } else {
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_else(|_| ".".to_string());
            let path = PathBuf::from(home)
                .join(".ccm")
                .join("mcp")
                .join("data")
                .join("ccm_mcp_db");
            let _ = std::fs::create_dir_all(&path);
            path.to_string_lossy().to_string()
        };

        tracing::info!(path = %db_path, "Using Vector DB path");

        // Auto-indexing removed to allow agentic control.
        if let Some(root) = &project_root {
            tracing::info!(
                root = %root,
                "Project root detected. Indexing is available on demand."
            );
        }

        // Initialize Graph (Load from disk if available)
        let default_db_path = PathBuf::from(&db_path);
        let default_artifact_parent = default_db_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let default_artifacts = match &default_project_root {
            Some(root) => match ccm_core::resolve_index_artifacts(
                root.to_string_lossy().as_ref(),
                Some(default_db_path.to_string_lossy().as_ref()),
            ) {
                Ok(artifacts) => artifacts,
                Err(error) => {
                    tracing::warn!(
                        error = %error,
                        "Active index pointer is invalid. Start index_project to repair it."
                    );
                    ccm_core::IndexArtifactPaths {
                        db_path: default_db_path.clone(),
                        graph_path: default_artifact_parent.join("ccm_graph.json"),
                        manifest_path: default_artifact_parent.join("ccm_manifest.json"),
                        generation_id: None,
                    }
                }
            },
            None => ccm_core::IndexArtifactPaths {
                db_path: default_db_path.clone(),
                graph_path: default_artifact_parent.join("ccm_graph.json"),
                manifest_path: default_artifact_parent.join("ccm_manifest.json"),
                generation_id: None,
            },
        };
        let graph_path = default_artifacts.graph_path.to_string_lossy().to_string();
        let mut graph = CodeGraph::new();

        if std::path::Path::new(&graph_path).exists() {
            tracing::info!(path = %graph_path, "Loading CodeGraph");
            match CodeGraph::load_from_file(&graph_path) {
                Ok(g) => {
                    graph = g;
                    tracing::info!(
                        nodes = graph.graph.node_count(),
                        "Graph loaded successfully"
                    );
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to load graph");
                }
            }
        } else {
            tracing::warn!(
                path = %graph_path,
                "No persisted graph found. Starting empty."
            );
        }

        let active_db_path = default_artifacts.db_path.to_string_lossy().to_string();
        let store = LanceDbStore::new(&active_db_path, "code_vectors").await?;
        let policy_path = std::path::Path::new(&db_path)
            .parent()
            .map(|parent| parent.join("ccm_learn/policies.json"));
        let default_engine = Arc::new(RetrievalEngine::new_with_active_policy(
            Arc::new(RwLock::new(graph)),
            store,
            policy_path.as_deref(),
        ));
        let cache_size = engine_cache_size();
        let require_allowed_roots = require_allowed_roots();
        let allowed_roots = load_allowed_roots();

        if require_allowed_roots && allowed_roots.is_empty() {
            tracing::warn!(
                "CCM_ALLOWED_ROOTS is required but empty. MCP will reject all project paths."
            );
        }

        Ok(Self {
            default_engine: RwLock::new(default_engine),
            engines: RwLock::new(EngineCache::new(cache_size)),
            index_locks: std::sync::Mutex::new(std::collections::HashMap::new()),
            index_jobs: std::sync::Mutex::new(std::collections::HashMap::new()),
            next_index_job_id: std::sync::atomic::AtomicU64::new(1),
            default_project_root,
            default_db_path,
            allowed_roots,
            require_allowed_roots,
        })
    }

    /// Retrieves the engine for a specific project path, or defaults to the startup engine.
    /// Loads the engine dynamically if it's not in the cache.
    pub async fn get_engine(&self, project_path: Option<&str>) -> Result<Arc<RetrievalEngine>> {
        let path = match project_path {
            Some(path) => path.to_string(),
            None => {
                let Some(root) = &self.default_project_root else {
                    if self.require_allowed_roots {
                        return Err(anyhow::anyhow!(
                            "No default project root is available and strict allowlist mode is enabled. Set CCM_PROJECT_ROOT and CCM_ALLOWED_ROOTS."
                        ));
                    }
                    return Ok(self.default_engine.read().await.clone());
                };
                root.to_string_lossy().to_string()
            }
        };

        if !self.is_path_allowed(&path) {
            return Err(anyhow::anyhow!(
                "Project path '{}' is not allowed. Set CCM_ALLOWED_ROOTS to permit access.",
                path
            ));
        }

        // Cache key normalize edilir; "/repo" ile "/repo/" ayrı entry oluşturmasın.
        let canonical_path = canonicalize_project_path(Path::new(&path));
        let cache_key = canonical_path.to_string_lossy().to_string();

        if self.index_job_in_progress(&cache_key) {
            return Err(anyhow::anyhow!(
                "Project indexing is in progress. Retry this tool after index_project reports completion."
            ));
        }

        let artifacts = self.project_artifacts(&cache_key)?;
        let engine_cache_key = format!(
            "{}#{}",
            cache_key,
            artifacts.generation_id.as_deref().unwrap_or("legacy")
        );

        // Check cache (write lock needed for LRU order update)
        {
            let mut engines = self.engines.write().await;
            if let Some(engine) = engines.get(&engine_cache_key) {
                return Ok(engine);
            }
        }

        tracing::info!(path = %cache_key, "Loading context for project");
        let db_path = artifacts.db_path.to_string_lossy().to_string();
        let graph_path = artifacts.graph_path.to_string_lossy().to_string();
        let manifest_path = artifacts.manifest_path.to_string_lossy().to_string();

        // Uzun full index retrieval çağrısının içinde çalıştırılmaz.
        if !Path::new(&db_path).exists()
            || !Path::new(&graph_path).is_file()
            || !Path::new(&manifest_path).is_file()
        {
            return Err(anyhow::anyhow!(
                "Project index is missing. Call index_project first; large indexes run in the background."
            ));
        }

        let graph = CodeGraph::load_from_file(&graph_path).map_err(|error| {
            anyhow::anyhow!(
                "Project graph '{}' could not be loaded: {}. Run index_project to rebuild it.",
                graph_path,
                error
            )
        })?;
        tracing::info!(
            path = %cache_key,
            nodes = graph.graph.node_count(),
            "Loaded graph for project"
        );

        let store = LanceDbStore::new(&db_path, "code_vectors").await?;
        let requested_db_path = self.project_db_path(&cache_key)?;
        let policy_path = requested_db_path
            .parent()
            .map(|parent| parent.join("ccm_learn/policies.json"));
        let engine = Arc::new(RetrievalEngine::new_with_active_policy(
            Arc::new(RwLock::new(graph)),
            store,
            policy_path.as_deref(),
        ));

        let mut engines = self.engines.write().await;
        if let Some(existing) = engines.get(&engine_cache_key) {
            return Ok(existing);
        }
        Ok(engines.insert(engine_cache_key, engine))
    }

    pub async fn refresh_project_engine(&self, project_path: &str) -> Result<()> {
        // get_engine ile aynı normalize key kullanılır ki cache tutarlı kalsın.
        let canonical_path = canonicalize_project_path(Path::new(project_path));
        let cache_key = canonical_path.to_string_lossy().to_string();
        let artifacts = self.project_artifacts(&cache_key)?;
        let db_path = artifacts.db_path.to_string_lossy().to_string();
        let graph = CodeGraph::load_from_file(&artifacts.graph_path.to_string_lossy())?;
        let store = LanceDbStore::new(&db_path, "code_vectors").await?;
        let requested_db_path = self.project_db_path(&cache_key)?;
        let policy_path = requested_db_path
            .parent()
            .map(|parent| parent.join("ccm_learn/policies.json"));
        let engine = Arc::new(RetrievalEngine::new_with_active_policy(
            Arc::new(RwLock::new(graph)),
            store,
            policy_path.as_deref(),
        ));

        let engine_cache_key = format!(
            "{}#{}",
            cache_key,
            artifacts.generation_id.as_deref().unwrap_or("legacy")
        );
        self.engines
            .write()
            .await
            .insert(engine_cache_key, engine.clone());
        if self
            .default_project_root
            .as_ref()
            .is_some_and(|root| canonical_path == *root)
        {
            *self.default_engine.write().await = engine;
        }
        Ok(())
    }

    fn is_path_allowed(&self, path: &str) -> bool {
        let candidate = canonicalize_project_path(Path::new(path));
        if self.allowed_roots.is_empty() {
            if self.require_allowed_roots {
                return false;
            }
            // Strict mod kapalıyken bile keyfi yollara izin verilmez:
            // yalnızca başlangıçta seçilen default proje kökü kabul edilir.
            return self
                .default_project_root
                .as_ref()
                .is_some_and(|root| candidate.starts_with(root));
        }
        self.allowed_roots
            .iter()
            .any(|root| candidate.starts_with(root))
    }
}

fn load_allowed_roots() -> Vec<PathBuf> {
    let raw = std::env::var("CCM_ALLOWED_ROOTS").unwrap_or_default();
    let mut roots: Vec<PathBuf> = if raw.trim().is_empty() {
        Vec::new()
    } else {
        let parts: Vec<&str> = if cfg!(windows) {
            raw.split([';', ',']).collect()
        } else {
            raw.split([':', ';', ',']).collect()
        };

        parts
            .into_iter()
            .map(|item| item.trim())
            .filter(|item| !item.is_empty())
            .map(|item| canonicalize_project_path(Path::new(item)))
            .collect()
    };

    if roots.is_empty() {
        if let Ok(root) = std::env::var("CCM_PROJECT_ROOT") {
            roots.push(canonicalize_project_path(Path::new(&root)));
        }
    }

    roots
}

fn require_allowed_roots() -> bool {
    // Varsayılan strict: allowlist zorunlu. Geniş erişim için açıkça
    // CCM_REQUIRE_ALLOWED_ROOTS=0 verilmesi gerekir.
    std::env::var("CCM_REQUIRE_ALLOWED_ROOTS")
        .or_else(|_| std::env::var("CCM_MCP_REQUIRE_ALLOWED_ROOTS"))
        .map(|val| matches!(val.to_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(true)
}

fn canonicalize_project_path(path: &Path) -> PathBuf {
    let abs = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    std::fs::canonicalize(&abs).unwrap_or_else(|_| normalize_path(&abs))
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => result.push(prefix.as_os_str()),
            Component::RootDir => result.push(std::path::MAIN_SEPARATOR_STR),
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            Component::Normal(part) => result.push(part),
        }
    }
    result
}

/// Main request dispatcher.
/// Returns Ok(Some(response)) for requests, Ok(None) for notifications.
pub async fn handle_request(
    state: &Arc<ServerState>,
    raw_request: &str,
) -> Result<Option<JsonRpcResponse>> {
    let raw_value: Value = serde_json::from_str(raw_request)?;
    let Some(object) = raw_value.as_object() else {
        return Ok(Some(create_error_response(
            None,
            -32600,
            "Invalid Request: JSON-RPC payload must be an object",
        )));
    };
    let request_id = object.get("id").cloned();
    let is_notification = !object.contains_key("id");
    if request_id
        .as_ref()
        .is_some_and(|id| !(id.is_null() || id.is_string() || id.is_number()))
    {
        return Ok(Some(create_error_response(
            None,
            -32600,
            "Invalid Request: id must be a string, number, or null",
        )));
    }
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0")
        || object.get("method").and_then(Value::as_str).is_none()
    {
        return Ok(Some(create_error_response(
            request_id,
            -32600,
            "Invalid Request: jsonrpc must be '2.0' and method must be a string",
        )));
    }
    if object
        .get("params")
        .is_some_and(|params| !(params.is_object() || params.is_array()))
    {
        if is_notification {
            return Ok(None);
        }
        return Ok(Some(create_error_response(
            request_id,
            -32602,
            "Invalid params: params must be an object or array",
        )));
    }
    let request: JsonRpcRequest = serde_json::from_value(raw_value)?;

    if request.jsonrpc != "2.0" {
        return Ok(Some(create_error_response(
            request.id,
            -32600,
            "Invalid Request: jsonrpc must be '2.0'",
        )));
    }

    let response = match request.method.as_str() {
        "initialize" => handle_initialize(request.id, request.params.as_ref()).map(Some),
        "initialized" | "notifications/initialized" => {
            Ok(Some(create_success_response(request.id, json!({}))))
        }
        "tools/list" => handle_list_tools(request.id).map(Some),
        "resources/list" => Ok(Some(create_success_response(
            request.id,
            json!({ "resources": [] }),
        ))),
        "resources/templates/list" => Ok(Some(create_success_response(
            request.id,
            json!({ "resourceTemplates": [] }),
        ))),
        "tools/call" => {
            // `tools/call` notification'ı MCP sözleşmesinin parçası değildir.
            // Yanıt üretmediği için ağır bir tool'u (örn. index_project) bu
            // yoldan çalıştırmak ana JSON-RPC loop'unu bloklar ve sonraki
            // gerçek istekleri geciktirir. Notification olarak gelen
            // tools/call güvenle yok sayılır.
            if is_notification {
                tracing::warn!("tools/call notification ignored; use a request with an id instead");
                Ok(None)
            } else {
                handle_call_tool(state, request.id, request.params)
                    .await
                    .map(Some)
            }
        }
        _ => Ok(Some(create_error_response(
            request.id,
            -32601,
            &format!("Method not found: {}", request.method),
        ))),
    };

    if is_notification {
        if let Err(error) = response {
            tracing::warn!(method = %request.method, error = %error, "Notification failed");
        }
        Ok(None)
    } else {
        response
    }
}

fn handle_initialize(id: Option<Value>, params: Option<&Value>) -> Result<JsonRpcResponse> {
    let result = json!({
        "protocolVersion": negotiate_protocol_version(params),
        "capabilities": ServerCapabilities {
            tools: ToolsCapability { list_changed: false },
            resources: ResourcesCapability {
                subscribe: false,
                list_changed: false,
            },
        },
        "serverInfo": ServerInfo {
            name: "ccm-mcp".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
    });
    Ok(create_success_response(id, result))
}

fn negotiate_protocol_version(params: Option<&Value>) -> &'static str {
    let requested = params
        .and_then(|value| value.get("protocolVersion"))
        .and_then(Value::as_str);

    match requested {
        Some(version) => SUPPORTED_PROTOCOL_VERSIONS
            .iter()
            .copied()
            .find(|supported| *supported == version)
            .unwrap_or(LATEST_PROTOCOL_VERSION),
        None => LATEST_PROTOCOL_VERSION,
    }
}

fn handle_list_tools(id: Option<Value>) -> Result<JsonRpcResponse> {
    let tools_list = vec![
        ToolDefinition {
            name: "get_context".to_string(),
            description: Some("Get code context for a given file and line.".to_string()),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "The file path" },
                    "line": { "type": "integer", "minimum": 1, "description": "The line number" },
                    "project_path": { "type": "string", "description": "Optional absolute path to the project root. If provided, uses the index in that project." },
                    "include_body": { "type": "boolean", "description": "Include node body snippets. Defaults to false (metadata only)." },
                    "max_chars": { "type": "integer", "minimum": 1, "maximum": 100000, "description": "Maximum total body characters to include. Defaults to 4000." }
                },
                "required": ["file", "line"]
            }),
        },
        ToolDefinition {
            name: "search_code".to_string(),
            description: Some("Search the codebase using hybrid semantic and graph-aware ranking. Returns node IDs and location metadata so results can be chained into read_graph.".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "The search query (e.g. 'how does authentication work?')" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "description": "Optional maximum number of results to return. Defaults to 5." },
                    "project_path": { "type": "string", "description": "Optional absolute path to the project root. If provided, uses the index in that project." },
                    "include_body": { "type": "boolean", "description": "Include node body snippets. Defaults to false (metadata only)." },
                    "max_chars": { "type": "integer", "minimum": 1, "maximum": 100000, "description": "Maximum total body characters to include. Defaults to 4000." }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "find_nodes".to_string(),
            description: Some("Find graph nodes by name, file path, or node ID fragment. Use this before read_graph when you do not already know the node ID.".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "A node name, file path fragment, or node ID fragment to search for." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "description": "Optional maximum number of matches to return. Defaults to 10." },
                    "project_path": { "type": "string", "description": "Optional absolute path to the project root. If provided, uses the index in that project." },
                    "include_body": { "type": "boolean", "description": "Include node body snippets. Defaults to false (metadata only)." },
                    "max_chars": { "type": "integer", "minimum": 1, "maximum": 100000, "description": "Maximum total body characters to include. Defaults to 4000." }
                },
                "required": ["query"]
            }),
        },
        ToolDefinition {
            name: "read_graph".to_string(),
            description: Some("Get details of a specific code node by ID.".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "node_id": { "type": "string", "description": "The ID of the node to retrieve." },
                    "project_path": { "type": "string", "description": "Optional absolute path to the project root. If provided, uses the index in that project." },
                    "include_body": { "type": "boolean", "description": "Include node body snippets. Defaults to false (metadata only)." },
                    "max_chars": { "type": "integer", "minimum": 1, "maximum": 100000, "description": "Maximum total body characters to include. Defaults to 4000." }
                },
                "required": ["node_id"]
            }),
        },
        ToolDefinition {
            name: "index_project".to_string(),
            description: Some("Refresh the project index. Usually performs an incremental update and reports when the existing index is already up to date.".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "project_path": { "type": "string", "description": "Absolute path to the project root to index." }
                },
                "required": ["project_path"]
            }),
        },
        ToolDefinition {
            name: "find_usages".to_string(),
            description: Some("Find all nodes that call or reference a given node. Answers 'who calls this function?'.".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "node_id": { "type": "string", "description": "Node ID to find usages for (from read_graph or search_code results)." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "description": "Max usages to return. Defaults to 20." },
                    "project_path": { "type": "string", "description": "Optional absolute path to the project root." },
                    "include_body": { "type": "boolean", "description": "Include node body snippets. Defaults to false (metadata only)." },
                    "max_chars": { "type": "integer", "minimum": 1, "maximum": 100000, "description": "Maximum total body characters to include. Defaults to 4000." }
                },
                "required": ["node_id"]
            }),
        },
        ToolDefinition {
            name: "trace_call_chain".to_string(),
            description: Some("Find the BFS call chain between two nodes. Shows how execution flows from one function to another.".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "from_id": { "type": "string", "description": "Starting node ID." },
                    "to_id": { "type": "string", "description": "Target node ID." },
                    "max_depth": { "type": "integer", "minimum": 1, "maximum": 32, "description": "Max hops to search. Defaults to 8." },
                    "project_path": { "type": "string", "description": "Optional absolute path to the project root." },
                    "include_body": { "type": "boolean", "description": "Include node body snippets. Defaults to false (metadata only)." },
                    "max_chars": { "type": "integer", "minimum": 1, "maximum": 100000, "description": "Maximum total body characters to include. Defaults to 4000." }
                },
                "required": ["from_id", "to_id"]
            }),
        },
        ToolDefinition {
            name: "impact_of_change".to_string(),
            description: Some("Analyze the blast radius of changing a file. Returns all dependents across the codebase. Essential for safe refactoring.".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "file": { "type": "string", "description": "Relative path of the file to analyze (e.g. 'src/engine.rs')." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "description": "Max dependents to return. Defaults to 30." },
                    "project_path": { "type": "string", "description": "Optional absolute path to the project root." },
                    "include_body": { "type": "boolean", "description": "Include node body snippets. Defaults to false (metadata only)." },
                    "max_chars": { "type": "integer", "minimum": 1, "maximum": 100000, "description": "Maximum total body characters to include. Defaults to 4000." }
                },
                "required": ["file"]
            }),
        },
        ToolDefinition {
            name: "diff_context".to_string(),
            description: Some("Get graph nodes for recently changed files based on git history. Shows what code has changed in the last N days.".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "project_path": { "type": "string", "description": "Absolute path to the project root (must be a git repo)." },
                    "days": { "type": "integer", "minimum": 1, "maximum": 3650, "description": "Days to look back in git history. Defaults to 7." },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "description": "Max nodes to return. Defaults to 30." },
                    "include_body": { "type": "boolean", "description": "Include node body snippets. Defaults to false (metadata only)." },
                    "max_chars": { "type": "integer", "minimum": 1, "maximum": 100000, "description": "Maximum total body characters to include. Defaults to 4000." }
                },
                "required": ["project_path"]
            }),
        },
    ];

    Ok(create_success_response(id, json!({ "tools": tools_list })))
}

async fn handle_call_tool(
    state: &Arc<ServerState>,
    id: Option<Value>,
    params: Option<Value>,
) -> Result<JsonRpcResponse> {
    let request_id = id.as_ref().map(|value| match value {
        Value::String(text) => text.clone(),
        _ => value.to_string(),
    });
    let tool_name = params
        .as_ref()
        .and_then(|value| value.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();
    let context = ccm_core::trajectory::TrajectoryContext {
        tool_name,
        request_id,
    };
    ccm_core::trajectory::with_context(context, handle_call_tool_inner(state, id, params)).await
}

async fn handle_call_tool_inner(
    state: &Arc<ServerState>,
    id: Option<Value>,
    params: Option<Value>,
) -> Result<JsonRpcResponse> {
    let Some(params) = params else {
        return Ok(create_error_response(
            id,
            -32602,
            "Missing tools/call params",
        ));
    };
    let Some(tool_name) = params.get("name").and_then(|v| v.as_str()) else {
        return Ok(create_error_response(id, -32602, "Missing tool name"));
    };
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

    if let Err(message) = validate_tool_arguments(tool_name, &arguments) {
        return Ok(create_error_response(id, -32602, &message));
    }

    // Extract project_path if present
    let project_path = arguments.get("project_path").and_then(|v| v.as_str());

    if tool_name == "index_project" {
        let path = project_path.ok_or_else(|| anyhow::anyhow!("Missing project_path"))?;
        if !state.is_path_allowed(path) {
            return Ok(create_error_response(
                id,
                -32602,
                "Project path is not allowed. Set CCM_ALLOWED_ROOTS (or disable CCM_REQUIRE_ALLOWED_ROOTS).",
            ));
        }

        let result = tools::index_project(state.clone(), &arguments).await?;
        return Ok(create_success_response(id, json!(result)));
    }

    // Resolve Engine
    let engine = match state.get_engine(project_path).await {
        Ok(e) => e,
        Err(e) => {
            tracing::warn!(error = %e, tool = %tool_name, "Failed to load project context");
            let message = if e.to_string().contains("Project index is missing") {
                "Project index is missing. Call index_project first.".to_string()
            } else if e.to_string().contains("Project indexing is in progress") {
                "Project indexing is in progress. Poll index_project before retrying this tool."
                    .to_string()
            } else if e.to_string().contains("not allowed")
                || e.to_string().contains("No default project root")
            {
                e.to_string()
            } else {
                "Failed to load project context. Check project_path, allowlist, and index state."
                    .to_string()
            };
            return Ok(create_error_response(
                id, -32603, // Internal error / Invalid params
                &message,
            ));
        }
    };

    let result = match tool_name {
        "get_context" => tools::get_context(&engine, &arguments).await?,
        "search_code" => tools::search_code(&engine, &arguments).await?,
        "find_nodes" => tools::find_nodes(&engine, &arguments).await?,
        "read_graph" => tools::read_graph(&engine, &arguments).await?,
        "find_usages" => tools::find_usages(&engine, &arguments).await?,
        "trace_call_chain" => tools::trace_call_chain(&engine, &arguments).await?,
        "impact_of_change" => tools::impact_of_change(&engine, &arguments).await?,
        "diff_context" => tools::diff_context(&engine, &arguments).await?,
        _ => {
            return Ok(create_error_response(
                id,
                -32602,
                &format!("Unknown tool: {}", tool_name),
            ))
        }
    };

    Ok(create_success_response(id, serde_json::to_value(result)?))
}

fn validate_tool_arguments(tool_name: &str, arguments: &Value) -> std::result::Result<(), String> {
    let required_strings: &[&str] = match tool_name {
        "get_context" => &["file"],
        "search_code" | "find_nodes" => &["query"],
        "read_graph" | "find_usages" => &["node_id"],
        "trace_call_chain" => &["from_id", "to_id"],
        "impact_of_change" => &["file"],
        "diff_context" | "index_project" => &["project_path"],
        _ => return Err(format!("Unknown tool: {}", tool_name)),
    };

    for name in required_strings {
        let valid = arguments
            .get(*name)
            .and_then(Value::as_str)
            .is_some_and(|value| !value.trim().is_empty());
        if !valid {
            return Err(format!(
                "Missing or invalid '{}' argument for {}",
                name, tool_name
            ));
        }
    }

    if tool_name == "get_context"
        && arguments
            .get("line")
            .and_then(Value::as_u64)
            .is_none_or(|line| line == 0)
    {
        return Err("Missing or invalid 'line' argument for get_context".to_string());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::negotiate_protocol_version;
    use serde_json::json;

    #[test]
    fn initialize_prefers_latest_when_client_omits_version() {
        assert_eq!(negotiate_protocol_version(None), "2025-11-25");
    }

    #[test]
    fn initialize_honors_supported_client_version() {
        let params = json!({
            "protocolVersion": "2025-06-18"
        });

        assert_eq!(negotiate_protocol_version(Some(&params)), "2025-06-18");
    }

    #[test]
    fn initialize_falls_back_to_latest_for_unknown_versions() {
        let params = json!({
            "protocolVersion": "2024-11-05"
        });

        assert_eq!(negotiate_protocol_version(Some(&params)), "2025-11-25");
    }
}
