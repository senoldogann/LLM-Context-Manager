//! Observable-only retrieval experience log.
//!
//! Yalnızca gözlemlenebilir olayları kaydeder (query, dönen node id'leri, rank,
//! skor, token, latency, policy version). Feedback ("dosya düzenlendi", "test
//! geçti") bu iterasyonda üretilmez — uydurulmaz. Varsayılan kapalı:
//! `CCM_TRAJECTORY_LOG=1` ise fire-and-forget JSONL append.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::policy::TaskType;

#[derive(Debug, Clone)]
pub struct TrajectoryContext {
    pub tool_name: String,
    pub request_id: Option<String>,
}

tokio::task_local! {
    static TRAJECTORY_CONTEXT: TrajectoryContext;
}

pub async fn with_context<F>(context: TrajectoryContext, future: F) -> F::Output
where
    F: Future,
{
    TRAJECTORY_CONTEXT.scope(context, future).await
}

pub fn current_context() -> Option<TrajectoryContext> {
    TRAJECTORY_CONTEXT.try_with(Clone::clone).ok()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalResultItem {
    pub node_id: Option<String>,
    pub file_path: Option<String>,
    pub relevance_score: f32,
    pub rank: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalEvent {
    pub session_id: String,
    #[serde(default)]
    pub tool_name: Option<String>,
    #[serde(default)]
    pub request_id: Option<String>,
    pub task_type: TaskType,
    pub policy_version: u32,
    pub query: Option<String>,
    pub cursor: Option<CursorRef>,
    pub results: Vec<RetrievalResultItem>,
    pub estimated_tokens: usize,
    pub latency_ms: u64,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorRef {
    pub file_path: String,
    pub line: usize,
}

/// `CCM_TRAJECTORY_LOG=1` ise event'i varsayılan konuma ekler.
/// Konum: `CCM_TRAJECTORY_PATH` ya da `./data/ccm_learn/experiences.jsonl`.
pub fn record_if_enabled(event: RetrievalEvent) {
    if !trajectory_enabled() {
        return;
    }
    if let Err(error) = append_event(&default_path(), &event) {
        tracing::warn!(error = %error, "trajectory event yazılamadı");
    }
}

/// Trajectory aktif mi? (MCP layer env bağlamını koşullu kurmak için kullanır.)
pub fn trajectory_enabled() -> bool {
    std::env::var("CCM_TRAJECTORY_LOG")
        .map(|value| matches!(value.to_lowercase().as_str(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

pub fn append_event(path: &PathBuf, event: &RetrievalEvent) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut line = serde_json::to_string(event)?;
    line.push('\n');
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    file.write_all(line.as_bytes())?;
    Ok(())
}

fn default_path() -> PathBuf {
    std::env::var("CCM_TRAJECTORY_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data/ccm_learn/experiences.jsonl"))
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_event_writes_jsonl_lines() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("experiences.jsonl");
        let event = RetrievalEvent {
            session_id: "s1".into(),
            tool_name: Some("search_code".into()),
            request_id: Some("req-1".into()),
            task_type: TaskType::BugFix,
            policy_version: 1,
            query: Some("find where compute_tax is implemented".into()),
            cursor: None,
            results: vec![RetrievalResultItem {
                node_id: Some("./src/tax.rs:function_item:1:0".into()),
                file_path: Some("./src/tax.rs".into()),
                relevance_score: 0.9,
                rank: 1,
            }],
            estimated_tokens: 120,
            latency_ms: 3,
            timestamp_ms: 1,
        };
        append_event(&path, &event).expect("append");
        append_event(&path, &event).expect("append");
        let content = std::fs::read_to_string(&path).expect("read");
        assert_eq!(content.lines().count(), 2);
        let parsed: RetrievalEvent =
            serde_json::from_str(content.lines().next().unwrap()).expect("parse jsonl");
        assert_eq!(parsed.results[0].rank, 1);
    }

    #[tokio::test]
    async fn trajectory_context_is_scoped_to_the_current_task() {
        assert!(current_context().is_none());
        let context = TrajectoryContext {
            tool_name: "search_code".to_string(),
            request_id: Some("42".to_string()),
        };
        with_context(context, async {
            let active = current_context().expect("scoped context");
            assert_eq!(active.tool_name, "search_code");
            assert_eq!(active.request_id.as_deref(), Some("42"));
        })
        .await;
        assert!(current_context().is_none());
    }
}
