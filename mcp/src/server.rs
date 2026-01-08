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
        let graph = CodeGraph::new();
        let store = LanceDbStore::new("data/ccm_mcp_db", "mcp_vectors").await?;
        let engine = Arc::new(RetrievalEngine::new(graph, store));

        // In a real scenario, we should load an existing index or index in background.
        // For this demo/mcp instance, we can't easily index everything on startup without scanning files.
        // So we leave the engine initialized but empty until we add a "scan_workspace" tool.
        // Or we could trigger a scan if we had the path.

        Ok(Self { engine })
    }
}

/// Main request dispatcher.
pub async fn handle_request(state: &ServerState, raw_request: &str) -> Result<JsonRpcResponse> {
    let request: JsonRpcRequest = serde_json::from_str(raw_request)?;

    match request.method.as_str() {
        "initialize" => handle_initialize(request.id),
        "initialized" => Ok(create_success_response(request.id, json!({}))),
        "tools/list" => handle_list_tools(request.id),
        "tools/call" => handle_call_tool(state, request.id, request.params).await,
        _ => Ok(create_error_response(
            request.id,
            -32601,
            &format!("Method not found: {}", request.method),
        )),
    }
}

fn handle_initialize(id: Option<Value>) -> Result<JsonRpcResponse> {
    let result = json!({
        "protocolVersion": "2024-11-05",
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
