//! MCP Server request handling logic.

use anyhow::Result;
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::protocol::{
    create_error_response, create_success_response, JsonRpcRequest, JsonRpcResponse,
    ServerCapabilities, ServerInfo, ToolDefinition, ToolsCapability,
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
    pub default_engine: Arc<RetrievalEngine>,
    pub engines: RwLock<EngineCache>,
    allowed_roots: Vec<PathBuf>,
    require_allowed_roots: bool,
}

const DEFAULT_ENGINE_CACHE_SIZE: usize = 8;

fn engine_cache_size() -> usize {
    std::env::var("CCM_MCP_ENGINE_CACHE_SIZE")
        .ok()
        .and_then(|val| val.parse::<usize>().ok())
        .map(|val| val.max(1))
        .unwrap_or(DEFAULT_ENGINE_CACHE_SIZE)
}

pub struct EngineCache {
    map: HashMap<String, Arc<RetrievalEngine>>,
    order: VecDeque<String>,
    max: usize,
}

impl EngineCache {
    fn new(max: usize) -> Self {
        Self {
            map: HashMap::new(),
            order: VecDeque::new(),
            max,
        }
    }

    fn get(&self, key: &str) -> Option<Arc<RetrievalEngine>> {
        self.map.get(key).cloned()
    }

    fn insert(&mut self, key: String, engine: Arc<RetrievalEngine>) -> Arc<RetrievalEngine> {
        if !self.map.contains_key(&key) {
            if self.map.len() >= self.max {
                if let Some(evict_key) = self.order.pop_front() {
                    self.map.remove(&evict_key);
                }
            }
            self.order.push_back(key.clone());
        }

        self.map.insert(key, engine.clone());
        engine
    }

    pub fn evict(&mut self, key: &str) {
        self.map.remove(key);
        self.order.retain(|k| k != key);
    }
}

