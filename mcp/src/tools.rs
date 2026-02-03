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

    let project_path = args.get("project_path").and_then(|v| v.as_str());
    let normalized_file = try_normalize_path(file, project_path);

    // Try with normalized path first
    let cursor = CursorPosition {
        file_path: normalized_file.clone(),
        line,
        column: 0,
    };

    let mut suggestions = engine.predict_context(&cursor).await?;

    // If no suggestions, try with original file path (fallback)
    if suggestions.is_empty() && normalized_file != file {
        let cursor_original = CursorPosition {
            file_path: file.to_string(),
            line,
            column: 0,
        };
        suggestions = engine.predict_context(&cursor_original).await?;
    }

    if suggestions.is_empty() {
        return Ok(ToolResult {
            content: vec![ToolResultContent {
                content_type: "text".to_string(),
                text: format!(
                    "No context found for {}:{} (Normalized: {})\n\nThe code graph may not be indexed yet. Try indexing the project first.",
                    file, line, normalized_file
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
    tracing::debug!(hits = hits.len(), query = %query, "search_code results");

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

    let project_path = args.get("project_path").and_then(|v| v.as_str());
    let normalized_id = try_normalize_path(node_id, project_path);

    // Try finding by normalized ID first, then raw ID
    // Try finding by normalized ID first, then raw ID
    let mut node_opt = engine.get_node_by_id(&normalized_id).await;
    if node_opt.is_none() {
        node_opt = engine.get_node_by_id(node_id).await;
    }

    if let Some(node) = node_opt {
        let mut output = format!(
            "## Node Details: {}\n\n**Type:** {:?}\n**ID:** {}\n**Range:** Lines {}-{}\n\n```\n{}\n```",
            node.name, node.node_type, node.id, node.start_line, node.end_line, node.content
        );

        // Append neighbors if available (Graph Navigator)
        // Try getting neighbors with both IDs as well
        let mut neighbors_opt = engine.get_node_neighbors(&node.id).await;
        if neighbors_opt.is_none() {
            neighbors_opt = engine.get_node_neighbors(node_id).await;
        }

        if let Some(neighbors) = neighbors_opt {
            output.push_str("\n\n### 🔗 Graph Connections\n");

            if !neighbors.calls.is_empty() {
                output.push_str(&format!("**Calls:** {}\n", neighbors.calls.join(", ")));
            }
            if !neighbors.called_by.is_empty() {
                output.push_str(&format!(
                    "**Called By:** {}\n",
                    neighbors.called_by.join(", ")
                ));
            }
            if !neighbors.contains.is_empty() {
                output.push_str(&format!(
                    "**Contains:** {}\n",
                    neighbors.contains.join(", ")
                ));
            }

            if neighbors.calls.is_empty()
                && neighbors.called_by.is_empty()
                && neighbors.contains.is_empty()
            {
                output.push_str("_(No direct connections found)_");
            }
        }

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
                text: format!(
                    "Node not found with ID: {} (Normalized: {})",
                    node_id, normalized_id
                ),
            }],
            is_error: Some(true),
        })
    }
}

/// Helper: Normalizes a file path to match graph conventions (relative, starts with ./)
fn try_normalize_path(path_str: &str, project_path: Option<&str>) -> String {
    // 1. Try stripping project root if absolute
    if let Some(root) = project_path {
        let path = std::path::Path::new(path_str);
        if path.is_absolute() {
            if let Ok(stripped) = path.strip_prefix(root) {
                let s = stripped.to_string_lossy();
                if s.starts_with("./") {
                    return s.to_string();
                }
                return format!("./{}", s);
            }
        }
    }

    // 2. If relative but missing ./ prefix, add it
    if !path_str.starts_with("./") && !path_str.starts_with("/") {
        return format!("./{}", path_str);
    }

    path_str.to_string()
}

/// Tool: index_project
/// Manually triggers a re-index of the specified project.
pub async fn index_project(args: &Value) -> Result<ToolResult> {
    let project_path = args
        .get("project_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'project_path' argument"))?;

    tracing::info!(path = %project_path, "Starting manual index");

    // Default db path relative to project
    let db_path = std::path::Path::new(project_path)
        .join("data/ccm_mcp_db") // Use mcp specific db folder to separate from CLI
        .to_string_lossy()
        .to_string();

    match ccm_core::index_directory(project_path, Some(&db_path)).await {
        Ok(stats) => {
            let message = format!(
                "Debugging: Indexing completed successfully.\n\nStats:\n- Files Indexed: {}\n- Files Failed: {}\n- Nodes Created: {}\n\nThe project is now ready for semantic search and graph navigation.",
                stats.files_indexed, stats.files_failed, stats.nodes_created
            );
            Ok(ToolResult {
                content: vec![ToolResultContent {
                    content_type: "text".to_string(),
                    text: message,
                }],
                is_error: None,
            })
        }
        Err(e) => {
            let error_msg = format!("Indexing failed: {}", e);
            tracing::warn!(error = %e, "Indexing failed");
            Ok(ToolResult {
                content: vec![ToolResultContent {
                    content_type: "text".to_string(),
                    text: error_msg,
                }],
                is_error: Some(true),
            })
        }
    }
}
