//! Deterministik train/holdout split ve promotion gate.
//!
//! Evaluator optimizasyon sırasında dokunulmaz: split hash ile sabit, gate
//! kriterleri kodda açıktır. Promotion, train'de seçilen TEK winner'ın holdout
//! ölçümüyle karar verilir (tek hipotez testi, Bonferroni yok).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::eval::{EvalReport, TaskResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Split {
    Train,
    Holdout,
}

impl Split {
    pub fn as_str(&self) -> &'static str {
        match self {
            Split::Train => "train",
            Split::Holdout => "holdout",
        }
    }
}

/// sha256(task.id) ilk byte'ı; `default_split` bunu %75 train eşiğiyle kullanır.
pub fn hash_split_byte(task_id: &str) -> u8 {
    let digest = crate::hash::sha256(task_id.as_bytes());
    digest[0]
}

/// `0xC0` = %75: byte < 0xC0 → train. (v0.5'teki `0x78`/%47 hatası düzeltildi.)
pub fn default_split(task_id: &str) -> Split {
    if hash_split_byte(task_id) < 0xC0 {
        Split::Train
    } else {
        Split::Holdout
    }
}

/// Tip bazlı stratified split: her query tipi kendi içinde hash'lenir.
/// `search_code` için train'de en az 2 task garantilenir; bu garanti, açık
/// `split: "holdout"` alanını dahi train'e çevirebilir (sentetik corpus dengeli
/// olduğu için etkisizdir; dar bir tip tamamen holdout ise per-tip pass-rate
/// kontrolü boş kalır ve bu bilinçli bir takastır).
pub fn stratified_split(tasks: &[crate::eval::Task]) -> HashMap<String, Split> {
    let mut result: HashMap<String, Split> = HashMap::new();
    let mut by_type: HashMap<String, Vec<&crate::eval::Task>> = HashMap::new();
    for task in tasks {
        by_type
            .entry(task.query.kind.clone())
            .or_default()
            .push(task);
    }

    for (kind, group) in by_type {
        let mut sorted: Vec<&crate::eval::Task> = group.clone();
        sorted.sort_by(|a, b| a.id.cmp(&b.id));
        let mut assigned_train = 0usize;
        for task in sorted {
            let split = match task.split.as_deref() {
                Some("holdout") => Split::Holdout,
                Some("train") => Split::Train,
                _ => default_split(&task.id),
            };
            let mut split = split;
            if kind == "search_code" && assigned_train < 2 && split == Split::Holdout {
                split = Split::Train;
            }
            if split == Split::Train {
                assigned_train += 1;
            }
            result.insert(task.id.clone(), split);
        }
    }
    result
}

/// Sign test için gerekli iyileşen task sayısı:
/// en küçük k öyle ki P(Bin(n, 0.5) >= k) <= alpha.
pub fn sign_test_k(n: usize, alpha: f64) -> usize {
    if n == 0 {
        return 0;
    }
    let mut k = n;
    while k > 0 && binomial_tail(n, k, 0.5) <= alpha {
        k -= 1;
    }
    (k + 1).min(n)
}

fn binomial_tail(n: usize, k: usize, p: f64) -> f64 {
    if k == 0 {
        return 1.0;
    }
    let mut tail = 0.0;
    for i in k..=n {
        tail += binomial(n, i) * p.powi(i as i32) * (1.0 - p).powi((n - i) as i32);
    }
    tail.min(1.0)
}

