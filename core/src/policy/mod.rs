//! Versioned retrieval policy ve policy store.
//!
//! Self-improvement kapsamında mevcut sabit hybrid ayarlar (ağırlıklar, kenar
//! ağırlıkları, top-k, pencere büyüklükleri, eşikler) versioned bir
//! `RetrievalPolicy` yapısına taşınır. Baseline, bugünkü varsayılan davranışın
//! birebir anlık görüntüsüdür; yalnızca holdout'ta kanıtlanan iyileşme sonrası
//! yeni policy aktif olabilir.

pub mod gate;
pub mod store;

use serde::{Deserialize, Serialize};

use crate::engine::hybrid::{default_edge_weights, HybridWeights};
use crate::graph::EdgeType;

pub const POLICY_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    BugFix,
    Feature,
    Refactor,
    Investigation,
    Test,
    #[default]
    Unknown,
}

impl TaskType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskType::BugFix => "bug_fix",
            TaskType::Feature => "feature",
            TaskType::Refactor => "refactor",
            TaskType::Investigation => "investigation",
            TaskType::Test => "test",
            TaskType::Unknown => "unknown",
        }
    }

    pub fn parse(raw: &str) -> TaskType {
        match raw {
            "bug_fix" => TaskType::BugFix,
            "feature" => TaskType::Feature,
            "refactor" => TaskType::Refactor,
            "investigation" => TaskType::Investigation,
            "test" => TaskType::Test,
            _ => TaskType::Unknown,
        }
    }
}

/// Tek bir retrieval policy. Tüm alanlar serde desteklidir; `baseline()` bugünkü
/// üretim davranışını birebir temsil eder.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetrievalPolicy {
    pub version: u32,
    pub task_type: TaskType,
    pub weights: HybridWeights,
    pub edge_weights: Vec<(EdgeType, f32)>,
    pub two_hop_decay: f32,
    pub seed_multiplier: u32,
    pub top_k: usize,
    pub token_budget_chars: usize,
    pub confidence_threshold: f32,
    pub confidence_margin: f32,
    pub min_score: f32,
    pub semantic_discount: f32,
    pub semantic_hits: usize,
    pub fallback_enabled: bool,
    pub primary_window_lines: usize,
    pub primary_window_chars: usize,
    pub related_window_lines: usize,
    pub related_window_chars: usize,
}

impl RetrievalPolicy {
    /// Bugünkü üretim davranışının birebir anlık görüntüsü.
    pub fn baseline() -> Self {
        Self {
            version: 1,
            task_type: TaskType::Unknown,
            weights: HybridWeights::default(),
            edge_weights: default_edge_weights(),
            two_hop_decay: 0.60,
            seed_multiplier: 3,
            top_k: 5,
            token_budget_chars: 12_000,
            confidence_threshold: 0.55,
            confidence_margin: 0.05,
            min_score: 0.05,
            semantic_discount: 0.7,
            semantic_hits: 2,
            fallback_enabled: true,
            primary_window_lines: 120,
            primary_window_chars: 12_000,
            related_window_lines: 80,
            related_window_chars: 8_000,
        }
    }

    /// Task tipine özel baseline: yalnızca `task_type` alanı değişir.
    pub fn baseline_for(task_type: TaskType) -> Self {
        let mut policy = Self::baseline();
        policy.task_type = task_type;
        policy
    }

    /// `CCM_HYBRID_*` ortam değişkenleri varsa ağırlıkları override eder ve
    /// debug override olarak uyarı loglar. Policy store önceliklidir; env yalnızca
    /// runtime (engine) tarafında devreye girer, evaluation env'den arındırılmıştır.
    pub fn with_env_overrides(mut self) -> Self {
        let defaults = HybridWeights::default();
        let graph = env_f32("CCM_HYBRID_GRAPH_WEIGHT", defaults.graph);
        let semantic = env_f32("CCM_HYBRID_SEM_WEIGHT", defaults.semantic);
        let spatial = env_f32("CCM_HYBRID_SPATIAL_WEIGHT", defaults.spatial);
        let recent = env_f32("CCM_HYBRID_RECENT_WEIGHT", defaults.recent);
        let any_override = graph != defaults.graph
            || semantic != defaults.semantic
            || spatial != defaults.spatial
            || recent != defaults.recent;
        if any_override {
            tracing::warn!(
                graph,
                semantic,
                spatial,
                recent,
                "CCM_HYBRID_* ortam değişkenleri policy store'unu debug override ediyor"
            );
            self.weights = normalize_hybrid_weights(HybridWeights {
                graph,
                semantic,
                spatial,
                recent,
            })
            .unwrap_or(self.weights);
        }
        if let Some(min_score) = env_f32_opt("CCM_MIN_COMBINED_SCORE") {
            self.min_score = min_score;
        }
        self
    }

    pub fn edge_weight(&self, edge: &EdgeType) -> f32 {
        self.edge_weights
            .iter()
            .find(|(kind, _)| kind == edge)
            .map(|(_, weight)| *weight)
            .unwrap_or(0.60)
    }
}

fn env_f32(name: &str, default: f32) -> f32 {
    env_f32_opt(name).unwrap_or(default)
}

fn env_f32_opt(name: &str) -> Option<f32> {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
}

fn normalize_hybrid_weights(weights: HybridWeights) -> Option<HybridWeights> {
    let sum = weights.graph + weights.semantic + weights.spatial + weights.recent;
    if sum <= f32::EPSILON {
        return None;
    }
    Some(HybridWeights {
        graph: weights.graph / sum,
        semantic: weights.semantic / sum,
        spatial: weights.spatial / sum,
        recent: weights.recent / sum,
    })
}

#[cfg(test)]
mod tests {
    use super::{RetrievalPolicy, TaskType};

    #[test]
    fn baseline_matches_production_defaults() {
        let policy = RetrievalPolicy::baseline();
        assert_eq!(policy.weights.graph, 0.55);
        assert_eq!(policy.weights.semantic, 0.35);
        assert_eq!(policy.weights.spatial, 0.08);
        assert_eq!(policy.weights.recent, 0.02);
        assert_eq!(policy.top_k, 5);
        assert_eq!(policy.semantic_hits, 2);
        assert_eq!(policy.semantic_discount, 0.7);
        assert_eq!(policy.two_hop_decay, 0.60);
    }

    #[test]
    fn task_type_parse_roundtrip() {
        assert_eq!(TaskType::parse("bug_fix"), TaskType::BugFix);
        assert_eq!(TaskType::parse("unknown"), TaskType::Unknown);
        assert_eq!(TaskType::parse("whatever"), TaskType::Unknown);
        assert_eq!(TaskType::BugFix.as_str(), "bug_fix");
    }

    #[test]
    fn baseline_for_changes_only_task_type() {
        let base = RetrievalPolicy::baseline();
        let bug = RetrievalPolicy::baseline_for(TaskType::BugFix);
        assert_eq!(bug.task_type, TaskType::BugFix);
        assert_eq!(bug.weights, base.weights);
        assert_eq!(bug.top_k, base.top_k);
    }
}
