//! MCP Server request handling logic.

use anyhow::Result;
use serde_json::{json, Value};
use std::collections::HashMap;
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

/// Holds the server's shared state.
pub struct ServerState {
    pub default_engine: Arc<RetrievalEngine>,
    pub engines: RwLock<HashMap<String, Arc<RetrievalEngine>>>,
}

impl ServerState {
    pub async fn new() -> Result<Self> {
        eprintln!("Initializing CCM Core Engine for MCP...");

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

        eprintln!("Using Vector DB Path: {}", db_path);

        // Auto-indexing removed to allow agentic control.
        if let Some(root) = &project_root {
            eprintln!(
                "Project root detected: {}. Indexing is available on demand.",
                root
            );
        }

        // Initialize Graph (Load from disk if available)
        let graph_path = format!("{}/../ccm_graph.json", db_path); // db_path is data/ccm_db, so json is data/ccm_graph.json
        let mut graph = CodeGraph::new();

        if std::path::Path::new(&graph_path).exists() {
            eprintln!("Loading CodeGraph from: {}", graph_path);
            match CodeGraph::load_from_file(&graph_path) {
                Ok(g) => {
                    graph = g;
                    eprintln!(
                        "✓ Graph loaded successfully. Nodes: {}",
                        graph.graph.node_count()
                    );
                }
                Err(e) => eprintln!("⚠ Failed to load graph: {}", e),
            }
        } else {
            eprintln!(
                "⚠ No persisted graph found at {}. Starting empty.",
                graph_path
            );
        }

        let store = LanceDbStore::new(&db_path, "code_vectors").await?;
        let default_engine = Arc::new(RetrievalEngine::new(Arc::new(RwLock::new(graph)), store));

        Ok(Self {
            default_engine,
            engines: RwLock::new(HashMap::new()),
        })
    }

    /// Retrieves the engine for a specific project path, or defaults to the startup engine.
    /// Loads the engine dynamically if it's not in the cache.
    pub async fn get_engine(&self, project_path: Option<&str>) -> Result<Arc<RetrievalEngine>> {
        let path = match project_path {
            Some(p) => p,
            None => return Ok(self.default_engine.clone()),
        };

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

        eprintln!("Loading context for project: {}", path);
        // Assume db at path/data/ccm_db
        // Use the MCP specific DB path
        let db_path = format!("{}/data/ccm_mcp_db", path);

        // Sanity check & Lazy Indexing
        if !std::path::Path::new(&db_path).exists() {
            eprintln!(
                "⚠ Index not found at {}. Triggering Lazy Indexing...",
                db_path
            );

            // LAZY INDEXING: Index specifically for this request
            match ccm_core::index_directory(path, Some(&db_path)).await {
                Ok(stats) => {
                    eprintln!(
                        "✓ Lazy Indexing Complete. Indexed {} nodes.",
                        stats.nodes_created
                    );
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
                eprintln!(
                    "Loaded graph for {}: {} nodes",
                    path,
                    graph.graph.node_count()
                );
            }
        }

        let store = LanceDbStore::new(&db_path, "code_vectors").await?;
        let engine = Arc::new(RetrievalEngine::new(Arc::new(RwLock::new(graph)), store));

        write.insert(path.to_string(), engine.clone());
        Ok(engine)
    }
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
        "initialize" => handle_initialize(request.id).map(Some),
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

fn handle_initialize(id: Option<Value>) -> Result<JsonRpcResponse> {
    let result = json!({
        "protocolVersion": "2025-06-18",
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
        "index_project" => tools::index_project(&arguments).await?,
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
