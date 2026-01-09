//! MCP Server request handling logic.

use anyhow::Result;
use serde_json::{json, Value};
use std::sync::Arc;

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
    pub engine: Arc<RetrievalEngine>,
}

impl ServerState {
    pub async fn new() -> Result<Self> {
        eprintln!("Initializing CCM Core Engine for MCP...");

        // Use CCM_DB_PATH env var if available
        let db_path =
            std::env::var("CCM_DB_PATH").unwrap_or_else(|_| "data/ccm_mcp_db".to_string());
        let project_root = std::env::var("CCM_PROJECT_ROOT").ok();

        eprintln!("Using Vector DB Path: {}", db_path);

        // Auto-Index if project root is provided
        if let Some(root) = &project_root {
            eprintln!("Auto-indexing enabled for: {}", root);
            let root_clone = root.clone();
            let db_path_clone = db_path.clone();

            // Spawn background indexing task
            tokio::spawn(async move {
                eprintln!("[Auto-Index] Starting background indexing...");
                match ccm_core::index_directory(&root_clone, Some(&db_path_clone)).await {
                    Ok(stats) => {
                        eprintln!(
                            "[Auto-Index] Complete! Indexed {} nodes.",
                            stats.nodes_created
                        );
                    }
                    Err(e) => {
                        eprintln!("[Auto-Index] Failed: {}", e);
                    }
                }
            });
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
        let engine = Arc::new(RetrievalEngine::new(graph, store));

        Ok(Self { engine })
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
                    "line": { "type": "integer", "description": "The line number" }
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
                    "query": { "type": "string", "description": "The search query (e.g. 'how does authentication work?')" }
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
                    "node_id": { "type": "string", "description": "The ID of the node to retrieve." }
                },
                "required": ["node_id"]
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

    let result = match tool_name {
        "get_context" => tools::get_context(&state.engine, &arguments).await?,
        "search_code" => tools::search_code(&state.engine, &arguments).await?,
        "read_graph" => tools::read_graph(&state.engine, &arguments).await?,
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
