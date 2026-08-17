//! PolicyStore: disk üzerinde versioned policy'ler ve append-only history.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{RetrievalPolicy, POLICY_SCHEMA_VERSION};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyStore {
    pub schema_version: u32,
    pub baseline: RetrievalPolicy,
    pub active_id: Option<u32>,
    pub policies: Vec<RetrievalPolicy>,
}

impl PolicyStore {
    pub fn new(baseline: RetrievalPolicy) -> Self {
        Self {
            schema_version: POLICY_SCHEMA_VERSION,
            baseline,
            active_id: None,
            policies: Vec::new(),
        }
    }

    pub fn active(&self) -> &RetrievalPolicy {
        self.active_id
            .and_then(|id| self.policies.iter().find(|policy| policy.version == id))
            .unwrap_or(&self.baseline)
    }

    pub fn add_candidate(&mut self, policy: RetrievalPolicy) {
        if !self.policies.iter().any(|p| p.version == policy.version) {
            self.policies.push(policy);
        }
    }

    pub fn activate(&mut self, version: u32) {
        if self.policies.iter().any(|p| p.version == version) {
            self.active_id = Some(version);
        }
    }

    pub fn rollback(&mut self) {
        self.active_id = None;
    }

    pub fn load(path: &Path) -> Result<Self> {
        let file = std::fs::File::open(path)
            .with_context(|| format!("policy store açılamadı: {}", path.display()))?;
        serde_json::from_reader(std::io::BufReader::new(file))
            .with_context(|| format!("policy store JSON çözümlenemedi: {}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("policy store dizini oluşturulamadı: {}", parent.display())
            })?;
        }
        #[cfg(unix)]
        let file = {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(path)
        }
        .with_context(|| format!("policy store yazılamadı: {}", path.display()))?;
        #[cfg(not(unix))]
        let file = std::fs::File::create(path)
            .with_context(|| format!("policy store yazılamadı: {}", path.display()))?;
        serde_json::to_writer_pretty(std::io::BufWriter::new(file), self)?;
        Ok(())
    }

    pub fn default_dir() -> PathBuf {
        std::env::var("CCM_LEARN_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("data/ccm_learn"))
    }

    pub fn default_policies_path() -> PathBuf {
        Self::default_dir().join("policies.json")
    }

    pub fn default_history_path() -> PathBuf {
        Self::default_dir().join("history.jsonl")
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PromotionResult {
    Promoted,
    Rejected,
    RolledBack,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyMetrics {
    pub tasks_scored: usize,
    pub pass_rate: f64,
    pub mean_recall_at_k: f64,
    pub mean_tokens: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PolicyHistoryEntry {
    pub policy_id: u32,
    pub parent_id: Option<u32>,
    pub created_at: u64,
    pub task_type: String,
    pub params: RetrievalPolicy,
    pub train_metrics: Option<PolicyMetrics>,
    pub holdout_metrics: Option<PolicyMetrics>,
    pub promotion_result: PromotionResult,
    pub overfit_flag: Option<String>,
    pub reason: String,
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn append_history(path: &Path, entry: &PolicyHistoryEntry) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("history dizini oluşturulamadı: {}", parent.display()))?;
    }
    let mut line = serde_json::to_string(entry)?;
    line.push('\n');
    use std::io::Write;
    #[cfg(unix)]
    let mut file = {
        use std::os::unix::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(path)
            .with_context(|| format!("history açılamadı: {}", path.display()))?
    };
    #[cfg(not(unix))]
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("history açılamadı: {}", path.display()))?;
    file.write_all(line.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy::TaskType;

    #[test]
    fn store_roundtrip_preserves_active_policy() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = PolicyStore::new(RetrievalPolicy::baseline());
        let candidate = RetrievalPolicy::baseline_for(TaskType::Feature);
        let candidate = RetrievalPolicy {
            version: 2,
            ..candidate
        };
        store.add_candidate(candidate.clone());
        store.activate(candidate.version);
        let path = dir.path().join("policies.json");
        store.save(&path).expect("save");

        let loaded = PolicyStore::load(&path).expect("load");
        assert_eq!(loaded.active_id, Some(2));
        assert_eq!(loaded.active().task_type, TaskType::Feature);
    }

    #[test]
    fn active_falls_back_to_baseline_when_unset() {
        let store = PolicyStore::new(RetrievalPolicy::baseline());
        assert_eq!(store.active().version, 1);
        assert_eq!(store.active().task_type, TaskType::Unknown);
    }
}
