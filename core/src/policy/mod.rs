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

/// H1 graf skor yayılımında komşu desteğinin payı (λ); geliştirme setinde
/// önceden yazılmış kuralla seçilen değer. Eski policy dosyalarında alan
/// yoksa bu değer yüklenir.
pub const DEFAULT_PROPAGATION_WEIGHT: f32 = 0.1;

/// `search_code_hybrid` sonuçlarının sıralama yöntemi.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RankingMethod {
    /// Yalnızca semantik benzerlik (S): vektör aramasının düğüm tohumları
    /// semantik skora göre sıralanır, graf sıralamaya katılmaz.
    Semantic,
    /// Hibrit sıralama (H0): graf, semantik ve yol skorlarının doğrusal
    /// birleşimi ve güven fallback'i. Alanı olmayan eski policy dosyaları bu
    /// yöntemle oluşturulduğundan serde varsayılanıdır.
    #[default]
    Hybrid,
    /// Graf skor yayılımı (H1): tohumların semantik skoru tek adımlık
    /// komşuluk kenarları boyunca yayılır ve sıralamayı değiştirir.
    Propagation,
}

impl RankingMethod {
    pub fn as_str(&self) -> &'static str {
        match self {
            RankingMethod::Semantic => "semantic",
            RankingMethod::Hybrid => "hybrid",
            RankingMethod::Propagation => "propagation",
        }
    }

    /// Yöntem adını çözer; bilinmeyen ad açık bir hatadır.
    pub fn parse(raw: &str) -> anyhow::Result<RankingMethod> {
        match raw {
            "semantic" => Ok(RankingMethod::Semantic),
            "hybrid" => Ok(RankingMethod::Hybrid),
            "propagation" => Ok(RankingMethod::Propagation),
            other => Err(anyhow::anyhow!(
                "Unknown ranking method '{}'; expected semantic, hybrid or propagation",
                other
            )),
        }
    }
}

fn default_propagation_weight() -> f32 {
    DEFAULT_PROPAGATION_WEIGHT
}

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
    /// Sıralama yöntemi; alanı olmayan eski policy dosyalarında hibrit (H0).
    #[serde(default)]
    pub ranking: RankingMethod,
    /// H1'de komşu desteğinin payı λ ∈ [0, 1]: skor = (1 − λ)·semantik + λ·destek.
    #[serde(default = "default_propagation_weight")]
    pub propagation_weight: f32,
}

impl RetrievalPolicy {
    /// Bugünkü üretim davranışının birebir anlık görüntüsü. Sıralama yöntemi,
    /// SWE-bench Lite test setinde dosya düzeyinde en yüksek MRR@10'u veren
    /// yöntemdir.
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
            ranking: RankingMethod::Propagation,
            propagation_weight: DEFAULT_PROPAGATION_WEIGHT,
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
    use super::{RankingMethod, RetrievalPolicy, TaskType, DEFAULT_PROPAGATION_WEIGHT};

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
        assert_eq!(policy.ranking, RankingMethod::Propagation);
        assert_eq!(policy.propagation_weight, 0.1);
    }

    #[test]
    fn ranking_method_parse_roundtrip() {
        for method in [
            RankingMethod::Semantic,
            RankingMethod::Hybrid,
            RankingMethod::Propagation,
        ] {
            assert_eq!(
                RankingMethod::parse(method.as_str()).expect("known method"),
                method
            );
        }
        assert!(RankingMethod::parse("bm25").is_err());
    }

    #[test]
    fn policy_without_ranking_fields_loads_hybrid() {
        let mut value = serde_json::to_value(RetrievalPolicy::baseline()).expect("serialize");
        let object = value.as_object_mut().expect("policy object");
        object.remove("ranking");
        object.remove("propagation_weight");
        let loaded: RetrievalPolicy = serde_json::from_value(value).expect("deserialize");
        assert_eq!(loaded.ranking, RankingMethod::Hybrid);
        assert_eq!(loaded.propagation_weight, DEFAULT_PROPAGATION_WEIGHT);
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
