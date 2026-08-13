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

const DEFAULT_MAX_LIMIT: usize = 50;
const DEFAULT_MAX_BODY_CHARS: usize = 4_000;

fn format_suggestions_output(
    suggestions: &[ccm_core::engine::ContextSuggestion],
    include_body: bool,
    max_body_chars: usize,
) -> String {
    let mut output = String::new();
    let mut total_chars = 0usize;
    let mut truncated = 0usize;
    for suggestion in suggestions {
        output.push_str(&format!(
            "## {} (Score: {:.2})\n**Reason:** {}\n{}",
            suggestion.title,
            suggestion.relevance_score,
            suggestion.reason,
            format_suggestion_metadata(suggestion),
        ));
        if include_body && !suggestion.content.is_empty() {
            let remaining = max_body_chars.saturating_sub(total_chars);
            if remaining > 0 {
                let body: String = suggestion.content.chars().take(remaining).collect();
                output.push_str(&format!("```\n{}\n```", body));
                total_chars += body.chars().count();
                if body.chars().count() < suggestion.content.chars().count() {
                    truncated += 1;
                    output.push_str("\n*(body truncated)*");
                }
            } else {
                truncated += 1;
            }
        }
        output.push_str("\n\n---\n");
    }
    if truncated > 0 {
        output.push_str(&format!(
            "_Note: {} result body or bodies were truncated by max_chars._\n",
            truncated
        ));
    }
    output
}

fn limit_from_args(args: &Value, default: usize) -> usize {
    args.get("limit")
        .and_then(|v| v.as_u64())
        .map(|v| (v as usize).clamp(1, DEFAULT_MAX_LIMIT))
        .unwrap_or(default)
}

fn max_chars_from_args(args: &Value) -> usize {
    args.get("max_chars")
        .and_then(|v| v.as_u64())
        .map(|v| v as usize)
        .unwrap_or(DEFAULT_MAX_BODY_CHARS)
}

fn include_body_from_args(args: &Value) -> bool {
    args.get("include_body")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
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
            text: format_suggestions_output(
                &suggestions,
                include_body_from_args(args),
                max_chars_from_args(args),
            ),
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

    let limit = limit_from_args(args, 5);
    let hits = engine.search_code_hybrid(query, limit).await?;
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
            text: format_suggestions_output(
                &hits,
                include_body_from_args(args),
                max_chars_from_args(args),
            ),
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

    let limit = limit_from_args(args, 10);
    let matches = engine.find_graph_nodes(query, limit).await;

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
            text: format_suggestions_output(
                &matches,
                include_body_from_args(args),
                max_chars_from_args(args),
            ),
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
            "## Node Details: {}\n\n**Type:** {:?}\n**ID:** {}\n**Range:** Lines {}-{}",
            node.name, node.node_type, node.id, node.start_line, node.end_line
        );
        if include_body_from_args(args) && !node.content.is_empty() {
            let max_chars = max_chars_from_args(args);
            let body: String = node.content.chars().take(max_chars).collect();
            output.push_str(&format!("\n\n```\n{}\n```", body));
            if body.chars().count() < node.content.chars().count() {
                output.push_str("\n*(body truncated by max_chars)*");
            }
        }

        // Append neighbors if available (Graph Navigator)
        if let Some(neighbors) = engine.get_node_neighbors(&node.id).await {
            output.push_str("\n\n### Graph Connections\n");

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
    if let Some((path_and_kind, suffix)) = node_id.split_once(":symbol:") {
        let (path_part, kind_part) = path_and_kind
            .rsplit_once(':')
            .ok_or_else(|| anyhow::anyhow!("Stable node ID is missing its node kind"))?;
        return Ok(format!(
            "{}:{}:symbol:{}",
            normalize_graph_path(path_part, project_path)?,
            kind_part,
            suffix
        ));
    }

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
pub async fn index_project(
    state: Arc<crate::server::ServerState>,
    args: &Value,
) -> Result<ToolResult> {
    let project_path = args
        .get("project_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'project_path' argument"))?;

    let canonical_path = std::fs::canonicalize(project_path)
        .unwrap_or_else(|_| std::path::PathBuf::from(project_path));
    let job_key = canonical_path.to_string_lossy().to_string();

    let existing = state.index_jobs.lock().unwrap().get(&job_key).cloned();
    if let Some(receiver) = existing {
        if let Some(result) = receiver.borrow().clone() {
            state.index_jobs.lock().unwrap().remove(&job_key);
            return Ok(result);
        }
        return Ok(index_in_progress_result(project_path));
    }

    tracing::info!(path = %project_path, "Starting manual index");
    let (sender, mut receiver) = tokio::sync::watch::channel(None);
    state
        .index_jobs
        .lock()
        .unwrap()
        .insert(job_key.clone(), receiver.clone());

    let job_state = state.clone();
    let job_path = project_path.to_string();
    tokio::spawn(async move {
        let worker_state = job_state.clone();
        let worker_path = job_path.clone();
        let worker =
            tokio::spawn(async move { run_index_project(&worker_state, &worker_path).await });
        let result = match worker.await {
            Ok(result) => result,
            Err(error) => index_task_failed_result(&job_path, &error.to_string()),
        };
        let _ = sender.send(Some(result));
    });

    let timeout_ms = std::env::var("CCM_INDEX_RESPONSE_TIMEOUT_MS").ok();
    let timeout_secs = std::env::var("CCM_INDEX_RESPONSE_TIMEOUT_SECS").ok();
    let wait_duration = index_response_timeout(timeout_ms.as_deref(), timeout_secs.as_deref());

    match tokio::time::timeout(wait_duration, receiver.changed()).await {
        Ok(Ok(())) => {
            if let Some(result) = receiver.borrow().clone() {
                state.index_jobs.lock().unwrap().remove(&job_key);
                return Ok(result);
            }
        }
        Ok(Err(error)) => {
            state.index_jobs.lock().unwrap().remove(&job_key);
            return Ok(index_task_failed_result(project_path, &error.to_string()));
        }
        Err(_) => {}
    }

    Ok(index_started_result(project_path))
}

