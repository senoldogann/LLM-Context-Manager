//! MCP Tool implementations that bridge to ccm-core.

use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;

use crate::protocol::{ToolResult, ToolResultContent};
use ccm_core::engine::{CursorPosition, RetrievalEngine};

/// Tool: get_context
/// Returns context for a given file path and line number.
pub async fn get_context(engine: &Arc<RetrievalEngine>, args: &Value) -> Result<ToolResult> {
    // Input Validation
    let file = args
        .get("file")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'file' argument"))?;

    if file.is_empty() {
        return Ok(ToolResult {
            content: vec![ToolResultContent {
                content_type: "text".to_string(),
                text: "Error: 'file' argument cannot be empty.".to_string(),
            }],
            is_error: Some(true),
        });
    }

    let line = args.get("line").and_then(|v| v.as_u64()).ok_or_else(|| {
        anyhow::anyhow!("Missing or invalid 'line' argument (must be a positive integer)")
    })? as usize;

    let cursor = CursorPosition {
        file_path: file.to_string(),
        line,
        column: 0,
    };

    let suggestions = engine.predict_context(&cursor).await?;

    if suggestions.is_empty() {
        return Ok(ToolResult {
            content: vec![ToolResultContent {
                content_type: "text".to_string(),
                text: format!(
                    "No context found for {}:{}\n\nThe code graph may not be indexed yet. Try indexing the project first.",
                    file, line
                ),
            }],
            is_error: None,
        });
    }

    let mut output = String::new();
    for suggestion in suggestions {
        output.push_str(&format!(
            "## {} (Score: {:.2})\n**Reason:** {}\n\n```\n{}\n```\n\n---\n",
            suggestion.title, suggestion.relevance_score, suggestion.reason, suggestion.content
        ));
    }

    Ok(ToolResult {
        content: vec![ToolResultContent {
            content_type: "text".to_string(),
            text: output,
        }],
        is_error: None,
    })
}

/// Tool: search_code
/// Performs semantic search in the codebase.
pub async fn search_code(engine: &Arc<RetrievalEngine>, args: &Value) -> Result<ToolResult> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing query argument"))?;

    // Input Validation: Empty Query
    if query.trim().is_empty() {
        return Ok(ToolResult {
            content: vec![ToolResultContent {
                content_type: "text".to_string(),
                text: "Error: 'query' cannot be empty. Please provide a search term.".to_string(),
            }],
            is_error: Some(true),
        });
    }

    let hits = engine.search_code(query, 5).await?;
    eprintln!(
        "[DEBUG] search_code found {} hits for query: '{}'",
        hits.len(),
        query
    );

    if hits.is_empty() {
        return Ok(ToolResult {
            content: vec![ToolResultContent {
                content_type: "text".to_string(),
                text: format!("No results found for query: '{}'\n\nTip: Make sure the project has been indexed.", query),
            }],
            is_error: None,
        });
    }

    let mut output = String::new();
    for hit in hits {
        output.push_str(&format!(
            "## {} (Score: {:.2})\n**Reason:** {}\n\n```\n{}\n```\n\n---\n",
            hit.title, hit.relevance_score, hit.reason, hit.content
        ));
    }

    Ok(ToolResult {
        content: vec![ToolResultContent {
            content_type: "text".to_string(),
            text: output,
        }],
        is_error: None,
    })
}

/// Tool: read_graph
/// Retrieves details of a specific node in the code graph.
pub async fn read_graph(engine: &Arc<RetrievalEngine>, args: &Value) -> Result<ToolResult> {
    let node_id = args
        .get("node_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing node_id argument"))?;

    if let Some(node) = engine.get_node_by_id(node_id) {
        let output = format!(
            "## Node Details: {}\n\n**Type:** {:?}\n**ID:** {}\n**Range:** Lines {}-{}\n\n```\n{}\n```",
            node.name, node.node_type, node.id, node.start_line, node.end_line, node.content
        );

        Ok(ToolResult {
            content: vec![ToolResultContent {
                content_type: "text".to_string(),
                text: output,
            }],
            is_error: None,
        })
    } else {
        Ok(ToolResult {
            content: vec![ToolResultContent {
                content_type: "text".to_string(),
                text: format!("Node not found with ID: {}", node_id),
            }],
            is_error: Some(true),
        })
    }
}
