//! MCP Tool implementations that bridge to ccm-core.

use anyhow::Result;
use serde_json::Value;
use std::sync::Arc;

use crate::protocol::{ToolResult, ToolResultContent};
use ccm_core::engine::{CursorPosition, RetrievalEngine};

/// Tool: get_context
/// Returns context for a given file path and line number.
pub async fn get_context(engine: &Arc<RetrievalEngine>, args: &Value) -> Result<ToolResult> {
    let file = args
        .get("file")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let line = args.get("line").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

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
pub async fn search_code(_engine: &Arc<RetrievalEngine>, args: &Value) -> Result<ToolResult> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .unwrap_or("(empty query)");

    // TODO: Implement actual vector search when embedding model is integrated.
    // For now, return a placeholder indicating the feature is planned.
    Ok(ToolResult {
        content: vec![ToolResultContent {
            content_type: "text".to_string(),
            text: format!(
                "Semantic search for '{}' is not yet fully implemented.\n\n\
                The vector store is ready, but an embedding model needs to be integrated \
                to convert text queries into vectors for similarity search.",
                query
            ),
        }],
        is_error: None,
    })
}
