//! Kontekst aramasının sıralama yöntemlerini altın `search_code` görevleriyle
//! ölçer: Recall@1/5/10, MRR@10 ve sıcak indeksteki gecikme yüzdelikleri.
//!
//! Ölçüm üretimdeki `RetrievalEngine::search_code_hybrid` yolunu çağırır. Her
//! proje için tek motor açılır; yöntemler aynı indeks, aynı sorgu ve aynı
//! limitle çalışır, aralarında yalnızca `RetrievalPolicy::ranking` değişir.
//! Embedding kapalıysa, indeks başka bir modelle ya da görevlerden farklı bir
//! commit'te kurulduysa veya motor semantik aramadan başka bir yola düştüyse
//! ölçüm hata verir; sessiz fallback yoktur. Aynı sorgunun tekrarları farklı
//! sıralama verirse sıralama deterministik sayılmaz ve ölçüm yine hata verir.
//!
//! Bir görevin sırası, altın kümeyle eşleşen ilk sonucun 1'den başlayan
//! konumudur. Dosya düzeyinde sonuç düğümünün dosya yolu, fonksiyon düzeyinde
//! düğüm kimliği karşılaştırılır. Recall@K sırası K'dan küçük ya da eşit olan
//! görevlerin payıdır; MRR@10 sıra tersinin ortalamasıdır (10'dan sonraki ya
//! da bulunmayan eşleşme 0).

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::RwLock;

use crate::engine::{ContextSuggestion, RetrievalEngine};
use crate::eval::{
    node_id_to_file_path, normalize_path, normalize_repo_path, normalize_task_node_id,
    GoldenTasksFile, Task,
};
use crate::graph::{CodeGraph, EdgeType};
use crate::policy::{RankingMethod, RetrievalPolicy};
use crate::vector::embedder::{EmbeddingIdentity, EmbeddingSource};
use crate::vector::store::LanceDbStore;

pub const RETRIEVAL_EVAL_SCHEMA_VERSION: u32 = 1;

/// Recall@10 ve MRR@10 için gereken en az sonuç sayısı.
const METRIC_DEPTH: usize = 10;

const SEARCH_CODE_KIND: &str = "search_code";

/// H0 güven fallback'inin gerekçe önekleri; hibrit sonucun gerekçesi bunlardan
/// biriyle ya da doğrudan `Hybrid (` ile başlar.
const HYBRID_REASON_PREFIXES: [&str; 4] = ["", "GraphFallback ", "SemanticFallback ", "LowConf "];

/// Ölçüm ayarları; hepsi çağıran tarafından açıkça verilir.
#[derive(Debug, Clone)]
pub struct RetrievalEvalSettings {
    pub task_files: Vec<PathBuf>,
    pub methods: Vec<RankingMethod>,
    /// `search_code_hybrid` sonuç limiti; Recall@10 için en az 10.
    pub limit: usize,
    /// H1'in λ değeri; diğer yöntemler kullanmaz ama raporda yer alır.
    pub propagation_weight: f32,
    /// Her görev ve yöntem için ölçülen sorgu tekrarı.
    pub repeats: usize,
    /// Ölçülen CCM kodunun commit'i (7–40 küçük harf onaltılık).
    pub commit: String,
}

/// Görevin altın kümesi: dosya yolları ya da düğüm kimlikleri, normalize edilmiş.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "level", content = "targets", rename_all = "snake_case")]
pub enum Gold {
    File(Vec<String>),
    Node(Vec<String>),
}