fn index_response_timeout(
    timeout_ms: Option<&str>,
    timeout_secs: Option<&str>,
) -> std::time::Duration {
    const DEFAULT_MILLIS: u64 = 30_000;
    const MAX_MILLIS: u64 = 60_000;

    let millis = timeout_ms
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .or_else(|| {
            timeout_secs
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|value| *value > 0)
                .map(|value| value.saturating_mul(1_000))
        })
        .unwrap_or(DEFAULT_MILLIS)
        .min(MAX_MILLIS);
    std::time::Duration::from_millis(millis)
}

fn index_started_result(project_path: &str) -> ToolResult {
    ToolResult {
        content: vec![ToolResultContent {
            content_type: "text".to_string(),
            text: format!(
                "Project indexing started in the background for {}. Call index_project again to check status; other tools become available when indexing completes.",
                project_path
            ),
        }],
        is_error: None,
    }
}

fn index_in_progress_result(project_path: &str) -> ToolResult {
    ToolResult {
        content: vec![ToolResultContent {
            content_type: "text".to_string(),
            text: format!(
                "Project indexing is still in progress for {}. Call index_project again later to retrieve the final result.",
                project_path
            ),
        }],
        is_error: None,
    }
}

fn index_task_failed_result(project_path: &str, detail: &str) -> ToolResult {
    ToolResult {
        content: vec![ToolResultContent {
            content_type: "text".to_string(),
            text: format!(
                "Background indexing failed for {}: {}. Call index_project to retry.",
                project_path, detail
            ),
        }],
        is_error: Some(true),
    }
}

