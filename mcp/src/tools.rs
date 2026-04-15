//! MCP Tool implementations that bridge to ccm-core.

use anyhow::Result;
use serde_json::Value;
use std::path::{Component, Path};
use std::sync::Arc;

use crate::protocol::{ToolResult, ToolResultContent};
use ccm_core::engine::{CursorPosition, RetrievalEngine};

fn format_suggestion_metadata(suggestion: &ccm_core::engine::ContextSuggestion) -> String {
    let mut metadata = Vec::new();

    if let Some(node_id) = &suggestion.node_id {
        metadata.push(format!("**Node ID:** {}", node_id));
    }
    if let Some(file_path) = &suggestion.file_path {
        metadata.push(format!("**File:** {}", file_path));
    }
    if let Some(node_type) = &suggestion.node_type {
        metadata.push(format!("**Node Type:** {}", node_type));
    }
    match (suggestion.start_line, suggestion.end_line) {
        (Some(start), Some(end)) => metadata.push(format!("**Range:** {}-{}", start, end)),
        (Some(start), None) => metadata.push(format!("**Line:** {}", start)),
        _ => {}
    }

    if metadata.is_empty() {
        String::new()
    } else {
        format!("{}\n\n", metadata.join("\n"))
    }
}

fn format_suggestions_output(suggestions: &[ccm_core::engine::ContextSuggestion]) -> String {
    let mut output = String::new();
    for suggestion in suggestions {
        output.push_str(&format!(
            "## {} (Score: {:.2})\n**Reason:** {}\n{}\n```\n{}\n```\n\n---\n",
            suggestion.title,
            suggestion.relevance_score,
            suggestion.reason,
            format_suggestion_metadata(suggestion),
            suggestion.content
        ));
    }
    output
}

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
    let normalized_file = normalize_graph_path(file, project_path)?;

    let cursor = CursorPosition {
        file_path: normalized_file.clone(),
        line,
        column: 0,
    };

    let suggestions = engine.predict_context(&cursor).await?;

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

    Ok(ToolResult {
        content: vec![ToolResultContent {
            content_type: "text".to_string(),
            text: format_suggestions_output(&suggestions),
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

    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(5) as usize;
    let hits = engine.search_code_hybrid(query, limit.max(1)).await?;
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

    Ok(ToolResult {
        content: vec![ToolResultContent {
            content_type: "text".to_string(),
            text: format_suggestions_output(&hits),
        }],
        is_error: None,
    })
}

/// Tool: find_nodes
/// Finds graph nodes by name, path, or node ID fragment.
pub async fn find_nodes(engine: &Arc<RetrievalEngine>, args: &Value) -> Result<ToolResult> {
    let query = args
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing query argument"))?;

    if query.trim().is_empty() {
        return Ok(ToolResult {
            content: vec![ToolResultContent {
                content_type: "text".to_string(),
                text: "Error: 'query' cannot be empty. Please provide a node name, file path fragment, or node ID fragment.".to_string(),
            }],
            is_error: Some(true),
        });
    }

    let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
    let matches = engine.find_graph_nodes(query, limit.max(1)).await;

    if matches.is_empty() {
        return Ok(ToolResult {
            content: vec![ToolResultContent {
                content_type: "text".to_string(),
                text: format!("No graph nodes found for query: '{}'", query),
            }],
            is_error: None,
        });
    }

    Ok(ToolResult {
        content: vec![ToolResultContent {
            content_type: "text".to_string(),
            text: format_suggestions_output(&matches),
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
    let normalized_id = normalize_graph_node_id(node_id, project_path)?;

    let node_opt = engine.get_node_by_id(&normalized_id).await;

    if let Some(node) = node_opt {
        let mut output = format!(
            "## Node Details: {}\n\n**Type:** {:?}\n**ID:** {}\n**Range:** Lines {}-{}\n\n```\n{}\n```",
            node.name, node.node_type, node.id, node.start_line, node.end_line, node.content
        );

        // Append neighbors if available (Graph Navigator)
        if let Some(neighbors) = engine.get_node_neighbors(&node.id).await {
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
fn normalize_graph_path(path_str: &str, project_path: Option<&str>) -> Result<String> {
    if path_str.is_empty() {
        return Err(anyhow::anyhow!("Path argument cannot be empty"));
    }

    let normalized_input = path_str.replace('\\', "/");
    if normalized_input.contains('\0') {
        return Err(anyhow::anyhow!("Path argument contains invalid null byte"));
    }

    let path = Path::new(&normalized_input);
    if path.is_absolute() {
        let root = project_path.ok_or_else(|| {
            anyhow::anyhow!("Absolute paths require a matching 'project_path' argument")
        })?;
        let stripped = path
            .strip_prefix(Path::new(root))
            .map_err(|_| anyhow::anyhow!("Path is outside the provided project root"))?;
        return normalize_relative_graph_path(stripped);
    }

    normalize_relative_graph_path(path)
}

fn normalize_graph_node_id(node_id: &str, project_path: Option<&str>) -> Result<String> {
    let mut parts = node_id.rsplitn(4, ':');
    let column = parts.next();
    let line = parts.next();
    let kind = parts.next();
    let path = parts.next();

    match (path, kind, line, column) {
        (Some(path_part), Some(kind_part), Some(line_part), Some(column_part)) => Ok(format!(
            "{}:{}:{}:{}",
            normalize_graph_path(path_part, project_path)?,
            kind_part,
            line_part,
            column_part
        )),
        _ => normalize_graph_path(node_id, project_path),
    }
}

fn normalize_relative_graph_path(path: &Path) -> Result<String> {
    let mut parts = Vec::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(segment) => {
                let text = segment
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("Path contains non-UTF8 segment"))?;
                parts.push(text.to_string());
            }
            Component::ParentDir => {
                return Err(anyhow::anyhow!(
                    "Parent directory segments are not allowed in MCP path arguments"
                ));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(anyhow::anyhow!(
                    "Absolute paths are not allowed in MCP path arguments"
                ));
            }
        }
    }

    if parts.is_empty() {
        return Err(anyhow::anyhow!("Path must resolve to a file or node path"));
    }

    Ok(format!("./{}", parts.join("/")))
}

/// Tool: index_project
/// Manually triggers a re-index of the specified project.
pub async fn index_project(state: &crate::server::ServerState, args: &Value) -> Result<ToolResult> {
    let project_path = args
        .get("project_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'project_path' argument"))?;

    tracing::info!(path = %project_path, "Starting manual index");

    // Shared db path inside project
    let db_path = std::path::Path::new(project_path)
        .join("data/ccm_db") // Use shared db folder
        .to_string_lossy()
        .to_string();

    match ccm_core::update_index(project_path, Some(&db_path)).await {
        Ok(stats) => {
            // Invalidate the cache for this project so the next search uses the fresh index
            {
                let mut write = state.engines.write().await;
                write.evict(project_path);
            }

            let message = if stats.files_indexed == 0
                && stats.files_failed == 0
                && stats.nodes_created == 0
            {
                "No changes detected. Existing index is already up to date.".to_string()
            } else {
                format!(
                    "Project index refreshed successfully.\n\nStats:\n- Files Indexed: {}\n- Files Failed: {}\n- Nodes Created: {}\n\nThe project is ready for semantic search and graph navigation.",
                    stats.files_indexed, stats.files_failed, stats.nodes_created
                )
            };
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

#[cfg(test)]
mod tests {
    use super::{normalize_graph_node_id, normalize_graph_path};

    #[test]
    fn relative_paths_get_graph_prefix() {
        let normalized = normalize_graph_path("src/lib.rs", None).unwrap();
        assert_eq!(normalized, "./src/lib.rs");
    }

    #[test]
    fn parent_segments_are_rejected() {
        let error = normalize_graph_path("../secret.txt", None).unwrap_err();
        assert!(error.to_string().contains("Parent directory"));
    }

    #[test]
    fn node_ids_normalize_only_the_path_prefix() {
        let normalized = normalize_graph_node_id("src/lib.rs:function_item:12:0", None).unwrap();
        assert_eq!(normalized, "./src/lib.rs:function_item:12:0");
    }
}