#[derive(Debug, Clone, Serialize)]
pub struct RetrievalEvalReport {
    pub schema_version: u32,
    pub generated_at: u64,
    pub commit: String,
    pub dataset: DatasetSummary,
    pub settings: ReportSettings,
    pub methods: Vec<MethodReport>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DatasetSummary {
    pub task_files: Vec<TaskFileSummary>,
    pub tasks: usize,
    pub projects: Vec<ProjectSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskFileSummary {
    pub path: String,
    pub sha256: String,
    /// Ölçülen `search_code` görevleri.
    pub tasks: usize,
    /// Başka sorgu türündeki, ölçülmeyen görevler.
    pub skipped: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectSummary {
    pub name: String,
    pub path: String,
    pub repo_commit: Option<String>,
    pub generation_id: Option<String>,
    pub indexed_commit: Option<String>,
    pub embedding: EmbeddingIdentity,
    pub node_count: usize,
    pub edge_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReportSettings {
    pub limit: usize,
    pub repeats: usize,
    pub methods: Vec<RankingMethod>,
    pub seed_multiplier: u32,
    pub propagation_weight: f32,
    pub edge_weights: Vec<(EdgeType, f32)>,
    pub hardware: Hardware,
}

#[derive(Debug, Clone, Serialize)]
pub struct Hardware {
    pub os: String,
    pub arch: String,
    pub logical_cpus: usize,
    pub physical_cpus: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct MethodReport {
    pub method: RankingMethod,
    pub metrics: MethodMetrics,
    pub tasks: Vec<TaskOutcome>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MethodMetrics {
    pub tasks: usize,
    pub recall_at_1: f64,
    pub recall_at_5: f64,
    pub recall_at_10: f64,
    pub mrr_at_10: f64,
    /// Tüm görevlerin tüm ölçülen tekrarları üzerinden.
    pub latency_p50_ms: f64,
    pub latency_p95_ms: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskOutcome {
    pub id: String,
    pub project: String,
    pub split: Option<String>,
    pub gold: Gold,
    pub rank: Option<usize>,
    pub top_results: Vec<RankedResult>,
    pub latencies_ms: Vec<f64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RankedResult {
    pub node_id: String,
    /// Altın dosya yollarıyla karşılaştırılan normalize yol.
    pub file_path: String,
    pub score: f32,
    pub reason: String,
}

#[derive(Debug, Clone)]
struct EvalTask {
    id: String,
    project: String,
    repo_path: String,
    repo_commit: Option<String>,
    split: Option<String>,
    query: String,
    gold: Gold,
}

struct LoadedTaskFile {
    summary: TaskFileSummary,
    tasks: Vec<EvalTask>,
}

struct ProjectTasks {
    name: String,
    repo_path: String,
    repo_commit: Option<String>,
    tasks: Vec<EvalTask>,
}

struct Measurement {
    method: RankingMethod,
    outcome: TaskOutcome,
}

struct TimedRun {
    latency_ms: f64,
    results: Vec<ContextSuggestion>,
}

/// Ayarları doğrular, görevleri yükler, her projeyi ölçer ve raporu döndürür.
pub async fn run_retrieval_eval(settings: &RetrievalEvalSettings) -> Result<RetrievalEvalReport> {
    validate_settings(settings)?;
    let loaded = settings
        .task_files
        .iter()
        .map(|path| load_task_file(path))
        .collect::<Result<Vec<LoadedTaskFile>>>()?;
    let task_files: Vec<TaskFileSummary> = loaded.iter().map(|file| file.summary.clone()).collect();
    let tasks: Vec<EvalTask> = loaded.into_iter().flat_map(|file| file.tasks).collect();
    if tasks.is_empty() {
        bail!("Task files contain no search_code tasks");
    }
    ensure_unique_task_ids(&tasks)?;
    let task_count = tasks.len();
    let projects = group_by_project(tasks)?;

    let source = EmbeddingSource::from_env()?;
    if source == EmbeddingSource::Disabled {
        bail!(
            "Embeddings are disabled; retrieval evaluation needs the embedder that built the index"
        );
    }

    let mut project_summaries = Vec::with_capacity(projects.len());
    let mut measurements = Vec::with_capacity(task_count * settings.methods.len());
    for project in &projects {
        let (summary, project_measurements) = evaluate_project(project, &source, settings).await?;
        project_summaries.push(summary);
        measurements.extend(project_measurements);
    }

    let methods = settings
        .methods
        .iter()
        .map(|method| method_report(*method, &measurements))
        .collect::<Result<Vec<MethodReport>>>()?;
    let baseline = RetrievalPolicy::baseline();
    Ok(RetrievalEvalReport {
        schema_version: RETRIEVAL_EVAL_SCHEMA_VERSION,
        generated_at: crate::unix_now_secs(),
        commit: settings.commit.clone(),
        dataset: DatasetSummary {
            task_files,
            tasks: task_count,
            projects: project_summaries,
        },
        settings: ReportSettings {
            limit: settings.limit,
            repeats: settings.repeats,
            methods: settings.methods.clone(),
            seed_multiplier: baseline.seed_multiplier,
            propagation_weight: settings.propagation_weight,
            edge_weights: baseline.edge_weights,
            hardware: current_hardware(),
        },
        methods,
    })
}

/// Raporu girintili JSON olarak yazar.
pub fn write_report(report: &RetrievalEvalReport, path: &Path) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(report).context("Report could not be serialized")?;
    bytes.push(b'\n');
    std::fs::write(path, bytes)
        .with_context(|| format!("Report file '{}' could not be written", path.display()))
}

/// Yöntem başına metrik özeti; ilk satır başlıktır.
pub fn summary_lines(report: &RetrievalEvalReport) -> Vec<String> {
    let header = format!(
        "{:<12} {:>5} {:>6} {:>6} {:>6} {:>7} {:>9} {:>9}",
        "method", "tasks", "R@1", "R@5", "R@10", "MRR@10", "p50 ms", "p95 ms"
    );
    std::iter::once(header)
        .chain(report.methods.iter().map(|method| {
            let metrics = &method.metrics;
            format!(
                "{:<12} {:>5} {:>6.3} {:>6.3} {:>6.3} {:>7.3} {:>9.1} {:>9.1}",
                method.method.as_str(),
                metrics.tasks,
                metrics.recall_at_1,
                metrics.recall_at_5,
                metrics.recall_at_10,
                metrics.mrr_at_10,
                metrics.latency_p50_ms,
                metrics.latency_p95_ms
            )
        }))
        .collect()
}

/// Altın eşleşmesi ilk `k` sonuç içinde olan görevlerin payı.
pub fn recall_at(ranks: &[Option<usize>], k: usize) -> Result<f64> {
    if ranks.is_empty() {
        bail!("Recall@{} needs at least one task", k);
    }
    let hits = ranks
        .iter()
        .filter(|rank| rank.is_some_and(|value| value <= k))
        .count();
    Ok(hits as f64 / ranks.len() as f64)
}

/// İlk altın eşleşmesinin sıra tersinin ortalaması; `k`'dan sonraki ya da hiç
/// bulunmayan eşleşme 0 sayılır.
pub fn mrr_at(ranks: &[Option<usize>], k: usize) -> Result<f64> {
    if ranks.is_empty() {
        bail!("MRR@{} needs at least one task", k);
    }
    let total: f64 = ranks
        .iter()
        .map(|rank| match rank {
            Some(value) if *value <= k => 1.0 / *value as f64,
            _ => 0.0,
        })
        .sum();
    Ok(total / ranks.len() as f64)
}

/// En yakın sıra yöntemiyle yüzdelik: artan sıralı örneklerin ⌈p·n/100⌉.
/// elemanı (en az birinci eleman).
pub fn percentile_nearest_rank(samples: &[f64], percent: usize) -> Result<f64> {
    if samples.is_empty() {
        bail!("Percentile needs at least one sample");
    }
    if percent == 0 || percent > 100 {
        bail!("Percentile {} is outside 1..=100", percent);
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let rank = ((percent * sorted.len()).div_ceil(100)).max(1);
    Ok(sorted[rank - 1])
}

fn validate_settings(settings: &RetrievalEvalSettings) -> Result<()> {
    if settings.task_files.is_empty() {
        bail!("At least one task file is required");
    }
    if settings.methods.is_empty() {
        bail!("At least one ranking method is required");
    }
    let unique: HashSet<RankingMethod> = settings.methods.iter().copied().collect();
    if unique.len() != settings.methods.len() {
        let names: Vec<&str> = settings.methods.iter().map(RankingMethod::as_str).collect();
        bail!("Ranking methods must not repeat: {}", names.join(","));
    }
    if settings.limit < METRIC_DEPTH {
        bail!(
            "limit {} is below {}; Recall@10 and MRR@10 need at least {} results",
            settings.limit,
            METRIC_DEPTH,
            METRIC_DEPTH
        );
    }
    if settings.repeats == 0 {
        bail!("repeats must be at least 1");
    }
    if !(0.0..=1.0).contains(&settings.propagation_weight) {
        bail!(
            "propagation weight {} is outside [0, 1]",
            settings.propagation_weight
        );
    }
    if !is_commit_hash(&settings.commit) {
        bail!(
            "commit '{}' is not a 7-40 character lowercase hexadecimal hash",
            settings.commit
        );
    }
    Ok(())
}

fn is_commit_hash(raw: &str) -> bool {
    (7..=40).contains(&raw.len())
        && raw
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn load_task_file(path: &Path) -> Result<LoadedTaskFile> {
    let bytes = std::fs::read(path)
        .with_context(|| format!("Task file '{}' could not be read", path.display()))?;
    let parsed: GoldenTasksFile = serde_json::from_slice(&bytes)
        .with_context(|| format!("Task file '{}' could not be parsed", path.display()))?;
    let total = parsed.tasks.len();
    let tasks = parsed
        .tasks
        .into_iter()
        .filter(|task| task.query.kind == SEARCH_CODE_KIND)
        .map(eval_task)
        .collect::<Result<Vec<EvalTask>>>()?;
    Ok(LoadedTaskFile {
        summary: TaskFileSummary {
            path: path.display().to_string(),
            sha256: hex::encode(Sha256::digest(&bytes)),
            tasks: tasks.len(),
            skipped: total - tasks.len(),
        },
        tasks,
    })
}

fn eval_task(task: Task) -> Result<EvalTask> {
    let Task {
        id,
        repo,
        query,
        expected,
        split,
        ..
    } = task;
    let query_text = query
        .text
        .filter(|text| !text.trim().is_empty())
        .ok_or_else(|| anyhow!("Task '{}' has no query text", id))?;
    let repo_root = normalize_repo_path(&repo.path)?;
    let gold = match (non_empty(expected.file_paths), non_empty(expected.node_ids)) {
        (Some(paths), None) => Gold::File(
            paths
                .iter()
                .map(|raw| normalize_path(&repo_root, raw))
                .collect(),
        ),
        (None, Some(ids)) => Gold::Node(
            ids.iter()
                .map(|raw| crate::normalize_node_id(&normalize_task_node_id(&repo_root, raw)))
                .collect(),
        ),
        (Some(_), Some(_)) => bail!(
            "Task '{}' defines both file and node gold; exactly one level is allowed",
            id
        ),
        (None, None) => bail!("Task '{}' defines no gold file paths or node ids", id),
    };
    Ok(EvalTask {
        id,
        project: repo.name,
        repo_path: repo.path,
        repo_commit: repo.commit,
        split,
        query: query_text,
        gold,
    })
}

fn non_empty(values: Option<Vec<String>>) -> Option<Vec<String>> {
    values.filter(|items| !items.is_empty())
}

fn ensure_unique_task_ids(tasks: &[EvalTask]) -> Result<()> {
    let mut seen: HashSet<&str> = HashSet::new();
    for task in tasks {
        if !seen.insert(task.id.as_str()) {
            bail!("Task id '{}' appears more than once", task.id);
        }
    }
    Ok(())
}

/// Görevleri depo yoluna göre gruplar; projeler yol sırasında, görevler
/// dosyadaki sırasında kalır.
fn group_by_project(tasks: Vec<EvalTask>) -> Result<Vec<ProjectTasks>> {
    let grouped: BTreeMap<String, Vec<EvalTask>> =
        tasks.into_iter().fold(BTreeMap::new(), |mut groups, task| {
            groups
                .entry(task.repo_path.clone())
                .or_insert_with(Vec::new)
                .push(task);
            groups
        });
    grouped
        .into_iter()
        .map(|(repo_path, tasks)| project_tasks(repo_path, tasks))
        .collect()
}

fn project_tasks(repo_path: String, tasks: Vec<EvalTask>) -> Result<ProjectTasks> {
    let first = tasks
        .first()
        .ok_or_else(|| anyhow!("Repo path '{}' has no tasks", repo_path))?;
    if let Some(other) = tasks
        .iter()
        .find(|task| task.project != first.project || task.repo_commit != first.repo_commit)
    {
        bail!(
            "Tasks '{}' and '{}' share repo path '{}' but differ in repo name or commit",
            first.id,
            other.id,
            repo_path
        );
    }
    let name = first.project.clone();
    let repo_commit = first.repo_commit.clone();
    Ok(ProjectTasks {
        name,
        repo_path,
        repo_commit,
        tasks,
    })
}

/// Projenin etkin indeksini doğrular, motoru açar, her yöntemi bir kez
/// ölçmeden ısıtır ve görevleri yöntemler arasında dönüşümlü ölçer.
async fn evaluate_project(
    project: &ProjectTasks,
    source: &EmbeddingSource,
    settings: &RetrievalEvalSettings,
) -> Result<(ProjectSummary, Vec<Measurement>)> {
    let artifacts = crate::resolve_index_artifacts(&project.repo_path, None)?;
    if !artifacts.graph_path.exists() {
        bail!(
            "Project '{}' has no index graph at '{}'; run `ccm-cli index --path {}` first",
            project.name,
            artifacts.graph_path.display(),
            project.repo_path
        );
    }
    let vector_table = artifacts.db_path.join("code_vectors.lance");
    if !vector_table.exists() {
        bail!(
            "Project '{}' has no vector table at '{}'",
            project.name,
            vector_table.display()
        );
    }
    let embedding = crate::read_index_embedding(&artifacts.manifest_path)?.ok_or_else(|| {
        anyhow!(
            "Index manifest '{}' records no embedding identity",
            artifacts.manifest_path.display()
        )
    })?;
    if let Some(mismatch) = source.mismatch_with(Some(&embedding)) {
        return Err(anyhow::Error::new(mismatch).context(format!(
            "Project '{}' index cannot be searched with the configured embedder",
            project.name
        )));
    }
    let indexed_commit = read_indexed_commit(&artifacts.manifest_path)?;
    if let Some(expected) = &project.repo_commit {
        if indexed_commit.as_deref() != Some(expected.as_str()) {
            bail!(
                "Project '{}' index was built at commit {:?} but its tasks expect {}",
                project.name,
                indexed_commit,
                expected
            );
        }
    }

    let graph = CodeGraph::from_file(&artifacts.graph_path.to_string_lossy())?;
    let summary = ProjectSummary {
        name: project.name.clone(),
        path: project.repo_path.clone(),
        repo_commit: project.repo_commit.clone(),
        generation_id: artifacts.generation_id.clone(),
        indexed_commit,
        embedding,
        node_count: graph.graph.node_count(),
        edge_count: graph.graph.edge_count(),
    };
    let store = LanceDbStore::new(&artifacts.db_path.to_string_lossy(), "code_vectors").await?;
    let mut engine = RetrievalEngine::with_policy(
        Arc::new(RwLock::new(graph)),
        store,
        RetrievalPolicy::baseline(),
    );

    // Model yüklemesi ve tablo açılışı gecikmeye girmesin diye her yöntem bir
    // kez ölçülmeden çalışır.
    let warm_up_query = &project
        .tasks
        .first()
        .ok_or_else(|| anyhow!("Project '{}' has no tasks", project.name))?
        .query;
    for method in &settings.methods {
        engine.active_policy = method_policy(*method, settings.propagation_weight);
        engine
            .search_code_hybrid(warm_up_query, settings.limit)
            .await
            .with_context(|| {
                format!(
                    "Warm-up search failed for project '{}' with method {}",
                    project.name,
                    method.as_str()
                )
            })?;
    }

    let mut measurements = Vec::with_capacity(project.tasks.len() * settings.methods.len());
    for task in &project.tasks {
        for method in &settings.methods {
            engine.active_policy = method_policy(*method, settings.propagation_weight);
            let outcome = measure_task(&engine, task, *method, settings).await?;
            measurements.push(Measurement {
                method: *method,
                outcome,
            });
        }
    }
    Ok((summary, measurements))
}

/// Üretim baseline'ı; yalnızca sıralama yöntemi ve λ değişir, ortam
/// değişkeni override'ları uygulanmaz.
fn method_policy(method: RankingMethod, propagation_weight: f32) -> RetrievalPolicy {
    RetrievalPolicy {
        ranking: method,
        propagation_weight,
        ..RetrievalPolicy::baseline()
    }
}

async fn measure_task(
    engine: &RetrievalEngine,
    task: &EvalTask,
    method: RankingMethod,
    settings: &RetrievalEvalSettings,
) -> Result<TaskOutcome> {
    let mut runs: Vec<TimedRun> = Vec::with_capacity(settings.repeats);
    for _ in 0..settings.repeats {
        let started = Instant::now();
        let results = engine
            .search_code_hybrid(&task.query, settings.limit)
            .await
            .with_context(|| {
                format!(
                    "Search failed for task '{}' with method {}",
                    task.id,
                    method.as_str()
                )
            })?;
        runs.push(TimedRun {
            latency_ms: started.elapsed().as_secs_f64() * 1000.0,
            results,
        });
    }

    let first = runs
        .first()
        .ok_or_else(|| anyhow!("Task '{}' was not measured", task.id))?;
    let top_results = ranked_results(&first.results, method, &task.id)?;
    for (repeat, run) in runs.iter().enumerate().skip(1) {
        let repeated = ranked_results(&run.results, method, &task.id)?;
        if !same_ranking(&repeated, &top_results) {
            bail!(
                "Task '{}' with method {} returned a different ranking on repeat {}; the ranking is not deterministic",
                task.id,
                method.as_str(),
                repeat + 1
            );
        }
    }

    Ok(TaskOutcome {
        id: task.id.clone(),
        project: task.project.clone(),
        split: task.split.clone(),
        gold: task.gold.clone(),
        rank: first_gold_rank(&top_results, &task.gold),
        top_results,
        latencies_ms: runs.iter().map(|run| run.latency_ms).collect(),
    })
}

/// Sonuçları rapor biçimine çevirir; boş sonuç, kimliksiz düğüm ya da yönteme
/// ait olmayan gerekçe motorun başka bir arama yoluna düştüğünü gösterir.
fn ranked_results(
    results: &[ContextSuggestion],
    method: RankingMethod,
    task_id: &str,
) -> Result<Vec<RankedResult>> {
    if results.is_empty() {
        bail!(
            "Task '{}' returned no results with method {}; the semantic search produced no seeds",
            task_id,
            method.as_str()
        );
    }
    results
        .iter()
        .map(|result| {
            let node_id = result
                .node_id
                .clone()
                .ok_or_else(|| anyhow!("Task '{}' result '{}' has no node id", task_id, result.title))?;
            if !reason_matches_method(&result.reason, method) {
                bail!(
                    "Task '{}' result '{}' has reason '{}' that ranking method {} does not produce; the engine left the semantic search path",
                    task_id,
                    node_id,
                    result.reason,
                    method.as_str()
                );
            }
            Ok(RankedResult {
                file_path: node_id_to_file_path(&node_id),
                node_id,
                score: result.relevance_score,
                reason: result.reason.clone(),
            })
        })
        .collect()
}

fn same_ranking(left: &[RankedResult], right: &[RankedResult]) -> bool {
    left.len() == right.len() && left.iter().zip(right).all(|(a, b)| a.node_id == b.node_id)
}

fn reason_matches_method(reason: &str, method: RankingMethod) -> bool {
    match method {
        RankingMethod::Semantic => reason.starts_with("Semantic ("),
        RankingMethod::Propagation => reason.starts_with("Propagation ("),
        RankingMethod::Hybrid => HYBRID_REASON_PREFIXES.iter().any(|prefix| {
            reason
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with("Hybrid ("))
        }),
    }
}

fn first_gold_rank(results: &[RankedResult], gold: &Gold) -> Option<usize> {
    results
        .iter()
        .position(|result| matches_gold(result, gold))
        .map(|index| index + 1)
}

fn matches_gold(result: &RankedResult, gold: &Gold) -> bool {
    match gold {
        Gold::File(paths) => paths.contains(&result.file_path),
        Gold::Node(ids) => ids.contains(&crate::normalize_node_id(&result.node_id)),
    }
}

fn method_report(method: RankingMethod, measurements: &[Measurement]) -> Result<MethodReport> {
    let tasks: Vec<TaskOutcome> = measurements
        .iter()
        .filter(|measurement| measurement.method == method)
        .map(|measurement| measurement.outcome.clone())
        .collect();
    let metrics = method_metrics(&tasks).with_context(|| {
        format!(
            "Metrics of method {} could not be computed",
            method.as_str()
        )
    })?;
    Ok(MethodReport {
        method,
        metrics,
        tasks,
    })
}

fn method_metrics(tasks: &[TaskOutcome]) -> Result<MethodMetrics> {
    let ranks: Vec<Option<usize>> = tasks.iter().map(|task| task.rank).collect();
    let latencies: Vec<f64> = tasks
        .iter()
        .flat_map(|task| task.latencies_ms.iter().copied())
        .collect();
    Ok(MethodMetrics {
        tasks: tasks.len(),
        recall_at_1: recall_at(&ranks, 1)?,
        recall_at_5: recall_at(&ranks, 5)?,
        recall_at_10: recall_at(&ranks, 10)?,
        mrr_at_10: mrr_at(&ranks, 10)?,
        latency_p50_ms: percentile_nearest_rank(&latencies, 50)?,
        latency_p95_ms: percentile_nearest_rank(&latencies, 95)?,
    })
}

fn read_indexed_commit(manifest_path: &Path) -> Result<Option<String>> {
    #[derive(Deserialize)]
    struct ManifestCommit {
        #[serde(default)]
        indexed_commit: Option<String>,
    }
    let bytes = std::fs::read(manifest_path).with_context(|| {
        format!(
            "Index manifest '{}' could not be read",
            manifest_path.display()
        )
    })?;
    let parsed: ManifestCommit = serde_json::from_slice(&bytes).with_context(|| {
        format!(
            "Index manifest '{}' could not be parsed",
            manifest_path.display()
        )
    })?;
    Ok(parsed.indexed_commit)
}

fn current_hardware() -> Hardware {
    Hardware {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        logical_cpus: num_cpus::get(),
        physical_cpus: num_cpus::get_physical(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn assert_close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
    }

    fn result(node_id: &str) -> RankedResult {
        RankedResult {
            node_id: node_id.to_string(),
            file_path: node_id_to_file_path(node_id),
            score: 0.5,
            reason: "Semantic (semantic 0.50)".to_string(),
        }
    }

    fn valid_settings() -> RetrievalEvalSettings {
        RetrievalEvalSettings {
            task_files: vec![PathBuf::from("tasks.json")],
            methods: vec![RankingMethod::Semantic, RankingMethod::Hybrid],
            limit: 10,
            propagation_weight: 0.3,
            repeats: 3,
            commit: "4e3a55e".to_string(),
        }
    }

    fn task(expected: serde_json::Value) -> Task {
        serde_json::from_value(json!({
            "id": "t1",
            "repo": {"name": "demo", "path": "corpus/demo", "commit": "abc1234", "languages": null},
            "query": {"type": "search_code", "text": "parse config", "node_id": null,
                      "file_path": null, "line": null, "column": null},
            "expected": expected,
            "tags": null,
            "priority": null,
            "notes": null
        }))
        .expect("task json")
    }

    /// Sıralar 1, 3, yok, 12, 2: R@1 = 1/5, R@5 = R@10 = 3/5,
    /// MRR@10 = (1 + 1/3 + 0 + 0 + 1/2) / 5 = 0.366667.
    #[test]
    fn recall_and_mrr_match_hand_computed_example() {
        let ranks = [Some(1), Some(3), None, Some(12), Some(2)];
        assert_close(recall_at(&ranks, 1).unwrap(), 0.2);
        assert_close(recall_at(&ranks, 5).unwrap(), 0.6);
        assert_close(recall_at(&ranks, 10).unwrap(), 0.6);
        assert_close(mrr_at(&ranks, 10).unwrap(), 0.366_667);
        assert!(recall_at(&[], 1).is_err());
        assert!(mrr_at(&[], 10).is_err());
    }

    /// En yakın sıra: n = 5 için p50 3., p95 5. eleman; n = 20 için p50 10.,
    /// p95 19. eleman.
    #[test]
    fn nearest_rank_percentiles_match_hand_computed_examples() {
        let five = [5.0, 1.0, 4.0, 2.0, 3.0];
        assert_eq!(percentile_nearest_rank(&five, 50).unwrap(), 3.0);
        assert_eq!(percentile_nearest_rank(&five, 95).unwrap(), 5.0);
        let twenty: Vec<f64> = (1..=20).map(f64::from).collect();
        assert_eq!(percentile_nearest_rank(&twenty, 50).unwrap(), 10.0);
        assert_eq!(percentile_nearest_rank(&twenty, 95).unwrap(), 19.0);
        assert!(percentile_nearest_rank(&[], 50).is_err());
        assert!(percentile_nearest_rank(&five, 0).is_err());
        assert!(percentile_nearest_rank(&five, 101).is_err());
    }

    #[test]
    fn gold_rank_is_first_matching_file_or_node() {
        let results = vec![
            result("./src/a.py:function:1:0"),
            result("./src/b.py:function:3:0"),
            result("./src/b.py:class:9:0"),
        ];
        let file_gold = Gold::File(vec!["./src/b.py".to_string()]);
        let node_gold = Gold::Node(vec!["./src/b.py:class:9:0".to_string()]);
        let missing = Gold::File(vec!["./src/c.py".to_string()]);
        assert_eq!(first_gold_rank(&results, &file_gold), Some(2));
        assert_eq!(first_gold_rank(&results, &node_gold), Some(3));
        assert_eq!(first_gold_rank(&results, &missing), None);
    }

    #[test]
    fn reasons_identify_the_ranking_method() {
        assert!(reason_matches_method(
            "Semantic (semantic 0.71)",
            RankingMethod::Semantic
        ));
        assert!(reason_matches_method(
            "Propagation (semantic 0.70, support 0.40, weight 0.30)",
            RankingMethod::Propagation
        ));
        assert!(reason_matches_method(
            "Hybrid (graph 1.00, semantic 0.70, path 0.00, conf 0.62)",
            RankingMethod::Hybrid
        ));
        assert!(reason_matches_method(
            "LowConf Hybrid (semantic 0.70, path 0.00, conf 0.40)",
            RankingMethod::Hybrid
        ));
        assert!(!reason_matches_method(
            "Ranked lexical graph match (12)",
            RankingMethod::Hybrid
        ));
        assert!(!reason_matches_method(
            "Hybrid (semantic 0.70, path 0.00, conf 0.62)",
            RankingMethod::Semantic
        ));
    }

    #[test]
    fn settings_validation_rejects_unusable_values() {
        let valid = valid_settings();
        assert!(validate_settings(&valid).is_ok());
        let invalid = [
            RetrievalEvalSettings {
                limit: 5,
                ..valid.clone()
            },
            RetrievalEvalSettings {
                repeats: 0,
                ..valid.clone()
            },
            RetrievalEvalSettings {
                propagation_weight: 1.5,
                ..valid.clone()
            },
            RetrievalEvalSettings {
                propagation_weight: f32::NAN,
                ..valid.clone()
            },
            RetrievalEvalSettings {
                commit: "4E3A55E".to_string(),
                ..valid.clone()
            },
            RetrievalEvalSettings {
                methods: vec![RankingMethod::Hybrid, RankingMethod::Hybrid],
                ..valid.clone()
            },
            RetrievalEvalSettings {
                task_files: Vec::new(),
                ..valid.clone()
            },
        ];
        for settings in invalid {
            assert!(validate_settings(&settings).is_err(), "{settings:?}");
        }
    }

    #[test]
    fn task_gold_needs_exactly_one_level() {
        let file_task = eval_task(task(json!({
            "node_ids": null, "file_paths": ["src/app.py"], "min_recall": 1,
            "max_rank": null, "reason_contains": null
        })))
        .expect("file gold");
        assert_eq!(file_task.gold, Gold::File(vec!["./src/app.py".to_string()]));
        assert_eq!(file_task.query, "parse config");

        let node_task = eval_task(task(json!({
            "node_ids": ["src/app.py:function:12:0"], "file_paths": [], "min_recall": 1,
            "max_rank": null, "reason_contains": null
        })))
        .expect("node gold");
        assert_eq!(
            node_task.gold,
            Gold::Node(vec!["./src/app.py:function:12:0".to_string()])
        );

        let both = task(json!({
            "node_ids": ["src/app.py:function:12:0"], "file_paths": ["src/app.py"],
            "min_recall": 1, "max_rank": null, "reason_contains": null
        }));
        assert!(eval_task(both).is_err());
        let neither = task(json!({
            "node_ids": null, "file_paths": null, "min_recall": 1,
            "max_rank": null, "reason_contains": null
        }));
        assert!(eval_task(neither).is_err());
    }
}