async fn run_index_project(state: &crate::server::ServerState, project_path: &str) -> ToolResult {
    let lock = state.project_index_lock(project_path);
    let _guard = lock.lock().await;

    // Shared db path inside project
    let db_path = std::path::Path::new(project_path)
        .join("data/ccm_db") // Use shared db folder
        .to_string_lossy()
        .to_string();

    match ccm_core::update_index(project_path, Some(&db_path)).await {
        Ok(stats) => {
            // Refresh both the explicit project cache and the default startup engine.
            if let Err(error) = state.refresh_project_engine(project_path).await {
                return ToolResult {
                    content: vec![ToolResultContent {
                        content_type: "text".to_string(),
                        text: format!(
                            "Indexing completed but the engine refresh failed: {}",
                            error
                        ),
                    }],
                    is_error: Some(true),
                };
            }

            let message = if stats.files_indexed == 0
                && stats.files_failed == 0
                && stats.files_skipped == 0
                && stats.nodes_created == 0
            {
                "No changes detected. Existing index is already up to date.".to_string()
            } else {
                let mut lines = vec![
                    "Project index refreshed successfully.".to_string(),
                    String::new(),
                    "Stats:".to_string(),
                    format!("- Files Indexed: {}", stats.files_indexed),
                    format!("- Files Failed: {}", stats.files_failed),
                    format!("- Files Skipped: {}", stats.files_skipped),
                    format!("- Nodes Created: {}", stats.nodes_created),
                ];

                if !stats.reason_counts.is_empty() {
                    lines.push(String::new());
                    lines.push("Issue Breakdown:".to_string());
                    let mut reasons: Vec<_> = stats.reason_counts.iter().collect();
                    reasons.sort_by(|left, right| right.1.cmp(left.1));
                    for (reason, count) in reasons {
                        lines.push(format!("- {}: {}", reason, count));
                    }
                }

                if !stats.failed_files.is_empty() {
                    lines.push(String::new());
                    lines.push("Failed Files (sample):".to_string());
                    for issue in stats.failed_files.iter().take(10) {
                        lines.push(format!(
                            "- {} [{}] {}",
                            issue.path,
                            issue.reason.as_str(),
                            issue.detail
                        ));
                    }
                }

                if !stats.suggested_ignores.is_empty() {
                    lines.push(String::new());
                    lines.push("Suggested Ignore Patterns:".to_string());
                    for pattern in stats.suggested_ignores.iter().take(10) {
                        lines.push(format!("- {}", pattern));
                    }
                }

                lines.push(String::new());
                lines.push(
                    "The project is ready for semantic search and graph navigation.".to_string(),
                );
                lines.join("\n")
            };
            ToolResult {
                content: vec![ToolResultContent {
                    content_type: "text".to_string(),
                    text: message,
                }],
                is_error: None,
            }
        }
        Err(e) => {
            let error_msg = format!("Indexing failed: {}", e);
            tracing::warn!(error = %e, "Indexing failed");
            ToolResult {
                content: vec![ToolResultContent {
                    content_type: "text".to_string(),
                    text: error_msg,
                }],
                is_error: Some(true),
            }
        }
    }
}

/// Tool: find_usages
/// Verilen node_id'yi çağıran / kullanan tüm node'ları döndürür.
pub async fn find_usages(engine: &Arc<RetrievalEngine>, args: &Value) -> Result<ToolResult> {
    let node_id = args
        .get("node_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'node_id' argument"))?;

    if node_id.trim().is_empty() {
        return Ok(ToolResult {
            content: vec![ToolResultContent {
                content_type: "text".to_string(),
                text: "Error: 'node_id' cannot be empty.".to_string(),
            }],
            is_error: Some(true),
        });
    }

    let project_path = args.get("project_path").and_then(|v| v.as_str());
    let normalized_id = normalize_graph_node_id(node_id, project_path)?;
    let limit = limit_from_args(args, 20);
    let usages = engine.find_usages(&normalized_id, limit).await;

    if usages.is_empty() {
        return Ok(ToolResult {
            content: vec![ToolResultContent {
                content_type: "text".to_string(),
                text: format!("No usages found for node: '{}'", node_id),
            }],
            is_error: None,
        });
    }

    Ok(ToolResult {
        content: vec![ToolResultContent {
            content_type: "text".to_string(),
            text: format!(
                "## Usages of `{}` ({} found)\n\n{}",
                node_id,
                usages.len(),
                format_suggestions_output(
                    &usages,
                    include_body_from_args(args),
                    max_chars_from_args(args),
                )
            ),
        }],
        is_error: None,
    })
}

/// Tool: trace_call_chain
/// from_id'den to_id'ye giden çağrı zincirini BFS ile bulur.
pub async fn trace_call_chain(engine: &Arc<RetrievalEngine>, args: &Value) -> Result<ToolResult> {
    let from_id = args
        .get("from_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'from_id' argument"))?;
    let to_id = args
        .get("to_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'to_id' argument"))?;

    let project_path = args.get("project_path").and_then(|v| v.as_str());
    let normalized_from = normalize_graph_node_id(from_id, project_path)?;
    let normalized_to = normalize_graph_node_id(to_id, project_path)?;
    let max_depth = args.get("max_depth").and_then(|v| v.as_u64()).unwrap_or(8) as usize;

    let chain = engine
        .trace_call_chain(&normalized_from, &normalized_to, max_depth)
        .await;

    if chain.is_empty() {
        return Ok(ToolResult {
            content: vec![ToolResultContent {
                content_type: "text".to_string(),
                text: format!(
                    "No call chain found from '{}' to '{}' within {} hops.",
                    from_id, to_id, max_depth
                ),
            }],
            is_error: None,
        });
    }

    Ok(ToolResult {
        content: vec![ToolResultContent {
            content_type: "text".to_string(),
            text: format!(
                "## Call Chain: {} → {} ({} steps)\n\n{}",
                from_id,
                to_id,
                chain.len(),
                format_suggestions_output(
                    &chain,
                    include_body_from_args(args),
                    max_chars_from_args(args),
                )
            ),
        }],
        is_error: None,
    })
}

