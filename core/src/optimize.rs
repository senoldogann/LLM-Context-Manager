//! Candidate retrieval policy üretici + deterministik train-only optimizer.
//!
//! RL/fine-tuning yok: sabit seed'li grid + hill-climb. Adaylar yalnızca train
//! sette değerlendirilir; winner holdout'ta TEK kez ölçülür (tek hipotez testi).

use anyhow::Result;
use std::path::Path;
use std::sync::OnceLock;

use crate::engine::hybrid::HybridWeights;
use crate::eval::{self, EvalReport, GoldenTasksFile};
use crate::policy::gate::{quality, PromotionDecision, PromotionOptions, Split};
use crate::policy::store::{
    append_history, now_secs, PolicyHistoryEntry, PolicyMetrics, PolicyStore, PromotionResult,
};
use crate::policy::RetrievalPolicy;
use serde::{Deserialize, Serialize};

pub const OPTIMIZER_SEED: u64 = 42;
pub const MAX_CANDIDATES_DEFAULT: usize = 60;
const HILL_CLIMB_ITERATIONS: usize = 5;
const HILL_CLIMB_STARTS: usize = 3;

/// İkincil corpus adımı process-global env'i geçici değiştirdiği için paralel
/// çalışan başka bir eval çağrısının yanlış ortamda koşmasını engeller.
static EVAL_ENV_LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone)]
pub struct OptimizationOutcome {
    pub winner: RetrievalPolicy,
    pub candidate_count: usize,
    pub baseline_train: EvalReport,
    pub winner_train: EvalReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSummary {
    pub tasks_scored: usize,
    pub pass_rate: f64,
    pub mean_recall_at_k: f64,
    pub mean_precision_at_k: f64,
    pub mean_tokens: f64,
    pub mean_latency_ms: f64,
}

impl MetricsSummary {
    pub fn from_report(report: &EvalReport) -> Self {
        let scored = report.totals.scored;
        let pass_rate = crate::policy::gate::pass_rate(report);
        let recall = crate::policy::gate::mean_recall(report);
        let tokens = crate::policy::gate::mean_tokens(report);
        let precision_values: Vec<f64> = report
            .results
            .iter()
            .filter_map(|result| result.precision_at_k)
            .collect();
        let precision = if precision_values.is_empty() {
            0.0
        } else {
            precision_values.iter().sum::<f64>() / precision_values.len() as f64
        };
        let latency_values: Vec<f64> = report
            .results
            .iter()
            .filter_map(|result| result.latency_ms)
            .collect();
        let latency = if latency_values.is_empty() {
            0.0
        } else {
            latency_values.iter().sum::<f64>() / latency_values.len() as f64
        };
        Self {
            tasks_scored: scored,
            pass_rate,
            mean_recall_at_k: recall,
            mean_precision_at_k: precision,
            mean_tokens: tokens,
            mean_latency_ms: latency,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LearningReport {
    pub schema_version: u32,
    pub generated_at: u64,
    pub claim: String,
    pub seed: u64,
    pub candidate_count: usize,
    pub decision: PromotionDecision,
    pub winner_version: u32,
    pub train: TrainHoldoutPair,
    pub holdout: TrainHoldoutPair,
    #[serde(default)]
    pub secondary: Option<SecondaryComparison>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrainHoldoutPair {
    pub baseline: MetricsSummary,
    pub winner: MetricsSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecondaryComparison {
    pub corpus: String,
    pub task_count: usize,
    pub note: String,
    pub baseline: MetricsSummary,
    pub winner: MetricsSummary,
}

/// Train/holdout ayrımı + optimize + holdout gate + policy store/history + rapor.
pub async fn run_learning_pipeline(
    tasks_file: &GoldenTasksFile,
    out_dir: &Path,
    seed: u64,
    max_candidates: usize,
) -> Result<LearningReport> {
    let splits = crate::policy::gate::stratified_split(&tasks_file.tasks);
    let train_tasks = GoldenTasksFile {
        schema_version: tasks_file.schema_version,
        tasks: tasks_file
            .tasks
            .iter()
            .filter(|task| splits.get(&task.id) == Some(&Split::Train))
            .cloned()
            .collect(),
    };
    let holdout_tasks = GoldenTasksFile {
        schema_version: tasks_file.schema_version,
        tasks: tasks_file
            .tasks
            .iter()
            .filter(|task| splits.get(&task.id) == Some(&Split::Holdout))
            .cloned()
            .collect(),
    };

    let outcome = optimize(&train_tasks, seed, max_candidates).await?;
    let baseline = RetrievalPolicy::baseline();
    let baseline_holdout = eval::evaluate_policy(holdout_tasks.clone(), &baseline).await?;
    let winner_holdout = eval::evaluate_policy(holdout_tasks.clone(), &outcome.winner).await?;
    let decision = crate::policy::gate::evaluate_promotion(
        &outcome.baseline_train,
        &outcome.winner_train,
        &baseline_holdout,
        &winner_holdout,
        &PromotionOptions::default(),
    );
    let secondary = evaluate_secondary_corpus(&baseline, &outcome.winner).await?;

    let mut store = match PolicyStore::load(&PolicyStore::default_policies_path()) {
        Ok(store) => store,
        Err(_) => PolicyStore::new(baseline.clone()),
    };
    store.add_candidate(outcome.winner.clone());
    if decision.promoted {
        store.activate(outcome.winner.version);
    }
    store.save(&PolicyStore::default_policies_path())?;

    let entry = PolicyHistoryEntry {
        policy_id: outcome.winner.version,
        parent_id: Some(baseline.version),
        created_at: now_secs(),
        task_type: outcome.winner.task_type.as_str().to_string(),
        params: outcome.winner.clone(),
        train_metrics: Some(metrics_from(&outcome.winner_train)),
        holdout_metrics: Some(metrics_from(&winner_holdout)),
        promotion_result: if decision.promoted {
            PromotionResult::Promoted
        } else {
            PromotionResult::Rejected
        },
        overfit_flag: decision.overfit_warning.clone(),
        reason: decision.reason.clone(),
    };
    append_history(&PolicyStore::default_history_path(), &entry)?;

    let report = LearningReport {
        schema_version: 1,
        generated_at: now_secs(),
        claim: "proof_of_mechanism".to_string(),
        seed,
        candidate_count: outcome.candidate_count,
        decision,
        winner_version: outcome.winner.version,
        train: TrainHoldoutPair {
            baseline: MetricsSummary::from_report(&outcome.baseline_train),
            winner: MetricsSummary::from_report(&outcome.winner_train),
        },
        holdout: TrainHoldoutPair {
            baseline: MetricsSummary::from_report(&baseline_holdout),
            winner: MetricsSummary::from_report(&winner_holdout),
        },
        secondary,
    };

    std::fs::create_dir_all(out_dir)?;
    let report_path = out_dir.join("report.json");
    let content = serde_json::to_string_pretty(&report)?;
    std::fs::write(&report_path, content)?;

    Ok(report)
}

/// İkincil tablo: gerçek repo structural corpus'unda (read_graph + get_context)
/// baseline vs winner. `search_code` hariçtir çünkü gerçek corpus'ta offline
/// fixture yoktur; bu tablo iddia taşımaz, yalnızca regresyon izlemedir.
async fn evaluate_secondary_corpus(
    baseline: &RetrievalPolicy,
    winner: &RetrievalPolicy,
) -> Result<Option<SecondaryComparison>> {
    let tasks_path = std::env::var("CCM_REAL_EVAL_TASKS")
        .unwrap_or_else(|_| "eval/golden_tasks.v3.ccm.json".to_string());
    let tasks_path = Path::new(&tasks_path);
    if !tasks_path.exists() {
        return Ok(None);
    }
    let tasks_file = eval::load_tasks(tasks_path)?;
    let structural = GoldenTasksFile {
        schema_version: tasks_file.schema_version,
        tasks: tasks_file
            .tasks
            .into_iter()
            .filter(|task| task.query.kind != "search_code")
            .collect(),
    };
    if structural.tasks.is_empty() {
        return Ok(None);
    }
    let task_count = structural.tasks.len();

    // Gerçek repo structural index'i embedder gerektirmez; fixture/embedder
    // ortamı bu adım için kapatılıp geri yüklenir. Process-global env race'i
    // önlemek için tüm adım tek bir mutex altında koşar.
    let env_guard = EVAL_ENV_LOCK
        .get_or_init(|| tokio::sync::Mutex::new(()))
        .lock()
        .await;
    let prev_fixture = std::env::var("CCM_EMBEDDING_FIXTURE").ok();
    let prev_disable = std::env::var("CCM_DISABLE_EMBEDDER").ok();
    std::env::remove_var("CCM_EMBEDDING_FIXTURE");
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");

    let result = async {
        let baseline_report = eval::evaluate_policy(structural.clone(), baseline).await?;
        let winner_report = eval::evaluate_policy(structural, winner).await?;
        Ok::<_, anyhow::Error>((baseline_report, winner_report))
    }
    .await;

    restore_env("CCM_EMBEDDING_FIXTURE", prev_fixture);
    restore_env("CCM_DISABLE_EMBEDDER", prev_disable);
    drop(env_guard);
    let (baseline_report, winner_report) = result?;

    Ok(Some(SecondaryComparison {
        corpus: tasks_path.to_string_lossy().to_string(),
        task_count,
        note: "structural-only (read_graph+get_context); gerçek corpus search_code CI'da embedder gerektirir — ikincil tablo iddia taşımaz".to_string(),
        baseline: MetricsSummary::from_report(&baseline_report),
        winner: MetricsSummary::from_report(&winner_report),
    }))
}

fn restore_env(name: &str, previous: Option<String>) {
    match previous {
        Some(value) => std::env::set_var(name, value),
        None => std::env::remove_var(name),
    }
}

fn metrics_from(report: &EvalReport) -> PolicyMetrics {
    PolicyMetrics {
        tasks_scored: report.totals.scored,
        pass_rate: crate::policy::gate::pass_rate(report),
        mean_recall_at_k: crate::policy::gate::mean_recall(report),
        mean_tokens: crate::policy::gate::mean_tokens(report),
    }
}

/// Deterministik candidate uzayı (52 aday): ağırlık profilleri, semantic_hits,
/// pencere bütçeleri, top_k, tek alan perturbasyonları ve eşik varyantları.
/// Version'lar 2'den başlar (baseline=1). Plan hedefi ~60 adaydır; cap
/// `MAX_CANDIDATES_DEFAULT` hill-climb için yer bırakır.
pub fn candidate_policies(baseline: &RetrievalPolicy) -> Vec<RetrievalPolicy> {
    let mut candidates = Vec::new();
    let mut next_version = 2u32;

    let mut push = |mut policy: RetrievalPolicy, candidates: &mut Vec<RetrievalPolicy>| {
        policy.version = next_version;
        next_version += 1;
        candidates.push(policy);
    };

    // Ağırlık profilleri × semantic_hits {0,1}.
    let profiles = [
        (0.65f32, 0.25f32, 0.07f32, 0.03f32),
        (0.35f32, 0.50f32, 0.10f32, 0.05f32),
        (0.50f32, 0.50f32, 0.00f32, 0.00f32),
        (0.55f32, 0.35f32, 0.05f32, 0.05f32),
    ];
    for (graph, semantic, spatial, recent) in profiles {
        for semantic_hits in [0usize, 1] {
            let mut policy = baseline.clone();
            policy.weights = HybridWeights {
                graph,
                semantic,
                spatial,
                recent,
            };
            policy.semantic_hits = semantic_hits;
            push(policy, &mut candidates);
        }
    }

    // Baseline üzerinde semantic_hits varyantları.
    for semantic_hits in [0usize, 1] {
        let mut policy = baseline.clone();
        policy.semantic_hits = semantic_hits;
        push(policy, &mut candidates);
    }

    // Pencere bütçesi varyantları × semantic_hits (token verimliliği sinyali).
    for (primary_lines, primary_chars, related_lines, related_chars) in [
        (60usize, 6_000usize, 40usize, 4_000usize),
        (80usize, 8_000usize, 60usize, 6_000usize),
        (100usize, 10_000usize, 70usize, 7_000usize),
        (40usize, 4_000usize, 30usize, 3_000usize),
    ] {
        for semantic_hits in [0usize, 1] {
            let mut policy = baseline.clone();
            policy.primary_window_lines = primary_lines;
            policy.primary_window_chars = primary_chars;
            policy.related_window_lines = related_lines;
            policy.related_window_chars = related_chars;
            policy.semantic_hits = semantic_hits;
            push(policy, &mut candidates);
        }
    }

    // top_k × semantic_hits (search_code recall/token dengesi).
    for top_k in [4usize, 6usize, 8usize, 12usize, 16usize] {
        for semantic_hits in [0usize, 1] {
            let mut policy = baseline.clone();
            policy.top_k = top_k;
            policy.semantic_hits = semantic_hits;
            push(policy, &mut candidates);
        }
    }

    // Ağırlık profilleri × top_k {8,16} (keşif alanını genişletir).
    for (graph, semantic, spatial, recent) in profiles {
        for top_k in [8usize, 16usize] {
            let mut policy = baseline.clone();
            policy.weights = HybridWeights {
                graph,
                semantic,
                spatial,
                recent,
            };
            policy.top_k = top_k;
            policy.semantic_hits = 1;
            push(policy, &mut candidates);
        }
    }

    // Tek alan perturbasyonları (ağırlıklar + eşikler + bütçeler).
    for (field, delta) in [
        ("graph", 0.10f32),
        ("graph", -0.10f32),
        ("semantic", 0.10f32),
        ("semantic", -0.10f32),
        ("spatial", 0.05f32),
        ("recent", 0.05f32),
    ] {
        let mut policy = baseline.clone();
        match field {
            "graph" => policy.weights.graph += delta,
            "semantic" => policy.weights.semantic += delta,
            "spatial" => policy.weights.spatial += delta,
            _ => policy.weights.recent += delta,
        }
        policy.weights = normalize_weights(policy.weights);
        policy.semantic_hits = 1;
        push(policy, &mut candidates);
    }

    for two_hop_decay in [0.3f32, 0.9f32] {
        let mut policy = baseline.clone();
        policy.two_hop_decay = two_hop_decay;
        push(policy, &mut candidates);
    }

    for seed_multiplier in [2u32, 4u32] {
        let mut policy = baseline.clone();
        policy.seed_multiplier = seed_multiplier;
        push(policy, &mut candidates);
    }

    for confidence_threshold in [0.45f32, 0.65f32] {
        let mut policy = baseline.clone();
        policy.confidence_threshold = confidence_threshold;
        push(policy, &mut candidates);
    }

    for semantic_discount in [0.5f32, 0.9f32] {
        let mut policy = baseline.clone();
        policy.semantic_discount = semantic_discount;
        push(policy, &mut candidates);
    }

    for min_score in [0.0f32, 0.10f32] {
        let mut policy = baseline.clone();
        policy.min_score = min_score;
        push(policy, &mut candidates);
    }

    candidates
}

struct GridEntry {
    policy: RetrievalPolicy,
    report: EvalReport,
    quality: f64,
    tokens: f64,
    changed: usize,
}

/// Train sette en iyi candidate'ı deterministik olarak bulur: önce grid'den en
/// iyi 3 token-uygun nokta seçilir, sonra her birinden hill-climb yapılır ve
/// genel en iyi winner olur (plan: "en iyi 3 grid noktasından ±0.05 hill-climb").
pub async fn optimize(
    train_tasks: &GoldenTasksFile,
    seed: u64,
    max_candidates: usize,
) -> Result<OptimizationOutcome> {
    // Seed şu an yalnızca determinizmi garanti eder (grid sabittir); farklı seed
    // gelecekte candidate uzayını çeşitlendirirse version atamaları buradan devam eder.
    let _version_base = 2u32.wrapping_add((seed % 997) as u32);
    let baseline = RetrievalPolicy::baseline();
    let baseline_train = eval::evaluate_policy(train_tasks.clone(), &baseline).await?;
    let baseline_quality = quality(&baseline_train);
    let baseline_tokens = crate::policy::gate::mean_tokens(&baseline_train);

    let candidates = candidate_policies(&baseline);
    let mut candidate_count = 0usize;
    let mut top_starts: Vec<GridEntry> = Vec::with_capacity(HILL_CLIMB_STARTS);

    for policy in candidates {
        if candidate_count >= max_candidates {
            break;
        }
        let report = eval::evaluate_policy(train_tasks.clone(), &policy).await?;
        candidate_count += 1;
        let candidate_tokens = crate::policy::gate::mean_tokens(&report);
        // Gate ile tutarlı seçim: train'de token ARTIRAN adaylar winner olamaz
        // (token guard promotion'un zorunlu boyutudur).
        if candidate_tokens > baseline_tokens * 1.0 + f64::EPSILON {
            continue;
        }
        let candidate_quality = quality(&report);
        let changed_fields = changed_field_count(&policy, &baseline);
        let entry = GridEntry {
            policy,
            report,
            quality: candidate_quality,
            tokens: candidate_tokens,
            changed: changed_fields,
        };
        let position = top_starts
            .iter()
            .position(|existing| better_than(&entry, existing))
            .unwrap_or(top_starts.len());
        if position < HILL_CLIMB_STARTS {
            top_starts.insert(position, entry);
            top_starts.truncate(HILL_CLIMB_STARTS);
        }
    }

    // Hill-climb: top-3 başlangıç noktasından her birini iyileştir, genel en iyiyi koru.
    // Varyantlara grid'den sonra devam eden benzersiz version atanır; aksi halde
    // winner, farklı parametrelerle aynı version'ı taşıyıp policy store'da
    // (add_candidate dedup) veri bütünlüğünü bozabilir.
    let mut next_hill_version: u32 = 200;
    let mut best: Option<GridEntry> = None;
    for mut start in top_starts {
        for _ in 0..HILL_CLIMB_ITERATIONS {
            let variants = hill_climb_variants(&start.policy, next_hill_version);
            next_hill_version += variants.len() as u32;
            let mut improved = false;
            for variant in variants {
                if candidate_count >= max_candidates {
                    break;
                }
                let report = eval::evaluate_policy(train_tasks.clone(), &variant).await?;
                candidate_count += 1;
                if crate::policy::gate::mean_tokens(&report) > baseline_tokens + f64::EPSILON {
                    continue;
                }
                let variant_quality = quality(&report);
                if variant_quality > start.quality + 1e-9 {
                    start = GridEntry {
                        policy: variant.clone(),
                        report: report.clone(),
                        quality: variant_quality,
                        tokens: crate::policy::gate::mean_tokens(&report),
                        changed: changed_field_count(&variant, &baseline),
                    };
                    improved = true;
                    break;
                }
            }
            if !improved {
                break;
            }
        }
        if let Some(current_best) = &best {
            if better_than(&start, current_best) {
                best = Some(start);
            }
        } else {
            best = Some(start);
        }
    }

    let (winner, winner_train) = match best {
        Some(entry) => (entry.policy, entry.report),
        None => (baseline.clone(), baseline_train.clone()),
    };
    let _ = baseline_quality;

    Ok(OptimizationOutcome {
        winner,
        candidate_count,
        baseline_train,
        winner_train,
    })
}

/// Sıralama: kalite (desc), değişen alan sayısı (asc), token (asc), version (asc).
fn better_than(a: &GridEntry, b: &GridEntry) -> bool {
    if a.quality > b.quality + 1e-9 {
        return true;
    }
    if (a.quality - b.quality).abs() > 1e-9 {
        return false;
    }
    if a.changed != b.changed {
        return a.changed < b.changed;
    }
    if (a.tokens - b.tokens).abs() > 1e-9 {
        return a.tokens < b.tokens;
    }
    a.policy.version < b.policy.version
}

fn hill_climb_variants(policy: &RetrievalPolicy, base_version: u32) -> Vec<RetrievalPolicy> {
    let mut variants = Vec::new();

    let mut hits_less = policy.clone();
    hits_less.version = base_version;
    hits_less.semantic_hits = policy.semantic_hits.saturating_sub(1);
    variants.push(hits_less);

    let mut hits_more = policy.clone();
    hits_more.version = base_version + 1;
    hits_more.semantic_hits = policy.semantic_hits.saturating_add(1).min(2);
    variants.push(hits_more);

    let mut smaller_windows = policy.clone();
    smaller_windows.version = base_version + 2;
    smaller_windows.primary_window_chars /= 2;
    smaller_windows.related_window_chars /= 2;
    smaller_windows.primary_window_lines = (smaller_windows.primary_window_lines / 2).max(20);
    smaller_windows.related_window_lines = (smaller_windows.related_window_lines / 2).max(20);
    variants.push(smaller_windows);

    variants
}

fn normalize_weights(mut weights: HybridWeights) -> HybridWeights {
    let sum = weights.graph + weights.semantic + weights.spatial + weights.recent;
    if sum <= f32::EPSILON {
        return weights;
    }
    weights.graph /= sum;
    weights.semantic /= sum;
    weights.spatial /= sum;
    weights.recent /= sum;
    weights
}

/// Baseline'dan kaç policy alanının değiştiğini sayar (minimal-müdahale tercihi).
fn changed_field_count(policy: &RetrievalPolicy, baseline: &RetrievalPolicy) -> usize {
    let mut count = 0usize;
    if policy.weights != baseline.weights {
        count += 1;
    }
    if policy.edge_weights != baseline.edge_weights {
        count += 1;
    }
    if policy.two_hop_decay != baseline.two_hop_decay {
        count += 1;
    }
    if policy.seed_multiplier != baseline.seed_multiplier {
        count += 1;
    }
    if policy.top_k != baseline.top_k {
        count += 1;
    }
    if policy.token_budget_chars != baseline.token_budget_chars {
        count += 1;
    }
    if policy.confidence_threshold != baseline.confidence_threshold {
        count += 1;
    }
    if policy.confidence_margin != baseline.confidence_margin {
        count += 1;
    }
    if policy.min_score != baseline.min_score {
        count += 1;
    }
    if policy.semantic_discount != baseline.semantic_discount {
        count += 1;
    }
    if policy.semantic_hits != baseline.semantic_hits {
        count += 1;
    }
    if policy.fallback_enabled != baseline.fallback_enabled {
        count += 1;
    }
    if policy.primary_window_lines != baseline.primary_window_lines {
        count += 1;
    }
    if policy.primary_window_chars != baseline.primary_window_chars {
        count += 1;
    }
    if policy.related_window_lines != baseline.related_window_lines {
        count += 1;
    }
    if policy.related_window_chars != baseline.related_window_chars {
        count += 1;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::hybrid::HybridWeights;
    use crate::eval::{EvalReport, TaskResult, Totals};
    use crate::policy::RetrievalPolicy;

    fn task_result(
        id: &str,
        status: &str,
        recall: Option<f64>,
        precision: Option<f64>,
        tokens: Option<usize>,
        latency: Option<f64>,
    ) -> TaskResult {
        TaskResult {
            id: id.to_string(),
            query_type: "read_graph".to_string(),
            status: status.to_string(),
            recall_at_k: recall,
            precision_at_k: precision,
            tokens_estimated: tokens,
            latency_ms: latency,
            ..Default::default()
        }
    }

    fn report(totals: Totals, results: Vec<TaskResult>) -> EvalReport {
        EvalReport {
            schema_version: 1,
            generated_at: 0,
            totals,
            results,
        }
    }

    #[test]
    fn candidates_are_deterministic_and_versioned() {
        let baseline = RetrievalPolicy::baseline();
        let a = candidate_policies(&baseline);
        let b = candidate_policies(&baseline);
        assert_eq!(a.len(), b.len());
        assert_eq!(a.len(), 52, "candidate uzayı 52 olmalı: {}", a.len());
        let versions: Vec<u32> = a.iter().map(|policy| policy.version).collect();
        assert_eq!(versions[0], 2);
        for pair in versions.windows(2) {
            assert!(pair[1] > pair[0]);
        }
    }

    #[test]
    fn candidates_respect_normalization_and_variant_bounds() {
        let baseline = RetrievalPolicy::baseline();
        let candidates = candidate_policies(&baseline);
        let mut versions: std::collections::HashSet<u32> = std::collections::HashSet::new();
        for policy in &candidates {
            assert!(policy.semantic_hits <= 2);
            let sum = policy.weights.graph
                + policy.weights.semantic
                + policy.weights.spatial
                + policy.weights.recent;
            assert!(
                (sum - 1.0).abs() < 1e-4,
                "ağırlıklar normalize olmalı: {}",
                sum
            );
            assert!(changed_field_count(policy, &baseline) >= 1);
            assert!(
                versions.insert(policy.version),
                "version tekrarı: {}",
                policy.version
            );
        }
    }

    #[test]
    fn metrics_summary_averages_scored_results() {
        let report = report(
            Totals {
                tasks: 2,
                scored: 2,
                passed: 1,
                failed: 1,
                skipped: 0,
            },
            vec![
                task_result("a", "pass", Some(1.0), Some(1.0), Some(100), Some(10.0)),
                task_result("b", "fail", Some(0.5), Some(0.25), Some(300), Some(20.0)),
            ],
        );
        let summary = MetricsSummary::from_report(&report);
        assert_eq!(summary.tasks_scored, 2);
        assert!((summary.pass_rate - 50.0).abs() < 1e-9);
        assert!((summary.mean_recall_at_k - 0.75).abs() < 1e-9);
        assert!((summary.mean_precision_at_k - 0.625).abs() < 1e-9);
        assert!((summary.mean_tokens - 200.0).abs() < 1e-9);
        assert!((summary.mean_latency_ms - 15.0).abs() < 1e-9);
    }

    #[test]
    fn metrics_summary_is_zero_when_no_results() {
        let report = report(Totals::default(), Vec::new());
        let summary = MetricsSummary::from_report(&report);
        assert_eq!(summary.tasks_scored, 0);
        assert!((summary.pass_rate - 0.0).abs() < 1e-9);
        assert!((summary.mean_recall_at_k - 0.0).abs() < 1e-9);
        assert!((summary.mean_precision_at_k - 0.0).abs() < 1e-9);
        assert!((summary.mean_tokens - 0.0).abs() < 1e-9);
        assert!((summary.mean_latency_ms - 0.0).abs() < 1e-9);
    }

    #[test]
    fn hill_climb_variants_halve_windows_and_assign_versions() {
        let mut policy = RetrievalPolicy::baseline();
        policy.semantic_hits = 1;
        policy.primary_window_chars = 800;
        policy.related_window_chars = 600;
        policy.primary_window_lines = 30;
        policy.related_window_lines = 20;
        let variants = hill_climb_variants(&policy, 200);
        assert_eq!(variants.len(), 3);
        assert_eq!(variants[0].version, 200);
        assert_eq!(variants[1].version, 201);
        assert_eq!(variants[2].version, 202);
        assert_eq!(variants[2].primary_window_chars, 400);
        assert_eq!(variants[2].related_window_chars, 300);
        assert_eq!(variants[2].primary_window_lines, 20);
        assert_eq!(variants[2].related_window_lines, 20);
    }

    #[test]
    fn normalize_weights_scales_to_unit_sum_and_keeps_zero_sum() {
        let weights = normalize_weights(HybridWeights {
            graph: 2.0,
            semantic: 2.0,
            spatial: 0.0,
            recent: 0.0,
        });
        assert!((weights.graph - 0.5).abs() < 1e-6);
        assert!((weights.semantic - 0.5).abs() < 1e-6);
        assert!((weights.spatial - 0.0).abs() < 1e-6);

        let zero = normalize_weights(HybridWeights {
            graph: 0.0,
            semantic: 0.0,
            spatial: 0.0,
            recent: 0.0,
        });
        assert_eq!(
            zero,
            HybridWeights {
                graph: 0.0,
                semantic: 0.0,
                spatial: 0.0,
                recent: 0.0,
            }
        );
    }

    #[test]
    fn changed_field_count_counts_only_different_fields() {
        let baseline = RetrievalPolicy::baseline();
        assert_eq!(changed_field_count(&baseline, &baseline), 0);

        let mut one = baseline.clone();
        one.semantic_hits = 1;
        assert_eq!(changed_field_count(&one, &baseline), 1);

        let mut two = baseline.clone();
        two.semantic_hits = 1;
        two.top_k = 8;
        assert_eq!(changed_field_count(&two, &baseline), 2);

        let mut three = baseline.clone();
        three.weights = HybridWeights {
            graph: 0.5,
            semantic: 0.5,
            spatial: 0.0,
            recent: 0.0,
        };
        three.fallback_enabled = false;
        three.primary_window_lines = 90;
        assert_eq!(changed_field_count(&three, &baseline), 3);
    }

    #[test]
    fn better_than_orders_by_quality_changed_tokens_and_version() {
        let policy_a = RetrievalPolicy::baseline();
        let entry = |quality: f64, changed: usize, tokens: f64, version: u32| GridEntry {
            policy: RetrievalPolicy {
                version,
                ..policy_a.clone()
            },
            report: report(Totals::default(), Vec::new()),
            quality,
            tokens,
            changed,
        };
        let high_quality = entry(90.0, 2, 100.0, 1);
        let low_quality = entry(80.0, 2, 50.0, 2);
        assert!(better_than(&high_quality, &low_quality));
        assert!(!better_than(&low_quality, &high_quality));

        let fewer_changes = entry(80.0, 1, 100.0, 3);
        assert!(better_than(&fewer_changes, &low_quality));

        let fewer_tokens = entry(80.0, 1, 20.0, 4);
        assert!(better_than(&fewer_tokens, &fewer_changes));

        let same_metrics_older = entry(80.0, 1, 20.0, 2);
        assert!(better_than(&same_metrics_older, &fewer_tokens));
    }

    #[test]
    fn metrics_from_maps_report_metrics() {
        let report = report(
            Totals {
                tasks: 1,
                scored: 1,
                passed: 1,
                failed: 0,
                skipped: 0,
            },
            vec![task_result(
                "a",
                "pass",
                Some(1.0),
                Some(1.0),
                Some(50),
                None,
            )],
        );
        let metrics = metrics_from(&report);
        assert_eq!(metrics.tasks_scored, 1);
        assert!((metrics.pass_rate - 100.0).abs() < 1e-9);
        assert!((metrics.mean_recall_at_k - 1.0).abs() < 1e-9);
        assert!((metrics.mean_tokens - 50.0).abs() < 1e-9);
    }

    #[test]
    fn restore_env_restores_previous_value_and_removes_unset() {
        let name = "CCM_TEST_RESTORE_ENV_OPTIMIZE";
        std::env::set_var(name, "current");
        restore_env(name, Some("previous".to_string()));
        assert_eq!(
            std::env::var(name).expect("env değeri"),
            "previous".to_string()
        );

        std::env::set_var(name, "current");
        restore_env(name, None);
        assert!(std::env::var(name).is_err());
    }
}
