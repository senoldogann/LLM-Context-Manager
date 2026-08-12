use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::engine::hybrid::HybridScorer;
use crate::engine::{CursorPosition, RetrievalEngine};
use crate::graph::CodeGraph;
use crate::policy::{RetrievalPolicy, TaskType};
use crate::vector::store::LanceDbStore;
use petgraph::visit::EdgeRef;
use petgraph::Direction;
use std::sync::Arc;

#[derive(Debug, Clone, Deserialize)]
pub struct GoldenTasksFile {
    pub schema_version: u32,
    pub tasks: Vec<Task>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Task {
    pub id: String,
    pub repo: RepoRef,
    pub query: Query,
    pub expected: Expected,
    pub tags: Option<Vec<String>>,
    pub priority: Option<u8>,
    pub notes: Option<String>,
    #[serde(default)]
    pub task_type: Option<TaskType>,
    #[serde(default)]
    pub split: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RepoRef {
    pub name: String,
    pub path: String,
    pub commit: Option<String>,
    pub languages: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Query {
    #[serde(rename = "type")]
    pub kind: String,
    pub text: Option<String>,
    pub node_id: Option<String>,
    pub file_path: Option<String>,
    pub line: Option<u32>,
    pub column: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Expected {
    pub node_ids: Option<Vec<String>>,
    pub file_paths: Option<Vec<String>>,
    pub min_recall: u32,
    pub max_rank: Option<u32>,
    pub reason_contains: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalReport {
    pub schema_version: u32,
    pub generated_at: u64,
    pub totals: Totals,
    pub results: Vec<TaskResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Totals {
    pub tasks: usize,
    pub scored: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TaskResult {
    pub id: String,
    pub query_type: String,
    pub status: String,
    pub detail: String,
    pub matches: usize,
    pub expected_min_recall: u32,
    pub max_rank: u32,
    pub matched_items: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recall_at_k: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub precision_at_k: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relevant_coverage: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_estimated: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ranked: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missing_relevant: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrieved_unused: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_version: Option<u32>,
}

#[derive(Debug, Serialize)]
pub struct ComparisonReport {
    pub schema_version: u32,
    pub generated_at: u64,
    pub structural: EvalReport,
    pub hybrid: EvalReport,
    pub comparison: ComparisonSummary,
}

#[derive(Debug, Serialize)]
pub struct ComparisonSummary {
    pub structural_pass_rate: f64,
    pub hybrid_pass_rate: f64,
    pub improvement: f64,
    pub by_query_type: BTreeMap<String, QueryTypeComparison>,
}

#[derive(Debug, Serialize)]
pub struct QueryTypeComparison {
    pub structural_pass_rate: f64,
    pub hybrid_pass_rate: f64,
    pub improvement: f64,
}

#[derive(Debug, Clone, Copy)]
pub enum EvalMode {
    Structural,
    Hybrid,
}

pub fn write_report<W: Write>(writer: W, report: &EvalReport) -> Result<()> {
    serde_json::to_writer_pretty(writer, report)?;
    Ok(())
}

pub fn write_comparison_report<W: Write>(writer: W, report: &ComparisonReport) -> Result<()> {
    serde_json::to_writer_pretty(writer, report)?;
    Ok(())
}

pub fn report_pass_rate(report: &EvalReport) -> f64 {
    let total = report.totals.scored;
    if total == 0 {
        return 0.0;
    }
    (report.totals.passed as f64 / total as f64) * 100.0
}

pub fn enforce_quality_gate(
    report: &EvalReport,
    minimum_pass_rate: f64,
    baseline: Option<&EvalReport>,
    maximum_regression: f64,
) -> Result<()> {
    if report.totals.scored == 0 {
        anyhow::bail!("evaluation quality gate failed: no tasks were scored");
    }
    if report.totals.scored != report.totals.tasks || report.totals.skipped > 0 {
        anyhow::bail!(
            "evaluation quality gate failed: {} of {} tasks were scored ({} skipped)",
            report.totals.scored,
            report.totals.tasks,
            report.totals.skipped
        );
    }

    let current = report_pass_rate(report);
    if current < minimum_pass_rate {
        anyhow::bail!(
            "evaluation quality gate failed: pass rate {:.1}% is below {:.1}%",
            current,
            minimum_pass_rate
        );
    }

    if let Some(baseline) = baseline {
        let baseline_rate = report_pass_rate(baseline);
        let regression = baseline_rate - current;
        if regression > maximum_regression {
            anyhow::bail!(
                "evaluation quality gate failed: {:.1}% regression exceeds {:.1}% (baseline {:.1}%, current {:.1}%)",
                regression,
                maximum_regression,
                baseline_rate,
                current
            );
        }
    }

    Ok(())
}

pub fn load_report(path: &Path) -> Result<EvalReport> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open evaluation report: {}", path.display()))?;
    serde_json::from_reader(std::io::BufReader::new(file))
        .with_context(|| format!("Failed to parse evaluation report: {}", path.display()))
}

pub fn summarize_report(report: &EvalReport, report_path: Option<&str>) -> String {
    let mut output = Vec::new();
    output.push("==================================================".to_string());
    output.push("CCM Evaluation Report".to_string());
    output.push("==================================================".to_string());

    let total = report.totals.tasks as f64;
    let pass_rate = if total > 0.0 {
        (report.totals.passed as f64 / total) * 100.0
    } else {
        0.0
    };

    output.push(format!(
        "Tasks: {} | Passed: {} | Failed: {} | Skipped: {}",
        report.totals.tasks, report.totals.passed, report.totals.failed, report.totals.skipped
    ));
    output.push(format!("Pass Rate: {:.1}%", pass_rate));

    let mut by_type: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    for result in &report.results {
        let entry = by_type
            .entry(result.query_type.clone())
            .or_insert((0, 0, 0));
        entry.0 += 1;
        match result.status.as_str() {
            "pass" => entry.1 += 1,
            "fail" => entry.2 += 1,
            _ => {}
        }
    }

    if !by_type.is_empty() {
        output.push("Query Types:".to_string());
        for (kind, (total, passed, failed)) in by_type {
            output.push(format!(
                "  {}: {}/{} passed{}",
                kind,
                passed,
                total,
                if failed > 0 {
                    format!(" ({} failed)", failed)
                } else {
                    String::new()
                }
            ));
        }
    }

    let failed: Vec<&TaskResult> = report
        .results
        .iter()
        .filter(|r| r.status == "fail")
        .collect();
    if failed.is_empty() {
        output.push("Failed Tasks: none".to_string());
    } else {
        output.push("Failed Tasks:".to_string());
        for result in failed.iter().take(10) {
            output.push(format!("  {}: {}", result.id, result.detail));
        }
        if failed.len() > 10 {
            output.push(format!("  ... and {} more", failed.len() - 10));
        }
    }

    if let Some(path) = report_path {
        output.push(format!("Full report: {}", path));
    }

    output.push("==================================================".to_string());
    output.join("\n")
}

pub fn summarize_comparison_report(report: &ComparisonReport, report_path: Option<&str>) -> String {
    let mut output = Vec::new();
    output.push("==================================================".to_string());
    output.push("CCM Evaluation Comparison".to_string());
    output.push("==================================================".to_string());

    output.push(format!(
        "Structural Pass Rate: {:.1}%",
        report.comparison.structural_pass_rate
    ));
    output.push(format!(
        "Hybrid Pass Rate: {:.1}%",
        report.comparison.hybrid_pass_rate
    ));
    output.push(format!(
        "Improvement: {:+.1}%",
        report.comparison.improvement
    ));

    if !report.comparison.by_query_type.is_empty() {
        output.push("By Query Type:".to_string());
        for (kind, summary) in &report.comparison.by_query_type {
            output.push(format!(
                "  {}: {:.1}% -> {:.1}% ({:+.1}%)",
                kind, summary.structural_pass_rate, summary.hybrid_pass_rate, summary.improvement
            ));
        }
    }

    if let Some(path) = report_path {
        output.push(format!("Full report: {}", path));
    }

    output.push("==================================================".to_string());
    output.join("\n")
}

pub fn load_tasks(path: &Path) -> Result<GoldenTasksFile> {
    let file = std::fs::File::open(path)
        .with_context(|| format!("Failed to open tasks file: {}", path.display()))?;
    let reader = std::io::BufReader::new(file);
    let tasks: GoldenTasksFile = serde_json::from_reader(reader)
        .with_context(|| format!("Failed to parse tasks JSON: {}", path.display()))?;
    Ok(tasks)
}

pub async fn evaluate_from_path(path: &Path) -> Result<EvalReport> {
    let tasks_file = load_tasks(path)?;
    evaluate_with_mode(tasks_file, EvalMode::Structural).await
}

pub async fn evaluate(tasks_file: GoldenTasksFile) -> Result<EvalReport> {
    evaluate_with_mode(tasks_file, EvalMode::Structural).await
}

pub async fn evaluate_comparison_from_path(path: &Path) -> Result<ComparisonReport> {
    let structural_tasks = load_tasks(path)?;
    let hybrid_tasks = load_tasks(path)?;

    let structural = evaluate_with_mode(structural_tasks, EvalMode::Structural).await?;
    let hybrid = evaluate_with_mode(hybrid_tasks, EvalMode::Hybrid).await?;
    let comparison = build_comparison_summary(&structural, &hybrid);

    Ok(ComparisonReport {
        schema_version: structural.schema_version,
        generated_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        structural,
        hybrid,
        comparison,
    })
}

pub async fn evaluate_with_mode(tasks_file: GoldenTasksFile, mode: EvalMode) -> Result<EvalReport> {
    let mut totals = Totals::default();
    let mut results = Vec::new();
    let mut prepared_repos: HashSet<PathBuf> = HashSet::new();

    totals.tasks = tasks_file.tasks.len();

    for task in tasks_file.tasks {
        let repo_path = normalize_repo_path(&task.repo.path)?;
        let db_path = repo_path.join("data").join("ccm_db");
        let graph_path = repo_path.join("data").join("ccm_graph.json");

        let max_rank = task.expected.max_rank.unwrap_or(5).max(1);
        let expected_min_recall = task.expected.min_recall;

        let mut result = TaskResult {
            id: task.id.clone(),
            query_type: task.query.kind.clone(),
            status: "skipped".to_string(),
            detail: String::new(),
            matches: 0,
            expected_min_recall,
            max_rank,
            matched_items: Vec::new(),
            recall_at_k: None,
            precision_at_k: None,
            relevant_coverage: None,
            tokens_estimated: None,
            latency_ms: None,
            ranked: None,
            missing_relevant: None,
            retrieved_unused: None,
            policy_version: None,
        };

        if let Err(error) =
            ensure_eval_index(&repo_path, &db_path, &graph_path, &mut prepared_repos).await
        {
            result.detail = format!("Failed to prepare index: {}", error);
            totals.skipped += 1;
            results.push(result);
            continue;
        }

        match task.query.kind.as_str() {
            "search_code" => {
                let Some(query_text) = task.query.text.clone() else {
                    result.status = "fail".to_string();
                    result.detail = "Missing query.text".to_string();
                    totals.failed += 1;
                    results.push(result);
                    continue;
                };

                let search_result = if matches!(mode, EvalMode::Hybrid) {
                    if graph_path.exists() {
                        search_code_hybrid(&db_path, &graph_path, &query_text, max_rank as usize)
                            .await
                    } else {
                        search_code(&db_path, &query_text, max_rank as usize).await
                    }
                } else {
                    search_code(&db_path, &query_text, max_rank as usize).await
                };

                match search_result {
                    Ok(hits) => {
                        let (matches, matched_items) = score_hits(&task.expected, &hits);
                        result.matches = matches;
                        result.matched_items = matched_items;
                        if matches >= expected_min_recall as usize {
                            result.status = "pass".to_string();
                            result.detail = "Recall threshold met".to_string();
                            totals.passed += 1;
                        } else {
                            result.status = "fail".to_string();
                            result.detail = "Recall below threshold".to_string();
                            totals.failed += 1;
                        }
                        totals.scored += 1;
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        if msg.contains("Embedder not initialized") {
                            result.detail = "Embedder not configured; skipping search".to_string();
                            totals.skipped += 1;
                        } else {
                            result.status = "fail".to_string();
                            result.detail = format!("Search failed: {}", msg);
                            totals.failed += 1;
                        }
                    }
                }
            }
            "read_graph" => {
                let Some(node_id) = task.query.node_id.clone() else {
                    result.status = "fail".to_string();
                    result.detail = "Missing query.node_id".to_string();
                    totals.failed += 1;
                    results.push(result);
                    continue;
                };

                if !graph_path.exists() {
                    result.detail = format!("Missing graph at {}", graph_path.display());
                    totals.skipped += 1;
                    results.push(result);
                    continue;
                }

                let graph = CodeGraph::from_file(&graph_path.to_string_lossy())?;
                let normalized_id = normalize_task_node_id(&repo_path, &node_id);
                let hits = gather_graph_hits(&graph, &normalized_id)
                    .or_else(|| gather_graph_hits(&graph, &node_id));

                match hits {
                    Some(hits) => {
                        // Beklenen node_id'leri fuzzy lookup ile mevcut satır numaralarına çöz
                        let resolved_ids: Option<Vec<String>> =
                            task.expected.node_ids.as_ref().map(|ids| {
                                ids.iter()
                                    .map(|id| {
                                        let norm = normalize_task_node_id(&repo_path, id);
                                        graph
                                            .find_node_fuzzy_by_id(&norm)
                                            .or_else(|| graph.find_node_fuzzy_by_id(id))
                                            .map(|n| n.id)
                                            .unwrap_or(norm)
                                    })
                                    .collect()
                            });
                        let resolved_expected = Expected {
                            node_ids: resolved_ids,
                            file_paths: task.expected.file_paths.clone(),
                            min_recall: task.expected.min_recall,
                            max_rank: task.expected.max_rank,
                            reason_contains: task.expected.reason_contains.clone(),
                        };
                        let (matches, matched_items) = score_hits(&resolved_expected, &hits);
                        result.matches = matches;
                        result.matched_items = matched_items;
                        if matches >= expected_min_recall as usize {
                            result.status = "pass".to_string();
                            result.detail = "Recall threshold met".to_string();
                            totals.passed += 1;
                        } else {
                            result.status = "fail".to_string();
                            result.detail = "Recall below threshold".to_string();
                            totals.failed += 1;
                        }
                        totals.scored += 1;
                    }
                    None => {
                        result.status = "fail".to_string();
                        result.detail = "Node not found in graph".to_string();
                        totals.failed += 1;
                    }
                }
            }
            "get_context" => {
                let Some(file_path) = task.query.file_path.clone() else {
                    result.status = "fail".to_string();
                    result.detail = "Missing query.file_path".to_string();
                    totals.failed += 1;
                    results.push(result);
                    continue;
                };
                let Some(line) = task.query.line else {
                    result.status = "fail".to_string();
                    result.detail = "Missing query.line".to_string();
                    totals.failed += 1;
                    results.push(result);
                    continue;
                };

                if !graph_path.exists() {
                    result.detail = format!("Missing graph at {}", graph_path.display());
                    totals.skipped += 1;
                    results.push(result);
                    continue;
                }

                let graph = CodeGraph::from_file(&graph_path.to_string_lossy())?;
                let normalized_path = normalize_path(&repo_path, &file_path);
                let hits = gather_context_hits(&graph, &normalized_path, line as usize);

                match hits {
                    Some(hits) => {
                        let (matches, matched_items) = score_hits(&task.expected, &hits);
                        result.matches = matches;
                        result.matched_items = matched_items;
                        if matches >= expected_min_recall as usize {
                            result.status = "pass".to_string();
                            result.detail = "Recall threshold met".to_string();
                            totals.passed += 1;
                        } else {
                            result.status = "fail".to_string();
                            result.detail = "Recall below threshold".to_string();
                            totals.failed += 1;
                        }
                        totals.scored += 1;
                    }
                    None => {
                        result.status = "fail".to_string();
                        result.detail = "No node found at cursor position".to_string();
                        totals.failed += 1;
                    }
                }
            }
            other => {
                result.detail = format!("Unsupported query type: {}", other);
                totals.skipped += 1;
            }
        }

        results.push(result);
    }

    Ok(EvalReport {
        schema_version: tasks_file.schema_version,
        generated_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        totals,
        results,
    })
}

/// Belirli bir policy ile deterministik metrik üreten evaluation.
/// `evaluate_with_mode`'dan bağımsızdır; mevcut pass/fail davranışı korunur.
pub async fn evaluate_policy(
    tasks_file: GoldenTasksFile,
    policy: &RetrievalPolicy,
) -> Result<EvalReport> {
    let mut totals = Totals::default();
    let mut results = Vec::new();
    let mut prepared_repos: HashSet<PathBuf> = HashSet::new();
    let mut graph_cache: HashMap<PathBuf, Arc<CodeGraph>> = HashMap::new();

    totals.tasks = tasks_file.tasks.len();

    for task in tasks_file.tasks {
        let started = std::time::Instant::now();
        let repo_path = normalize_repo_path(&task.repo.path)?;
        let db_path = repo_path.join("data").join("ccm_db");
        let graph_path = repo_path.join("data").join("ccm_graph.json");

        let max_rank = task.expected.max_rank.unwrap_or(5).max(1);
        let expected_min_recall = task.expected.min_recall;

        let mut result = TaskResult {
            id: task.id.clone(),
            query_type: task.query.kind.clone(),
            status: "skipped".to_string(),
            detail: String::new(),
            matches: 0,
            expected_min_recall,
            max_rank,
            matched_items: Vec::new(),
            recall_at_k: None,
            precision_at_k: None,
            relevant_coverage: None,
            tokens_estimated: None,
            latency_ms: None,
            ranked: None,
            missing_relevant: None,
            retrieved_unused: None,
            policy_version: Some(policy.version),
        };

        if let Err(error) =
            ensure_eval_index(&repo_path, &db_path, &graph_path, &mut prepared_repos).await
        {
            result.detail = format!("Failed to prepare index: {}", error);
            totals.skipped += 1;
            results.push(result);
            continue;
        }

        let mut ranked: Vec<String> = Vec::new();
        let mut token_hint: usize = 0;

        match task.query.kind.as_str() {
            "search_code" => {
                let Some(query_text) = task.query.text.clone() else {
                    result.status = "fail".to_string();
                    result.detail = "Missing query.text".to_string();
                    totals.failed += 1;
                    results.push(result);
                    continue;
                };

                let search_limit = policy.top_k.max(1);
                let search_result = if graph_path.exists() {
                    let scorer = HybridScorer::from_policy(policy);
                    search_code_hybrid_with_policy(
                        &db_path,
                        &graph_path,
                        &query_text,
                        search_limit,
                        &scorer,
                        policy.seed_multiplier as usize,
                    )
                    .await
                } else {
                    search_code(&db_path, &query_text, search_limit).await
                };

                match search_result {
                    Ok(hits) => {
                        ranked = hits.clone();
                        if let Ok(graph) = cached_graph(&graph_path, &mut graph_cache) {
                            token_hint = tokens_for_ids(&graph, &hits);
                        }
                        let (matches, matched_items) = score_hits(&task.expected, &hits);
                        result.matches = matches;
                        result.matched_items = matched_items;
                        if matches >= expected_min_recall as usize {
                            result.status = "pass".to_string();
                            result.detail = "Recall threshold met".to_string();
                            totals.passed += 1;
                        } else {
                            result.status = "fail".to_string();
                            result.detail = "Recall below threshold".to_string();
                            totals.failed += 1;
                        }
                        totals.scored += 1;
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        if msg.contains("Embedder not initialized") {
                            result.detail = "Embedder not configured; skipping search".to_string();
                            totals.skipped += 1;
                        } else {
                            result.status = "fail".to_string();
                            result.detail = format!("Search failed: {}", msg);
                            totals.failed += 1;
                        }
                    }
                }
            }
            "read_graph" => {
                let Some(node_id) = task.query.node_id.clone() else {
                    result.status = "fail".to_string();
                    result.detail = "Missing query.node_id".to_string();
                    totals.failed += 1;
                    results.push(result);
                    continue;
                };

                if !graph_path.exists() {
                    result.detail = format!("Missing graph at {}", graph_path.display());
                    totals.skipped += 1;
                    results.push(result);
                    continue;
                }

                let graph = cached_graph(&graph_path, &mut graph_cache)?;
                let normalized_id = normalize_task_node_id(&repo_path, &node_id);
                let hits = gather_graph_hits(&graph, &normalized_id)
                    .or_else(|| gather_graph_hits(&graph, &node_id));

                match hits {
                    Some(hits) => {
                        ranked = hits.clone();
                        token_hint = tokens_for_ids(&graph, &hits);
                        let resolved_ids: Option<Vec<String>> =
                            task.expected.node_ids.as_ref().map(|ids| {
                                ids.iter()
                                    .map(|id| {
                                        let norm = normalize_task_node_id(&repo_path, id);
                                        graph
                                            .find_node_fuzzy_by_id(&norm)
                                            .or_else(|| graph.find_node_fuzzy_by_id(id))
                                            .map(|n| n.id)
                                            .unwrap_or(norm)
                                    })
                                    .collect()
                            });
                        let resolved_expected = Expected {
                            node_ids: resolved_ids,
                            file_paths: task.expected.file_paths.clone(),
                            min_recall: task.expected.min_recall,
                            max_rank: task.expected.max_rank,
                            reason_contains: task.expected.reason_contains.clone(),
                        };
                        let (matches, matched_items) = score_hits(&resolved_expected, &hits);
                        result.matches = matches;
                        result.matched_items = matched_items;
                        if matches >= expected_min_recall as usize {
                            result.status = "pass".to_string();
                            result.detail = "Recall threshold met".to_string();
                            totals.passed += 1;
                        } else {
                            result.status = "fail".to_string();
                            result.detail = "Recall below threshold".to_string();
                            totals.failed += 1;
                        }
                        totals.scored += 1;
                    }
                    None => {
                        result.status = "fail".to_string();
                        result.detail = "Node not found in graph".to_string();
                        totals.failed += 1;
                    }
                }
            }
            "get_context" | "predict_context" => {
                let Some(file_path) = task.query.file_path.clone() else {
                    result.status = "fail".to_string();
                    result.detail = "Missing query.file_path".to_string();
                    totals.failed += 1;
                    results.push(result);
                    continue;
                };
                let Some(line) = task.query.line else {
                    result.status = "fail".to_string();
                    result.detail = "Missing query.line".to_string();
                    totals.failed += 1;
                    results.push(result);
                    continue;
                };

                if !graph_path.exists() {
                    result.detail = format!("Missing graph at {}", graph_path.display());
                    totals.skipped += 1;
                    results.push(result);
                    continue;
                }

                let normalized_path = normalize_path(&repo_path, &file_path);
                let hits = if task.query.kind == "predict_context" {
                    let graph = match cached_graph(&graph_path, &mut graph_cache) {
                        Ok(graph) => graph,
                        Err(e) => {
                            result.status = "fail".to_string();
                            result.detail = format!("Graph load failed: {}", e);
                            totals.failed += 1;
                            results.push(result);
                            continue;
                        }
                    };
                    let store =
                        match LanceDbStore::new(&db_path.to_string_lossy(), "code_vectors").await {
                            Ok(store) => store,
                            Err(e) => {
                                result.status = "fail".to_string();
                                result.detail = format!("Store open failed: {}", e);
                                totals.failed += 1;
                                results.push(result);
                                continue;
                            }
                        };
                    let engine = RetrievalEngine::with_policy(
                        Arc::new(tokio::sync::RwLock::new((*graph).clone())),
                        store,
                        policy.clone(),
                    );
                    let cursor = CursorPosition {
                        file_path: normalized_path.clone(),
                        line: line as usize,
                        column: task.query.column.unwrap_or(0) as usize,
                    };
                    match engine.predict_context(&cursor).await {
                        Ok(suggestions) => {
                            token_hint = suggestions
                                .iter()
                                .map(|suggestion| suggestion.content.len() / 4)
                                .sum();
                            suggestions
                                .into_iter()
                                .filter_map(|suggestion| suggestion.node_id)
                                .collect::<Vec<String>>()
                        }
                        Err(e) => {
                            result.status = "fail".to_string();
                            result.detail = format!("Predict failed: {}", e);
                            totals.failed += 1;
                            results.push(result);
                            continue;
                        }
                    }
                } else {
                    let graph = cached_graph(&graph_path, &mut graph_cache)?;
                    let hits = gather_context_hits(&graph, &normalized_path, line as usize)
                        .unwrap_or_default();
                    token_hint = tokens_for_ids(&graph, &hits);
                    hits
                };

                ranked = hits.clone();
                let (matches, matched_items) = score_hits(&task.expected, &hits);
                result.matches = matches;
                result.matched_items = matched_items;
                if matches >= expected_min_recall as usize {
                    result.status = "pass".to_string();
                    result.detail = "Recall threshold met".to_string();
                    totals.passed += 1;
                } else {
                    result.status = "fail".to_string();
                    result.detail = "Recall below threshold".to_string();
                    totals.failed += 1;
                }
                totals.scored += 1;
            }
            other => {
                result.detail = format!("Unsupported query type: {}", other);
                totals.skipped += 1;
            }
        }

        fill_policy_metrics(&mut result, &task, &ranked, token_hint, started.elapsed());
        results.push(result);
    }

    Ok(EvalReport {
        schema_version: tasks_file.schema_version,
        generated_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        totals,
        results,
    })
}

fn fill_policy_metrics(
    result: &mut TaskResult,
    task: &Task,
    ranked: &[String],
    token_hint: usize,
    elapsed: std::time::Duration,
) {
    let expected_total = task
        .expected
        .node_ids
        .as_ref()
        .map(Vec::len)
        .or_else(|| task.expected.file_paths.as_ref().map(Vec::len))
        .unwrap_or(0);
    let matched = result.matches as f64;
    let expected = expected_total as f64;
    result.recall_at_k = Some(if expected > 0.0 {
        matched / expected
    } else {
        0.0
    });
    result.precision_at_k = Some(if ranked.is_empty() {
        0.0
    } else {
        matched / ranked.len() as f64
    });
    result.relevant_coverage = result.recall_at_k;
    result.latency_ms = Some(elapsed.as_secs_f64() * 1000.0);
    result.ranked = Some(ranked.to_vec());

    let matched_set: std::collections::HashSet<&str> =
        result.matched_items.iter().map(String::as_str).collect();
    result.missing_relevant = Some(
        task.expected
            .node_ids
            .iter()
            .flatten()
            .chain(task.expected.file_paths.iter().flatten())
            .filter(|id| !matched_set.contains(id.as_str()))
            .cloned()
            .collect(),
    );
    result.retrieved_unused = Some(
        ranked
            .iter()
            .filter(|id| !matched_set.contains(id.as_str()))
            .cloned()
            .collect(),
    );
    // Token tahmini: dönen içerik uzunluğu / 4 (yalnızca göreli karşılaştırma).
    result.tokens_estimated = Some(token_hint.max(1));
}

fn tokens_for_ids(graph: &CodeGraph, ranked: &[String]) -> usize {
    ranked
        .iter()
        .filter_map(|id| graph.find_node_by_id(id))
        .map(|node| node.content.len() / 4)
        .sum()
}

fn cached_graph(
    graph_path: &Path,
    cache: &mut HashMap<PathBuf, Arc<CodeGraph>>,
) -> Result<Arc<CodeGraph>> {
    if let Some(graph) = cache.get(graph_path) {
        return Ok(Arc::clone(graph));
    }
    let graph = Arc::new(CodeGraph::from_file(&graph_path.to_string_lossy())?);
    cache.insert(graph_path.to_path_buf(), Arc::clone(&graph));
    Ok(graph)
}

async fn ensure_eval_index(
    repo_path: &Path,
    db_path: &Path,
    graph_path: &Path,
    prepared_repos: &mut HashSet<PathBuf>,
) -> Result<()> {
    if db_path.exists() && graph_path.exists() {
        return Ok(());
    }

    let repo_key = repo_path.to_path_buf();
    if prepared_repos.contains(&repo_key) {
        return Ok(());
    }

    crate::update_index(
        &repo_path.to_string_lossy(),
        Some(&db_path.to_string_lossy()),
    )
    .await
    .with_context(|| {
        format!(
            "failed to build evaluation index for {}",
            repo_path.display()
        )
    })?;

    prepared_repos.insert(repo_key);

    if !db_path.exists() {
        return Err(anyhow::anyhow!("Missing index at {}", db_path.display()));
    }

    if !graph_path.exists() {
        return Err(anyhow::anyhow!("Missing graph at {}", graph_path.display()));
    }

    Ok(())
}

async fn search_code(db_path: &Path, query: &str, limit: usize) -> Result<Vec<String>> {
    let store = LanceDbStore::new(&db_path.to_string_lossy(), "code_vectors").await?;
    let hits = store.search(query, limit).await?;
    let mut ids = Vec::new();
    for (id, _, _) in hits {
        ids.push(id);
    }
    Ok(ids)
}

async fn search_code_hybrid(
    db_path: &Path,
    graph_path: &Path,
    query: &str,
    limit: usize,
) -> Result<Vec<String>> {
    let scorer = HybridScorer::default();
    search_code_hybrid_with_policy(db_path, graph_path, query, limit, &scorer, 3).await
}

async fn search_code_hybrid_with_policy(
    db_path: &Path,
    graph_path: &Path,
    query: &str,
    limit: usize,
    scorer: &HybridScorer,
    seed_multiplier: usize,
) -> Result<Vec<String>> {
    let store = LanceDbStore::new(&db_path.to_string_lossy(), "code_vectors").await?;
    let seed_limit = limit.saturating_mul(seed_multiplier).max(limit);
    let hits = store.search(query, seed_limit).await?;
    if hits.is_empty() {
        // Production ile aynı: vector sonucu yoksa graph üzerinden lexical fallback.
        let graph = CodeGraph::from_file(&graph_path.to_string_lossy())?;
        let mut fallback = Vec::new();
        for node in graph.graph.node_weights() {
            let file_path = crate::engine::extract_file_path(&node.id);
            if node.name.contains(query) || file_path.contains(query) {
                fallback.push(node.id.clone());
            }
        }
        fallback.sort();
        fallback.truncate(limit);
        return Ok(fallback);
    }

    let graph = CodeGraph::from_file(&graph_path.to_string_lossy())?;
    let mut semantic_scores: HashMap<String, f32> = HashMap::new();

    for (id, _content, distance) in hits {
        let node_id = crate::normalize_node_id(&id);
        let score = HybridScorer::semantic_score(distance);
        semantic_scores
            .entry(node_id)
            .and_modify(|existing| {
                if score > *existing {
                    *existing = score;
                }
            })
            .or_insert(score);
    }

    let seed_ids: Vec<String> = semantic_scores.keys().cloned().collect();
    let graph_scores = scorer.collect_graph_scores(&graph, &seed_ids);

    let mut candidates: Vec<(String, f32)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for id in semantic_scores.keys().chain(graph_scores.keys()) {
        if seen.insert(id.clone()) {
            let graph_score = graph_scores.get(id).copied().unwrap_or(0.0);
            let semantic_score = semantic_scores.get(id).copied().unwrap_or(0.0);
            let spatial_score = graph
                .find_node_by_id(id)
                .map(|node| {
                    crate::engine::repo_priority_score(&crate::engine::extract_file_path(&node.id))
                })
                .unwrap_or(0.0);
            let combined = scorer.combine(graph_score, semantic_score, spatial_score, 0.0);
            candidates.push((id.clone(), combined));
        }
    }

    // Production ile aynı filtre: düşük sinyalli adaylar elenir (graph sinyali korunur).
    candidates.retain(|(_, combined)| *combined >= scorer.min_score);

    candidates.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });

    Ok(candidates
        .into_iter()
        .take(limit)
        .map(|(id, _)| id)
        .collect())
}

fn score_hits(expected: &Expected, hits: &[String]) -> (usize, Vec<String>) {
    let mut matched = Vec::new();
    let mut expected_set: HashSet<String> = HashSet::new();

    if let Some(node_ids) = &expected.node_ids {
        expected_set.extend(node_ids.iter().cloned());
    } else if let Some(file_paths) = &expected.file_paths {
        expected_set.extend(file_paths.iter().cloned());
    }

    if expected_set.is_empty() {
        return (0, matched);
    }

    for hit in hits {
        let key = if expected.node_ids.is_some() {
            crate::normalize_node_id(hit)
        } else {
            node_id_to_file_path(hit)
        };

        if expected_set.contains(&key) {
            matched.push(key);
        }
    }

    matched.sort();
    matched.dedup();
    (matched.len(), matched)
}

fn build_comparison_summary(structural: &EvalReport, hybrid: &EvalReport) -> ComparisonSummary {
    let structural_pass_rate = pass_rate(structural);
    let hybrid_pass_rate = pass_rate(hybrid);

    let structural_by_type = query_type_pass_rate(structural);
    let hybrid_by_type = query_type_pass_rate(hybrid);

    let mut by_query_type = BTreeMap::new();
    for kind in structural_by_type.keys().chain(hybrid_by_type.keys()) {
        let structural_rate = structural_by_type.get(kind).copied().unwrap_or(0.0);
        let hybrid_rate = hybrid_by_type.get(kind).copied().unwrap_or(0.0);
        by_query_type.insert(
            kind.clone(),
            QueryTypeComparison {
                structural_pass_rate: structural_rate,
                hybrid_pass_rate: hybrid_rate,
                improvement: hybrid_rate - structural_rate,
            },
        );
    }

    ComparisonSummary {
        structural_pass_rate,
        hybrid_pass_rate,
        improvement: hybrid_pass_rate - structural_pass_rate,
        by_query_type,
    }
}

fn pass_rate(report: &EvalReport) -> f64 {
    report_pass_rate(report)
}

fn query_type_pass_rate(report: &EvalReport) -> BTreeMap<String, f64> {
    let mut totals: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for result in &report.results {
        let entry = totals.entry(result.query_type.clone()).or_insert((0, 0));
        entry.0 += 1;
        if result.status == "pass" {
            entry.1 += 1;
        }
    }

    totals
        .into_iter()
        .map(|(kind, (total, passed))| {
            let rate = if total == 0 {
                0.0
            } else {
                (passed as f64 / total as f64) * 100.0
            };
            (kind, rate)
        })
        .collect()
}

fn normalize_repo_path(path: &str) -> Result<PathBuf> {
    let raw = PathBuf::from(path);
    if raw.is_absolute() {
        return Ok(raw);
    }
    let cwd = std::env::current_dir().context("Failed to get current directory")?;
    Ok(cwd.join(raw))
}

fn normalize_task_node_id(repo_path: &Path, node_id: &str) -> String {
    let cleaned = node_id.replace('\\', "/");
    if let Some((path_and_kind, suffix)) = cleaned.split_once(":symbol:") {
        if let Some((file, kind)) = path_and_kind.rsplit_once(':') {
            return format!(
                "{}:{}:symbol:{}",
                normalize_path(repo_path, file),
                kind,
                suffix
            );
        }
    }
    let mut parts = cleaned.rsplitn(4, ':');
    let col = parts.next();
    let row = parts.next();
    let kind = parts.next();
    let file = parts.next();

    if let (Some(col), Some(row), Some(kind), Some(file)) = (col, row, kind, file) {
        let normalized_file = normalize_path(repo_path, file);
        return format!("{}:{}:{}:{}", normalized_file, kind, row, col);
    }

    normalize_path(repo_path, &cleaned)
}

fn normalize_path(repo_path: &Path, raw: &str) -> String {
    let path = Path::new(raw);
    if path.is_absolute() {
        if let Ok(rel) = path.strip_prefix(repo_path) {
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            return format!("./{}", rel_str);
        }
    }

    let mut normalized = raw.replace('\\', "/");
    if !normalized.starts_with("./") && !normalized.starts_with("/") {
        normalized = format!("./{}", normalized);
    }
    normalized
}

fn gather_graph_hits(graph: &CodeGraph, node_id: &str) -> Option<Vec<String>> {
    // Önce fuzzy arama yap (satır numarası kaymasını tolere eder), bulamazsa exact dene
    let resolved_id = graph.find_node_fuzzy_by_id(node_id)?.id;
    let idx = graph.find_node_index_by_id(&resolved_id)?;
    let mut hits = Vec::new();

    hits.push(graph.graph[idx].id.clone());

    for edge in graph.graph.edges_directed(idx, Direction::Outgoing) {
        hits.push(graph.graph[edge.target()].id.clone());
    }

    for edge in graph.graph.edges_directed(idx, Direction::Incoming) {
        hits.push(graph.graph[edge.source()].id.clone());
    }

    Some(hits)
}

fn gather_context_hits(graph: &CodeGraph, file_path: &str, line: usize) -> Option<Vec<String>> {
    let idx = graph.find_node_in_file(file_path, line)?;
    let node_id = graph.graph[idx].id.clone();
    gather_graph_hits(graph, &node_id)
}

fn node_id_to_file_path(id: &str) -> String {
    let cleaned = crate::normalize_node_id(id);
    if let Some((file_path, _)) = cleaned.split_once(':') {
        file_path.to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{CodeNode, EdgeType, NodeType};

    #[test]
    fn score_hits_matches_file_paths() {
        let expected = Expected {
            node_ids: None,
            file_paths: Some(vec!["./src/main.rs".to_string()]),
            min_recall: 1,
            max_rank: None,
            reason_contains: None,
        };

        let hits = vec![
            "./src/main.rs:func:1:0".to_string(),
            "./src/other.rs:func:2:0".to_string(),
        ];

        let (matches, items) = score_hits(&expected, &hits);
        assert_eq!(matches, 1);
        assert_eq!(items, vec!["./src/main.rs".to_string()]);
    }

    #[test]
    fn score_hits_matches_node_ids() {
        let expected = Expected {
            node_ids: Some(vec!["./src/main.rs:func:1:0".to_string()]),
            file_paths: None,
            min_recall: 1,
            max_rank: None,
            reason_contains: None,
        };

        let hits = vec![
            "./src/main.rs:func:1:0#chunk0".to_string(),
            "./src/other.rs:func:2:0".to_string(),
        ];

        let (matches, items) = score_hits(&expected, &hits);
        assert_eq!(matches, 1);
        assert_eq!(items, vec!["./src/main.rs:func:1:0".to_string()]);
    }

    #[test]
    fn score_hits_empty_expected() {
        let expected = Expected {
            node_ids: None,
            file_paths: None,
            min_recall: 1,
            max_rank: None,
            reason_contains: None,
        };

        let hits = vec!["./src/main.rs:func:1:0".to_string()];
        let (matches, items) = score_hits(&expected, &hits);
        assert_eq!(matches, 0);
        assert!(items.is_empty());
    }

    #[test]
    fn node_id_to_file_path_strips_suffix() {
        let file_path = node_id_to_file_path("./src/main.rs:func:1:0#chunk1");
        assert_eq!(file_path, "./src/main.rs");
    }

    #[test]
    fn normalize_task_node_id_preserves_stable_symbol_ids() {
        let repo = Path::new("/repo");
        let id = "./src/main.rs:function_item:symbol:0123456789abcdef:0";
        assert_eq!(normalize_task_node_id(repo, id), id);
    }

    #[test]
    fn gather_graph_hits_includes_neighbors() {
        let mut graph = CodeGraph::new();
        let node_a = CodeNode {
            id: "./src/a.rs:func:1:0".to_string(),
            node_type: NodeType::Function,
            name: "a".to_string(),
            content: "fn a() {}".into(),
            start_line: 1,
            end_line: 1,
        };
        let node_b = CodeNode {
            id: "./src/b.rs:func:2:0".to_string(),
            node_type: NodeType::Function,
            name: "b".to_string(),
            content: "fn b() {}".into(),
            start_line: 2,
            end_line: 2,
        };

        let idx_a = graph.add_node(node_a);
        let idx_b = graph.add_node(node_b);
        graph.add_edge(idx_a, idx_b, EdgeType::Calls);

        let hits = gather_graph_hits(&graph, "./src/a.rs:func:1:0").expect("missing hits");
        assert!(hits.contains(&"./src/a.rs:func:1:0".to_string()));
        assert!(hits.contains(&"./src/b.rs:func:2:0".to_string()));
    }

    #[test]
    fn gather_context_hits_finds_node_by_line() {
        let mut graph = CodeGraph::new();
        let file_node = CodeNode {
            id: "./src/main.rs".to_string(),
            node_type: NodeType::File,
            name: "./src/main.rs".to_string(),
            content: "".into(),
            start_line: 1,
            end_line: 10,
        };
        let func_node = CodeNode {
            id: "./src/main.rs:func:1:0".to_string(),
            node_type: NodeType::Function,
            name: "main".to_string(),
            content: "fn main() {}".into(),
            start_line: 1,
            end_line: 1,
        };

        let file_idx = graph.add_node(file_node);
        let func_idx = graph.add_node(func_node);
        graph.add_edge(file_idx, func_idx, EdgeType::Contains);

        let hits = gather_context_hits(&graph, "./src/main.rs", 1).expect("missing hits");
        assert!(hits.contains(&"./src/main.rs:func:1:0".to_string()));
    }

    #[test]
    fn comparison_summary_tracks_improvement() {
        let structural = EvalReport {
            schema_version: 1,
            generated_at: 0,
            totals: Totals {
                tasks: 2,
                scored: 2,
                passed: 1,
                failed: 1,
                skipped: 0,
            },
            results: vec![
                TaskResult {
                    id: "a".to_string(),
                    query_type: "read_graph".to_string(),
                    status: "pass".to_string(),
                    detail: String::new(),
                    matches: 1,
                    expected_min_recall: 1,
                    max_rank: 1,
                    matched_items: vec![],
                    ..Default::default()
                },
                TaskResult {
                    id: "b".to_string(),
                    query_type: "read_graph".to_string(),
                    status: "fail".to_string(),
                    detail: String::new(),
                    matches: 0,
                    expected_min_recall: 1,
                    max_rank: 1,
                    matched_items: vec![],
                    ..Default::default()
                },
            ],
        };

        let hybrid = EvalReport {
            schema_version: 1,
            generated_at: 0,
            totals: Totals {
                tasks: 2,
                scored: 2,
                passed: 2,
                failed: 0,
                skipped: 0,
            },
            results: vec![
                TaskResult {
                    id: "a".to_string(),
                    query_type: "read_graph".to_string(),
                    status: "pass".to_string(),
                    detail: String::new(),
                    matches: 1,
                    expected_min_recall: 1,
                    max_rank: 1,
                    matched_items: vec![],
                    ..Default::default()
                },
                TaskResult {
                    id: "b".to_string(),
                    query_type: "read_graph".to_string(),
                    status: "pass".to_string(),
                    detail: String::new(),
                    matches: 1,
                    expected_min_recall: 1,
                    max_rank: 1,
                    matched_items: vec![],
                    ..Default::default()
                },
            ],
        };

        let summary = build_comparison_summary(&structural, &hybrid);
        assert!(summary.improvement > 0.0);
        let by_type = summary
            .by_query_type
            .get("read_graph")
            .expect("missing type");
        assert!(by_type.improvement > 0.0);
    }
}