/// Tool: impact_of_change
/// Bir dosya değiştiğinde etkilenecek tüm node ve dosyaları listeler.
pub async fn impact_of_change(engine: &Arc<RetrievalEngine>, args: &Value) -> Result<ToolResult> {
    let file = args
        .get("file")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'file' argument"))?;

    if file.trim().is_empty() {
        return Ok(ToolResult {
            content: vec![ToolResultContent {
                content_type: "text".to_string(),
                text: "Error: 'file' cannot be empty.".to_string(),
            }],
            is_error: Some(true),
        });
    }

    let project_path = args.get("project_path").and_then(|v| v.as_str());
    let normalized = normalize_graph_path(file, project_path)?;
    let limit = limit_from_args(args, 30);
    let impacted = engine.impact_of_change(&normalized, limit).await;

    if impacted.is_empty() {
        return Ok(ToolResult {
            content: vec![ToolResultContent {
                content_type: "text".to_string(),
                text: format!(
                    "No dependents found for '{}'. The file may not be indexed or has no callers.",
                    file
                ),
            }],
            is_error: None,
        });
    }

    Ok(ToolResult {
        content: vec![ToolResultContent {
            content_type: "text".to_string(),
            text: format!(
                "## Impact of changing `{}` ({} dependents)\n\n{}",
                file,
                impacted.len(),
                format_suggestions_output(
                    &impacted,
                    include_body_from_args(args),
                    max_chars_from_args(args),
                )
            ),
        }],
        is_error: None,
    })
}

/// Tool: diff_context
/// Son N günde commit edilen dosyaların graph node'larını döndürür.
pub async fn diff_context(engine: &Arc<RetrievalEngine>, args: &Value) -> Result<ToolResult> {
    let project_path = args
        .get("project_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'project_path' argument"))?;

    let days = args.get("days").and_then(|v| v.as_u64()).unwrap_or(7) as u32;
    let limit = limit_from_args(args, 30);
    let nodes = engine.diff_context(project_path, days, limit).await;

    if nodes.is_empty() {
        return Ok(ToolResult {
            content: vec![ToolResultContent {
                content_type: "text".to_string(),
                text: format!(
                    "No changed code found in the last {} day(s). The project may not be a git repo or nothing has been committed recently.",
                    days
                ),
            }],
            is_error: None,
        });
    }

    Ok(ToolResult {
        content: vec![ToolResultContent {
            content_type: "text".to_string(),
            text: format!(
                "## Recently Changed Code (last {} days, {} nodes)\n\n{}",
                days,
                nodes.len(),
                format_suggestions_output(
                    &nodes,
                    include_body_from_args(args),
                    max_chars_from_args(args),
                )
            ),
        }],
        is_error: None,
    })
}

#[cfg(test)]
mod tests {
    use super::{index_response_timeout, normalize_graph_node_id, normalize_graph_path};
    use std::time::Duration;

    #[test]
    fn index_response_timeout_is_capped_below_client_timeout() {
        assert_eq!(
            index_response_timeout(Some("999999"), None),
            Duration::from_secs(60)
        );
        assert_eq!(
            index_response_timeout(None, Some("999")),
            Duration::from_secs(60)
        );
        assert_eq!(
            index_response_timeout(Some("1"), Some("999")),
            Duration::from_millis(1)
        );
    }

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

    #[test]
    fn stable_node_id_keeps_file_path_and_symbol_suffix() {
        let id = "./src/detector/yolo.py:class_definition:symbol:0123456789abcdef:0";
        assert_eq!(normalize_graph_node_id(id, None).unwrap(), id);
    }
}