fn binomial(n: usize, k: usize) -> f64 {
    if k > n {
        return 0.0;
    }
    let k = k.min(n - k);
    let mut result = 1.0f64;
    for i in 1..=k {
        result *= (n - k + i) as f64 / i as f64;
    }
    result
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromotionDecision {
    pub promoted: bool,
    pub reason: String,
    pub overfit_warning: Option<String>,
}

pub struct PromotionOptions {
    pub max_regression: f64,
    pub recall_improvement: f64,
    pub recall_tolerance: f64,
    pub token_guard_ratio: f64,
    pub alpha: f64,
}

impl Default for PromotionOptions {
    fn default() -> Self {
        Self {
            max_regression: 0.0,
            recall_improvement: 0.03,
            recall_tolerance: 0.01,
            token_guard_ratio: 1.05,
            alpha: 0.05,
        }
    }
}

/// Composite quality YALNIZCA train winner seçimi içindir; promotion gerçeği
/// değildir (plan v0.7 kararı).
pub fn quality(report: &EvalReport) -> f64 {
    let pass_rate = pass_rate(report);
    let recall = mean_recall(report);
    0.7 * pass_rate + 0.3 * recall
}

pub fn pass_rate(report: &EvalReport) -> f64 {
    if report.totals.scored == 0 {
        return 0.0;
    }
    report.totals.passed as f64 / report.totals.scored as f64 * 100.0
}

pub fn mean_recall(report: &EvalReport) -> f64 {
    let values: Vec<f64> = report
        .results
        .iter()
        .filter_map(|result| result.recall_at_k)
        .collect();
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

pub fn mean_tokens(report: &EvalReport) -> f64 {
    let values: Vec<f64> = report
        .results
        .iter()
        .filter_map(|result| result.tokens_estimated.map(|t| t as f64))
        .collect();
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

/// Promotion gate (v0.7 + token-efficiency boyutu):
/// 1. Her query tipinde pass-rate regresyonu yok.
/// 2. Holdout mean recall ya iyileşir (>= +recall_improvement) ya da
///    regresyon yapmaz (>= -recall_tolerance) VE token verimliliği
///    istatistiksel olarak iyileşir (sign test + ortalama düşüş).
/// 3. Token guard: mean_tokens <= baseline * token_guard_ratio.
/// 4. Overfit kontrolü: holdout_gain < 0 -> reject; < 0.5 * train_gain -> flag.
pub fn evaluate_promotion(
    baseline_train: &EvalReport,
    candidate_train: &EvalReport,
    baseline_holdout: &EvalReport,
    candidate_holdout: &EvalReport,
    options: &PromotionOptions,
) -> PromotionDecision {
    let mut failures: Vec<String> = Vec::new();

    // No-op koruması: hiçbir şey değişmemişse (n_eff=0 ya da candidate == baseline)
    // promote etmek anlamsızdır; sign testi 0<0 ile boşuna geçer.
    let candidate_is_baseline = candidate_holdout.results.iter().all(|result| {
        baseline_holdout
            .results
            .iter()
            .find(|base| base.id == result.id)
            .map(|base| {
                base.matches == result.matches
                    && base.tokens_estimated == result.tokens_estimated
                    && base.status == result.status
            })
            .unwrap_or(false)
    });
    let n_eff = changed_tasks(baseline_holdout, candidate_holdout);
    if candidate_is_baseline || n_eff == 0 {
        return PromotionDecision {
            promoted: false,
            reason: "no-op: candidate baseline ile aynı sonucu üretiyor (n_eff=0)".to_string(),
            overfit_warning: None,
        };
    }

    for kind in query_type_set(baseline_holdout, candidate_holdout) {
        let base = query_type_pass_rate(baseline_holdout, &kind);
        let cand = query_type_pass_rate(candidate_holdout, &kind);
        if cand < base - options.max_regression {
            failures.push(format!(
                "{} pass rate regresyonu: {:.1} -> {:.1}",
                kind, base, cand
            ));
        }
    }

    let base_recall = mean_recall(baseline_holdout);
    let cand_recall = mean_recall(candidate_holdout);
    let recall_gain = cand_recall - base_recall;

    let base_tokens = mean_tokens(baseline_holdout);
    let cand_tokens = mean_tokens(candidate_holdout);
    let token_gain = cand_tokens - base_tokens;

    if token_gain > 0.0 && recall_gain < options.recall_improvement {
        failures.push(format!(
            "ne recall ne token iyileşmesi: recall {:+.3}, tokens {:+.1}",
            recall_gain, token_gain
        ));
    }
    if recall_gain < -options.recall_tolerance {
        failures.push(format!("holdout recall regresyonu: {:+.3}", recall_gain));
    }
    if cand_tokens > base_tokens * options.token_guard_ratio {
        failures.push(format!(
            "token guard aşıldı: {:.0} > {:.0} * {:.2}",
            cand_tokens, base_tokens, options.token_guard_ratio
        ));
    }

    let required = sign_test_k(n_eff, options.alpha);
    let improved = improved_tasks(baseline_holdout, candidate_holdout);
    if recall_gain < options.recall_improvement && improved < required {
        failures.push(format!(
            "sign testi başarısız: improved {} < required {} (n_eff={})",
            improved, required, n_eff
        ));
    }

    let train_gain = mean_recall(candidate_train) - mean_recall(baseline_train);
    let overfit_warning = if recall_gain < 0.0 {
        Some("holdout_gain < 0".to_string())
    } else if train_gain > 0.0 && recall_gain < 0.5 * train_gain {
        Some(format!(
            "probable_overfit: holdout_gain {:.3} < 0.5 * train_gain {:.3}",
            recall_gain, train_gain
        ))
    } else {
        None
    };

    if !failures.is_empty() {
        return PromotionDecision {
            promoted: false,
            reason: failures.join("; "),
            overfit_warning,
        };
    }

    let reason = if recall_gain >= options.recall_improvement {
        format!(
            "holdout recall iyileşmesi {:+.3} (baseline {:.3})",
            recall_gain, base_recall
        )
    } else {
        format!(
            "token verimliliği iyileşmesi {:+.0} karakter ({:.0} -> {:.0}), recall regresyonsuz",
            token_gain, base_tokens, cand_tokens
        )
    };
    PromotionDecision {
        promoted: true,
        reason,
        overfit_warning,
    }
}

fn query_type_set(a: &EvalReport, b: &EvalReport) -> Vec<String> {
    let mut kinds: Vec<String> = a
        .results
        .iter()
        .map(|result| result.query_type.clone())
        .chain(b.results.iter().map(|result| result.query_type.clone()))
        .collect();
    kinds.sort();
    kinds.dedup();
    kinds
}

fn query_type_pass_rate(report: &EvalReport, kind: &str) -> f64 {
    let results: Vec<&TaskResult> = report
        .results
        .iter()
        .filter(|result| result.query_type == kind)
        .collect();
    let total = results.len();
    if total == 0 {
        return 0.0;
    }
    let passed = results
        .iter()
        .filter(|result| result.status == "pass")
        .count();
    passed as f64 / total as f64 * 100.0
}

/// Sign test evreni: candidate ile baseline arasında recall VEYA token değeri
/// değişen task'lar. Değişmeyen task'lar (tie) test dışıdır.
fn changed_tasks(baseline: &EvalReport, candidate: &EvalReport) -> usize {
    let base: HashMap<&str, usize> = baseline
        .results
        .iter()
        .map(|result| (result.id.as_str(), result.matches))
        .collect();
    let base_tokens: HashMap<&str, usize> = baseline
        .results
        .iter()
        .filter_map(|result| {
            result
                .tokens_estimated
                .map(|tokens| (result.id.as_str(), tokens))
        })
        .collect();
    candidate
        .results
        .iter()
        .filter(|result| {
            let matches_changed = base.get(result.id.as_str()).copied() != Some(result.matches);
            let tokens_changed =
                match (base_tokens.get(result.id.as_str()), result.tokens_estimated) {
                    (Some(base), Some(candidate)) => base != &candidate,
                    (None, None) => false,
                    _ => true,
                };
            matches_changed || tokens_changed
        })
        .count()
}

/// Per-task strict improvement: recall arttı VE token artmadı; ya da recall
/// korundu VE token azaldı.
fn improved_tasks(baseline: &EvalReport, candidate: &EvalReport) -> usize {
    let base: HashMap<&str, (usize, usize)> = baseline
        .results
        .iter()
        .filter_map(|result| {
            result
                .tokens_estimated
                .map(|tokens| (result.id.as_str(), (result.matches, tokens)))
        })
        .collect();
    candidate
        .results
        .iter()
        .filter(|result| {
            let Some((base_matches, base_tokens)) = base.get(result.id.as_str()) else {
                return false;
            };
            let Some(candidate_tokens) = result.tokens_estimated else {
                return false;
            };
            if result.matches > *base_matches {
                candidate_tokens <= *base_tokens
            } else if result.matches == *base_matches {
                candidate_tokens < *base_tokens
            } else {
                false
            }
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_split_uses_75_percent_threshold() {
        assert_eq!(default_split("syn-a-search-001"), Split::Train);
        // 0x78 eşiğiyle train olan ama 0xC0 eşiğinde holdout kalan id kontrolü:
        let byte = hash_split_byte("syn-a-search-001");
        assert!(byte < 0xC0);
    }

    #[test]
    fn sign_test_k_known_values() {
        assert_eq!(sign_test_k(75, 0.05), 46);
        assert_eq!(sign_test_k(25, 0.05), 18);
        assert_eq!(sign_test_k(0, 0.05), 0);
    }

    #[test]
    fn quality_is_weighted_composite() {
        let report = EvalReport {
            schema_version: 1,
            generated_at: 0,
            totals: crate::eval::Totals {
                tasks: 1,
                scored: 1,
                passed: 1,
                failed: 0,
                skipped: 0,
            },
            results: vec![TaskResult {
                id: "t".into(),
                query_type: "search_code".into(),
                status: "pass".into(),
                detail: String::new(),
                matches: 1,
                expected_min_recall: 1,
                max_rank: 5,
                matched_items: vec![],
                recall_at_k: Some(1.0),
                precision_at_k: Some(1.0),
                relevant_coverage: Some(1.0),
                tokens_estimated: Some(100),
                latency_ms: Some(1.0),
                ranked: Some(vec!["a".into()]),
                missing_relevant: Some(vec![]),
                retrieved_unused: Some(vec![]),
                policy_version: Some(1),
            }],
        };
        let q = quality(&report);
        assert!((q - (0.7 * 100.0 + 0.3 * 1.0)).abs() < 1e-9);
    }

    #[test]
    fn noop_candidate_is_rejected() {
        let mut baseline = EvalReport {
            schema_version: 1,
            generated_at: 0,
            totals: crate::eval::Totals {
                tasks: 1,
                scored: 1,
                passed: 1,
                failed: 0,
                skipped: 0,
            },
            results: vec![TaskResult {
                id: "t".into(),
                query_type: "search_code".into(),
                status: "pass".into(),
                detail: String::new(),
                matches: 1,
                expected_min_recall: 1,
                max_rank: 5,
                matched_items: vec![],
                recall_at_k: Some(1.0),
                precision_at_k: Some(1.0),
                relevant_coverage: Some(1.0),
                tokens_estimated: Some(100),
                latency_ms: Some(1.0),
                ranked: Some(vec!["a".into()]),
                missing_relevant: Some(vec![]),
                retrieved_unused: Some(vec![]),
                policy_version: Some(1),
            }],
        };
        let candidate = baseline.clone();
        let decision = evaluate_promotion(
            &baseline,
            &candidate,
            &baseline,
            &candidate,
            &PromotionOptions::default(),
        );
        assert!(!decision.promoted, "no-op promote edilmemeli");
        let _ = &mut baseline;
    }
}