impl ServerState {
    pub async fn new() -> Result<Self> {
        tracing::info!("Initializing CCM Core Engine for MCP...");

        // Use CCM_DB_PATH env var if available
        let db_path = if let Ok(path) = std::env::var("CCM_DB_PATH") {
            path
        } else {
            // Default to absolute path in home dir to avoid read-only CWD issues
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .unwrap_or_else(|_| ".".to_string());
            let path = std::path::PathBuf::from(home)
                .join(".ccm")
                .join("mcp")
                .join("data")
                .join("ccm_mcp_db");
            // Ensure directory exists
            let _ = std::fs::create_dir_all(&path);
            path.to_string_lossy().to_string()
        };

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

        tracing::info!(path = %db_path, "Using Vector DB path");

        // Auto-indexing removed to allow agentic control.
        if let Some(root) = &project_root {
            tracing::info!(
                root = %root,
                "Project root detected. Indexing is available on demand."
            );
        }

        // Initialize Graph (Load from disk if available)
        let graph_path = format!("{}/../ccm_graph.json", db_path); // db_path is data/ccm_db, so json is data/ccm_graph.json
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
                Err(e) => tracing::warn!(error = %e, "Failed to load graph"),
            }
        } else {
            tracing::warn!(
                path = %graph_path,
                "No persisted graph found. Starting empty."
            );
        }

        let store = LanceDbStore::new(&db_path, "code_vectors").await?;
        let default_engine = Arc::new(RetrievalEngine::new(Arc::new(RwLock::new(graph)), store));
        let cache_size = engine_cache_size();
        let require_allowed_roots = require_allowed_roots();
        let allowed_roots = load_allowed_roots();

        if require_allowed_roots && allowed_roots.is_empty() {
            tracing::warn!(
                "CCM_ALLOWED_ROOTS is required but empty. MCP will reject all project paths."
            );
        }

        Ok(Self {
            default_engine,
            engines: RwLock::new(EngineCache::new(cache_size)),
            allowed_roots,
            require_allowed_roots,
        })
    }

    /// Retrieves the engine for a specific project path, or defaults to the startup engine.
    /// Loads the engine dynamically if it's not in the cache.
    pub async fn get_engine(&self, project_path: Option<&str>) -> Result<Arc<RetrievalEngine>> {
        let path = match project_path {
            Some(p) => p,
            None => return Ok(self.default_engine.clone()),
        };

        if !self.is_path_allowed(path) {
            return Err(anyhow::anyhow!(
                "Project path '{}' is not allowed. Set CCM_ALLOWED_ROOTS to permit access.",
                path
            ));
        }

        // Check cache (read)
        {
            let read = self.engines.read().await;
            if let Some(engine) = read.get(path) {
                return Ok(engine.clone());
            }
        }

        // Load (write)
        let mut write = self.engines.write().await;
        // Double check
        if let Some(engine) = write.get(path) {
            return Ok(engine.clone());
        }

        tracing::info!(path = %path, "Loading context for project");
        // Assume db at path/data/ccm_db
        // Use the MCP specific DB path
        let db_path = format!("{}/data/ccm_db", path);

        // Sanity check & Lazy Indexing
        if !std::path::Path::new(&db_path).exists() {
            tracing::warn!(
                path = %db_path,
                "Index not found. Triggering lazy indexing."
            );

            // LAZY INDEXING: Incremental index for this request
            match ccm_core::update_index(path, Some(&db_path)).await {
                Ok(stats) => {
                    tracing::info!(nodes = stats.nodes_created, "Lazy indexing complete");
                }
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "Failed to auto-index project: {}. Please fix the project path or permissions.",
                        e
                    ));
                }
            }
        }

        let graph_path = format!("{}/data/ccm_graph.json", path);
        let mut graph = CodeGraph::new();
        if std::path::Path::new(&graph_path).exists() {
            if let Ok(g) = CodeGraph::load_from_file(&graph_path) {
                graph = g;
                tracing::info!(
                    path = %path,
                    nodes = graph.graph.node_count(),
                    "Loaded graph for project"
                );
            }
        }

        let store = LanceDbStore::new(&db_path, "code_vectors").await?;
        let engine = Arc::new(RetrievalEngine::new(Arc::new(RwLock::new(graph)), store));

        Ok(write.insert(path.to_string(), engine))
    }

    fn is_path_allowed(&self, path: &str) -> bool {
        if self.allowed_roots.is_empty() {
            return !self.require_allowed_roots;
        }
        let candidate = canonicalize_project_path(Path::new(path));
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
    std::env::var("CCM_REQUIRE_ALLOWED_ROOTS")
        .or_else(|_| std::env::var("CCM_MCP_REQUIRE_ALLOWED_ROOTS"))
        .map(|val| matches!(val.to_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
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
    state: &ServerState,
    raw_request: &str,
) -> Result<Option<JsonRpcResponse>> {
    let request: JsonRpcRequest = serde_json::from_str(raw_request)?;

    // Check if this is a notification (no id field means notification)
    let is_notification = request.id.is_none();

    match request.method.as_str() {
        "initialize" => handle_initialize(request.id, request.params.as_ref()).map(Some),
        "initialized" | "notifications/initialized" => {
            // Notifications don't get responses per JSON-RPC 2.0 spec
            if is_notification {
                Ok(None)
            } else {
                // If it has an id (unusual), respond with empty object
                Ok(Some(create_success_response(request.id, json!({}))))
            }
        }
        "tools/list" => handle_list_tools(request.id).map(Some),
        "tools/call" => handle_call_tool(state, request.id, request.params)
            .await
            .map(Some),
        _ => {
            // For unknown methods, only respond if it's a request (has id)
            if is_notification {
                Ok(None)
            } else {
                Ok(Some(create_error_response(
                    request.id,
                    -32601,
                    &format!("Method not found: {}", request.method),
                )))
            }
        }
    }
}

fn handle_initialize(id: Option<Value>, params: Option<&Value>) -> Result<JsonRpcResponse> {
    let result = json!({
        "protocolVersion": negotiate_protocol_version(params),
        "capabilities": ServerCapabilities {
            tools: ToolsCapability { list_changed: false },
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
                    "line": { "type": "integer", "description": "The line number" },
                    "project_path": { "type": "string", "description": "Optional absolute path to the project root. If provided, uses the index in that project." }
                },
                "required": ["file", "line"]
            }),
        },
        ToolDefinition {
            name: "search_code".to_string(),
            description: Some("Search the codebase using vector similarity.".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string", "description": "The search query (e.g. 'how does authentication work?')" },
                    "project_path": { "type": "string", "description": "Optional absolute path to the project root. If provided, uses the index in that project." }
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
                    "project_path": { "type": "string", "description": "Optional absolute path to the project root. If provided, uses the index in that project." }
                },
                "required": ["node_id"]
            }),
        },
        ToolDefinition {
            name: "index_project".to_string(),
            description: Some("Trigger a full re-index of the project. Use this when you start working on a project or when massive changes happen.".to_string()),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "project_path": { "type": "string", "description": "Absolute path to the project root to index." }
                },
                "required": ["project_path"]
            }),
        },
    ];

    Ok(create_success_response(id, json!({ "tools": tools_list })))
}

async fn handle_call_tool(
    state: &ServerState,
    id: Option<Value>,
    params: Option<Value>,
) -> Result<JsonRpcResponse> {
    let params = params.ok_or_else(|| anyhow::anyhow!("Missing params"))?;
    let tool_name = params
        .get("name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing tool name"))?;
    let arguments = params.get("arguments").cloned().unwrap_or(json!({}));

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
    }

    // Resolve Engine
    let engine = match state.get_engine(project_path).await {
        Ok(e) => e,
        Err(e) => {
            return Ok(create_error_response(
                id,
                -32603, // Internal error / Invalid params
                &format!("Failed to load project context: {}", e),
            ));
        }
    };

    let result = match tool_name {
        "get_context" => tools::get_context(&engine, &arguments).await?,
        "search_code" => tools::search_code(&engine, &arguments).await?,
        "read_graph" => tools::read_graph(&engine, &arguments).await?,
        "index_project" => tools::index_project(state, &arguments).await?,
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
