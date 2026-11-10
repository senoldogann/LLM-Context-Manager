//! MCP Tool implementations that bridge to ccm-core.

use anyhow::Result;
use petgraph::visit::EdgeRef;
use serde_json::Value;
use std::collections::HashSet;
use std::path::{Component, Path};
use std::sync::Arc;

use crate::protocol::{ToolResult, ToolResultContent};
use ccm_core::engine::{RetrievalEngine, TargetForm};

/// İstemci girdisinden kaynaklanan araç argüman hatası. MCP 2025-11-25 gereği
/// araç yürütme hatası (`isError: true`) olarak döner; model argümanı düzeltip
/// yeniden deneyebilir.
#[derive(Debug)]
pub struct ToolInputError(pub String);

impl std::fmt::Display for ToolInputError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for ToolInputError {}

pub(crate) fn input_error(message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(ToolInputError(message.into()))
}

const DEFAULT_MAX_LIMIT: usize = 50;
/// Varsayılan cevap bütçesi (yaklaşık token; bir token ≈ dört karakter).
pub(crate) const DEFAULT_MAX_TOKENS: usize = 1_500;
/// `map` için varsayılan bütçe: bir oturumun başında tek seferlik genel bakış.
pub(crate) const DEFAULT_MAP_TOKENS: usize = 1_000;
const MAX_MAX_TOKENS: usize = 20_000;
const CHARS_PER_TOKEN: usize = 4;
const MAX_GRAPH_DEPTH: usize = 32;
/// Receiver evidence en çok bu kadar caller döndürür; sınıra ulaşan liste tam sayılmaz.
const RECEIVER_EVIDENCE_LIMIT: usize = 20;
const MAX_DIFF_DAYS: u32 = 3_650;

/// Tek satırlık sonuç: `- Tür: ad · yol:başlangıç-bitiş · neden`.
fn suggestion_line(suggestion: &ccm_core::engine::ContextSuggestion) -> String {
    let mut line = format!("- {}", suggestion.title);
    if let Some(path) = &suggestion.file_path {
        let path = path.trim_start_matches("./");
        match (suggestion.start_line, suggestion.end_line) {
            (Some(start), Some(end)) => line.push_str(&format!(" · {path}:{start}-{end}")),
            (Some(start), None) => line.push_str(&format!(" · {path}:{start}")),
            _ => line.push_str(&format!(" · {path}")),
        }
    }
    if !suggestion.reason.is_empty() {
        line.push_str(" · ");
        line.push_str(&suggestion.reason);
    }
    line
}

/// Bütçenin birimi karakterdir (bir token ≈ dört karakter), bayt değil: çok
/// baytlı metin (`·`, CJK yorumlar) bütçeyi erken tüketmemeli.
fn char_count(text: &str) -> usize {
    text.chars().count()
}

/// Sonuçları bütçe içinde tek satır olarak listeler. Gövdeler istenirse bütçeye
/// dahildir ve sığmayan kısmı kesilir; sığmayan sonuçlar sonda sayılır.
fn format_suggestions_output(
    suggestions: &[ccm_core::engine::ContextSuggestion],
    include_body: bool,
    list_tokens: usize,
    requested_tokens: usize,
) -> String {
    let budget = list_tokens.saturating_mul(CHARS_PER_TOKEN);
    let mut output = String::new();
    let mut used = 0;
    for (index, suggestion) in suggestions.iter().enumerate() {
        let line = suggestion_line(suggestion);
        let line_chars = char_count(&line) + 1;
        if used + line_chars > budget {
            output.push_str(&omitted_footer(suggestions.len() - index, requested_tokens));
            return output;
        }
        output.push_str(&line);
        output.push('\n');
        used += line_chars;
        if include_body && !suggestion.content.is_empty() {
            let body = clip(&suggestion.content, budget.saturating_sub(used + 8));
            if !body.is_empty() {
                output.push_str("```\n");
                output.push_str(&body);
                output.push_str("\n```\n");
                used += char_count(&body) + 8;
            }
        }
    }
    output
}

/// Bütçeye sığmayan sonuç sayısı ve nasıl genişletileceği.
fn omitted_footer(count: usize, max_tokens: usize) -> String {
    format!(
        "… {count} more not shown (max_tokens={max_tokens}); narrow the query or raise max_tokens.\n"
    )
}

/// Metni `limit` karaktere keser; kesilirse sonuna `…` ekler.
fn clip(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut clipped: String = text.chars().take(limit.saturating_sub(1)).collect();
    if !clipped.is_empty() {
        clipped.push('…');
    }
    clipped
}

fn limit_from_args(args: &Value, default: usize) -> usize {
    args.get("limit")
        .and_then(|v| v.as_u64())
        .map(|v| (v as usize).clamp(1, DEFAULT_MAX_LIMIT))
        .unwrap_or(default)
}

/// Cevap bütçesi: `max_tokens`; yoksa eski `max_chars` / 4; yoksa varsayılan.
fn max_tokens_from_args(args: &Value) -> Result<usize> {
    Ok(requested_tokens(args)?.unwrap_or(DEFAULT_MAX_TOKENS))
}

/// İstenen bütçe: `max_tokens` ya da eski `max_chars` / 4, en çok
/// `MAX_MAX_TOKENS`; ikisi de yoksa `None`. Pozitif tam sayı olmayan değer
/// varsayılana düşmez, girdi hatası olur.
fn requested_tokens(args: &Value) -> Result<Option<usize>> {
    if let Some(value) = present_argument(args, "max_tokens") {
        return Ok(Some(
            positive_integer(value, "max_tokens")?.min(MAX_MAX_TOKENS),
        ));
    }
    match present_argument(args, "max_chars") {
        Some(value) => Ok(Some(
            (positive_integer(value, "max_chars")? / CHARS_PER_TOKEN).clamp(1, MAX_MAX_TOKENS),
        )),
        None => Ok(None),
    }
}

/// Argüman verilmiş mi; `null` verilmemiş sayılır.
fn present_argument<'a>(args: &'a Value, key: &str) -> Option<&'a Value> {
    args.get(key).filter(|value| !value.is_null())
}

/// Pozitif tam sayı argümanı; metin, sıfır ya da negatif değer hatadır.
fn positive_integer(value: &Value, key: &str) -> Result<usize> {
    value
        .as_u64()
        .filter(|number| *number > 0)
        .and_then(|number| usize::try_from(number).ok())
        .ok_or_else(|| input_error(format!("'{key}' must be a positive integer, got {value}")))
}

fn include_body_from_args(args: &Value) -> bool {
    args.get("include_body")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
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
                max_tokens_from_args(args)?,
                max_tokens_from_args(args)?,
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
                max_tokens_from_args(args)?,
                max_tokens_from_args(args)?,
            ),
        }],
        is_error: None,
    })
}

/// Helper: Normalizes a file path to match graph conventions (relative, starts with ./)
fn normalize_graph_path(path_str: &str, project_path: Option<&str>) -> Result<String> {
    if path_str.is_empty() {
        return Err(input_error("Path argument cannot be empty"));
    }

    let normalized_input = path_str.replace('\\', "/");
    if normalized_input.contains('\0') {
        return Err(input_error("Path argument contains invalid null byte"));
    }

    let path = Path::new(&normalized_input);
    if path.is_absolute() {
        let root = project_path.ok_or_else(|| {
            input_error("Absolute paths require a matching 'project_path' argument")
        })?;
        let stripped = path
            .strip_prefix(Path::new(root))
            .map_err(|_| input_error("Path is outside the provided project root"))?;
        return normalize_relative_graph_path(stripped);
    }

    normalize_relative_graph_path(path)
}

/// Hedef argümanını (`key`, boşsa eski `legacy_key`) biçimine göre ayırır. İlk
/// boşluksuz parça `yol:satır` ya da `yol:başlangıç-bitiş` ise konumdur; böylece
/// sonuç satırlarındaki ve aday listelerindeki konumlar olduğu gibi geri
/// verilebilir. `:symbol:` taşıyan ya da en az üç `:` içeren değer düğüm
/// kimliğidir; diğer her şey sembol ya da dosya adıdır (`Sınıf.üye`, `app/core.py`).
fn target_form(args: &Value, key: &str, legacy_key: &str) -> Result<TargetForm> {
    let raw = [key, legacy_key]
        .iter()
        .filter_map(|name| args.get(*name).and_then(Value::as_str))
        .map(str::trim)
        .find(|value| !value.is_empty())
        .ok_or_else(|| input_error(format!("Missing '{key}' argument")))?;
    let project_path = args.get("project_path").and_then(|v| v.as_str());
    let token = raw.split_whitespace().next().unwrap_or(raw);
    if token.contains(":symbol:") || token.matches(':').count() >= 3 {
        return Ok(TargetForm::NodeId(normalize_graph_node_id(
            token,
            project_path,
        )?));
    }
    if let Some((path, range)) = token.rsplit_once(':') {
        if let Some(line) = range_start(range) {
            return Ok(TargetForm::Location {
                file: normalize_graph_path(path, project_path)?,
                line,
            });
        }
    }
    // `yol.rs:ad`, `yol.py:Sınıf.ad`, `yol.rs:Tip::ad`: yol kısmı `:` içermez ve bir
    // kaynak dosya uzantısıyla biter; `bound::with_bound` gibi yollar buraya düşmez.
    if let Some((path, name)) = token.split_once(':') {
        let name = name.trim_start_matches(':');
        if is_source_file_path(path)
            && !name.is_empty()
            && name
                .split([':', '.'])
                .filter(|part| !part.is_empty())
                .all(identifier_part)
        {
            return Ok(TargetForm::FileSymbol {
                file: normalize_graph_path(path, project_path)?,
                name: name.to_string(),
            });
        }
    }
    Ok(TargetForm::Name(raw.to_string()))
}

/// `13` ya da `13-17` biçimindeki aralığın başlangıç satırı.
fn range_start(range: &str) -> Option<usize> {
    let (start, end) = match range.split_once('-') {
        Some((start, end)) => (start, Some(end)),
        None => (range, None),
    };
    let start = start.parse::<usize>().ok()?;
    match end {
        Some(end) => end.parse::<usize>().ok().map(|_| start),
        None => Some(start),
    }
}

/// Ajana okunur bir hata metniyle `isError` sonucu.
fn tool_error(text: String) -> ToolResult {
    ToolResult {
        content: vec![ToolResultContent {
            content_type: "text".to_string(),
            text,
        }],
        is_error: Some(true),
    }
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
                return Err(input_error(
                    "Parent directory segments are not allowed in MCP path arguments",
                ));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(input_error(
                    "Absolute paths are not allowed in MCP path arguments",
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
    let mode = args
        .get("mode")
        .and_then(|v| v.as_str())
        .map(normalize_index_mode)
        .transpose()?
        .unwrap_or_default();
    let job_key = crate::server::project_key_for_path(project_path);

    let job = match claim_index_job(&state, project_path, &job_key, mode) {
        IndexJobClaim::Joined(_) => return Ok(index_in_progress_result(project_path)),
        IndexJobClaim::Collected(finished) => return Ok(collected_result(&finished)),
        IndexJobClaim::Started(job) => job,
        IndexJobClaim::Rejected(result) => return Ok(result),
    };

    let timeout_ms = std::env::var("CCM_INDEX_RESPONSE_TIMEOUT_MS").ok();
    let timeout_secs = std::env::var("CCM_INDEX_RESPONSE_TIMEOUT_SECS").ok();
    let wait_duration = index_response_timeout(timeout_ms.as_deref(), timeout_secs.as_deref());
    match tokio::time::timeout(wait_duration, wait_for_index_job(&state, &job_key, job)).await {
        Ok(result) => Ok(result),
        Err(_) => Ok(index_started_result(project_path)),
    }
}

/// Tool: index_now
/// İndekslemeyi arka plan işi olarak başlatır (ya da projede süren işe katılır)
/// ve sonucunu bekler. İş isteğe bağlı değildir: istek iptal edilirse yalnızca
/// bekleme durur; indeksleme (worker, etkinleştirme ve kilit bırakma) tamamlanır
/// ve sonucu sonraki `index_now`/`index_project` çağrısına kalır. Worker'ı yarıda
/// öldürmek staging kopyasını geride bırakırdı.
pub async fn index_now(state: Arc<crate::server::ServerState>, args: &Value) -> Result<ToolResult> {
    let project_path = args
        .get("project_path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'project_path' argument"))?;
    let mode = args
        .get("mode")
        .and_then(|v| v.as_str())
        .map(normalize_index_mode)
        .transpose()?
        .unwrap_or_default();
    let job_key = crate::server::project_key_for_path(project_path);

    match claim_index_job(&state, project_path, &job_key, mode) {
        IndexJobClaim::Joined(job) | IndexJobClaim::Started(job) => {
            Ok(wait_for_index_job(&state, &job_key, job).await)
        }
        IndexJobClaim::Collected(finished) => Ok(collected_result(&finished)),
        IndexJobClaim::Rejected(result) => Ok(result),
    }
}

/// Bitmiş indeks işinin saklanan sonucu ve bitiş anı.
#[derive(Debug, Clone)]
pub(crate) struct FinishedIndexJob {
    pub result: ToolResult,
    pub finished_at: std::time::Instant,
}

/// Projede indeksleme işi talebinin sonucu.
enum IndexJobClaim {
    /// Aynı modda süren işe katılındı.
    Joined(crate::server::IndexJob),
    /// Aynı modda bitmiş ama sonucu alınmamış işin sonucu alındı ve kayıt silindi.
    Collected(FinishedIndexJob),
    /// Yeni iş kaydedildi ve başlatıldı.
    Started(crate::server::IndexJob),
    /// Farklı modda iş sürüyor ya da sonucu alınmamış iş sınırı dolu.
    Rejected(ToolResult),
}

/// Projedeki mevcut işin durumu (karar için gereken kadarı).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExistingJob {
    None,
    Running(IndexModeArg),
    Finished(IndexModeArg),
}

/// Talebin kararı; saf fonksiyon `decide_claim` üretir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClaimDecision {
    Start,
    Join,
    Collect,
    /// Bitmiş işin sonucu farklı moda aittir: atılır ve yeni iş başlar.
    DiscardAndStart,
    RejectModeConflict(IndexModeArg),
}

/// Talep kararı. Farklı moddaki iş isteğe "yapıldı" diye dönmez: süren iş
/// varken açık hata, bitmiş iş varken o sonuç atılıp yeni iş başlar. Aynı
/// moddaki bitmiş işin sonucu alınır (iptal edilen `index_now` ve zaman aşımına
/// uğrayan `index_project` sonucu böyle teslim edilir) ve cevap bunun önceki
/// bir işin sonucu olduğunu söyler.
fn decide_claim(existing: ExistingJob, requested: IndexModeArg) -> ClaimDecision {
    match existing {
        ExistingJob::None => ClaimDecision::Start,
        ExistingJob::Running(mode) if mode == requested => ClaimDecision::Join,
        ExistingJob::Running(mode) => ClaimDecision::RejectModeConflict(mode),
        ExistingJob::Finished(mode) if mode == requested => ClaimDecision::Collect,
        ExistingJob::Finished(_) => ClaimDecision::DiscardAndStart,
    }
}

/// Alınan önceki iş sonucunu, kaç saniye önce bittiğini söyleyerek döndürür.
fn collected_result(finished: &FinishedIndexJob) -> ToolResult {
    let mut result = finished.result.clone();
    let note = format!(
        "Result of the index job that finished {}s ago (started by an earlier call). Call again to start a new index.\n\n",
        finished.finished_at.elapsed().as_secs()
    );
    match result.content.first_mut() {
        Some(first) => first.text = format!("{note}{}", first.text),
        None => result.content.push(ToolResultContent {
            content_type: "text".to_string(),
            text: note,
        }),
    }
    result
}

/// Projede süren işi döndürür ya da yeni iş kaydedip başlatır. Denetim ve kayıt
/// tek kilit altında yapılır: eşzamanlı iki çağrı aynı projede iki iş başlatmaz.
/// `job_key`, indeks kilidinin ve tazelik durumunun anahtarıyla aynı proje
/// anahtarıdır (`project_key_for_path`); farklı türetilirse kilit kaydı silinemez.
fn claim_index_job(
    state: &Arc<crate::server::ServerState>,
    project_path: &str,
    job_key: &str,
    mode: IndexModeArg,
) -> IndexJobClaim {
    const MAX_INDEX_JOBS: usize = 64;
    let mut jobs = state.index_jobs.lock().unwrap();
    let existing = match jobs.get(job_key) {
        None => ExistingJob::None,
        Some(job) if job.receiver.borrow().is_none() => ExistingJob::Running(job.mode),
        Some(job) => ExistingJob::Finished(job.mode),
    };
    match decide_claim(existing, mode) {
        ClaimDecision::Start => {}
        ClaimDecision::Join => {
            let job = jobs
                .get(job_key)
                .expect("running job checked above")
                .clone();
            return IndexJobClaim::Joined(job);
        }
        ClaimDecision::Collect => {
            let job = jobs.remove(job_key).expect("finished job checked above");
            let finished = job
                .receiver
                .borrow()
                .clone()
                .expect("finished job carries its result");
            return IndexJobClaim::Collected(finished);
        }
        ClaimDecision::DiscardAndStart => {
            let discarded = jobs.remove(job_key).expect("finished job checked above");
            tracing::info!(
                path = %project_path,
                job = discarded.id,
                previous_mode = ?discarded.mode,
                requested_mode = ?mode,
                "Discarding an uncollected index result of another mode; starting a new job"
            );
        }
        ClaimDecision::RejectModeConflict(running) => {
            return IndexJobClaim::Rejected(index_task_failed_result(
                project_path,
                &format!(
                    "a {running:?} index job is running for this project; wait for it (call again with mode {running:?}) before starting a {mode:?} index"
                ),
            ));
        }
    }
    if jobs.len() >= MAX_INDEX_JOBS {
        return IndexJobClaim::Rejected(index_task_failed_result(
            project_path,
            "too many index jobs are awaiting completion or result polling; poll existing jobs and retry",
        ));
    }
    let (sender, receiver) = tokio::sync::watch::channel(None);
    let job = crate::server::IndexJob {
        id: state
            .next_index_job_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        mode,
        receiver,
    };
    jobs.insert(job_key.to_string(), job.clone());
    drop(jobs);

    tracing::info!(path = %project_path, job = job.id, "Starting manual index");
    spawn_index_job(
        state.clone(),
        project_path.to_string(),
        job_key.to_string(),
        job.id,
        mode,
        sender,
    );
    IndexJobClaim::Started(job)
}

/// İndeksleme işini isteklerden bağımsız bir görevde çalıştırır. Sonuç, iş
/// haritasındaki alıcı sayesinde saklanır; bekleyen yoksa (ör. istek iptal
/// edildiyse) bir sonraki çağrı alır. Saklama süresi dolunca kayıt silinir.
fn spawn_index_job(
    state: Arc<crate::server::ServerState>,
    project_path: String,
    job_key: String,
    job_id: u64,
    mode: IndexModeArg,
    sender: tokio::sync::watch::Sender<Option<FinishedIndexJob>>,
) {
    tokio::spawn(async move {
        let worker_state = state.clone();
        let worker_path = project_path.clone();
        let worker =
            tokio::spawn(async move { run_index_project(worker_state, &worker_path, mode).await });
        let result = match worker.await {
            Ok(result) => result,
            Err(error) => index_task_failed_result(&project_path, &error.to_string()),
        };
        let finished = FinishedIndexJob {
            result,
            finished_at: std::time::Instant::now(),
        };
        if sender.send(Some(finished)).is_err() {
            tracing::warn!(
                path = %project_path,
                job = job_id,
                "Index job finished after its record was removed; the result was dropped"
            );
        }
        state.release_index_lock(&job_key);
        tokio::time::sleep(index_result_retention()).await;
        state.remove_index_job_if_id(&job_key, job_id);
    });
}

/// İşin sonucunu bekler ve sonucu alan çağrı olarak kaydı siler. Bekleyen
/// isteğin iptali yalnızca bu beklemeyi bırakır; iş sürer.
async fn wait_for_index_job(
    state: &crate::server::ServerState,
    job_key: &str,
    job: crate::server::IndexJob,
) -> ToolResult {
    let mut receiver = job.receiver;
    loop {
        let finished = receiver.borrow_and_update().clone();
        if let Some(finished) = finished {
            state.remove_index_job_if_id(job_key, job.id);
            return finished.result;
        }
        if receiver.changed().await.is_err() {
            return index_task_failed_result(job_key, "index job completed without a result");
        }
    }
}

/// Arka plan quick index sonrası semantic upgrade'i planlar. Embedding uzun
/// sürdüğü için yanıt vermez; okuma araçları graph-only sonuçla çalışmaya
/// devam eder.
///
/// Durable tasarım: upgrade, MCP sürecinin ömründen bağımsız ayrı bir OS
/// sürecinde (detached process group) çalıştırılır. MCP process'i (örneğin
/// editor yeniden başlatıldığında) upgrade ortasında kapansa bile iş tamamlanır
/// ve aktif generation atomik şekilde vektörlü hale gelir. Upgrade yarıda kalırsa
/// bile `update_index`'in self-repair yolu (`vector_table_required && unhealthy`)
/// sonraki `index_project` çağrısında eksik vektörleri onarır.
pub(crate) fn schedule_semantic_upgrade(
    state: Arc<crate::server::ServerState>,
    project_path: std::sync::Arc<str>,
    db_path: String,
    kind: crate::server::SemanticUpgradeKind,
) {
    // Hızlı indeks yükseltmesi bitene kadar otomatik yenileme ertelenir; model
    // değişikliği yükseltmesi graf yenilemelerini durdurmaz (bkz.
    // `SemanticUpgradeKind`).
    let project_key = crate::server::project_key_for_path(&project_path);
    state.begin_semantic_upgrade(&project_key, kind);
    // Detached worker: MCP çıkışında ölmeyen, kendi process grubunda koşan süreç.
    // Yalnızca iş tamamlandığında (süreç hâlâ yaşıyorsa) engine cache tazelenir.
    tokio::spawn(async move {
        let refresh_state = state.clone();
        let refresh_path = project_path.clone();
        match spawn_detached_upgrade_worker(&project_path, &db_path).await {
            Ok(stats) => {
                tracing::info!(
                    nodes = stats.nodes_created,
                    "Background semantic upgrade completed"
                );
                // Yeni generation graph+vektör içerdiğinden cache'i tazele.
                let _ = refresh_state.refresh_project_engine(&refresh_path).await;
            }
            Err(error) => {
                tracing::warn!(error = %error, "Background semantic upgrade failed");
            }
        }
        refresh_state.end_semantic_upgrade(&project_key, kind);
    });
}

/// MCP sürecinden bağımsız, detached bir upgrade worker süreci başlatır ve
/// tamamlanmasını bekler. Süreç kendi process grubunda koştuğu ve `kill_on_drop`
/// edilmediği için MCP çıkışında işi bırakmaz. Sonucu (IndexStats) stdout'tan
/// JSON olarak okur; stderr projeye özel bir log dosyasına yönlendirilir.
async fn spawn_detached_upgrade_worker(
    project_path: &str,
    db_path: &str,
) -> Result<ccm_core::IndexStats> {
    let executable = std::env::current_exe()?;
    let project = project_path.to_string();
    let db = db_path.to_string();
    tokio::task::spawn_blocking(move || -> anyhow::Result<ccm_core::IndexStats> {
        let mut command = std::process::Command::new(&executable);
        command
            .arg("--ccm-internal-index-worker")
            .arg(&project)
            .arg(&db)
            .arg("upgrade")
            .env("CCM_INTERNAL_INDEX_WORKER", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(semantic_upgrade_log(&project));

        // Detached: süreci kendi process grubuna al; terminal/ebeveyn HUP'ından
        // etkilenmesin ve MCP çıkışında devam edebilsin.
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }

        let mut child = command.spawn()?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("upgrade worker stdout is unavailable"))?;
        let mut output = String::new();
        use std::io::Read;
        let mut reader = std::io::BufReader::new(stdout);
        reader
            .read_to_string(&mut output)
            .map_err(|error| anyhow::anyhow!("failed reading upgrade worker stdout: {}", error))?;
        let status = child.wait()?;
        if !status.success() {
            anyhow::bail!(
                "upgrade worker exited with status {}; see {}/.ccm/semantic-upgrade.log",
                status,
                project
            );
        }
        serde_json::from_str(&output).map_err(|error| {
            anyhow::anyhow!(
                "upgrade worker returned invalid JSON: {} (stdout: {})",
                error,
                output.chars().take(1_000).collect::<String>()
            )
        })
    })
    .await?
}

/// Worker stderr'ini projeye özel bir log dosyasına yönlendirir. Dosya açılamazsa
/// null'a düşer; upgrade akışı bozulmaz.
fn semantic_upgrade_log(project_path: &str) -> std::process::Stdio {
    let base = std::path::PathBuf::from(project_path);
    let log_dir = base.join(".ccm");
    let _ = std::fs::create_dir_all(&log_dir);
    let log_path = log_dir.join("semantic-upgrade.log");
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
    {
        Ok(file) => std::process::Stdio::from(file),
        Err(_) => std::process::Stdio::null(),
    }
}

/// `index_project`/`index_now` mod değerini normalize eder.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum IndexModeArg {
    #[default]
    Full,
    Quick,
    Upgrade,
}

fn normalize_index_mode(value: &str) -> Result<IndexModeArg> {
    match value.to_ascii_lowercase().as_str() {
        "full" | "default" => Ok(IndexModeArg::Full),
        "quick" | "graph" => Ok(IndexModeArg::Quick),
        "upgrade" | "semantic" => Ok(IndexModeArg::Upgrade),
        other => Err(input_error(format!(
            "Unknown index mode '{}'; expected 'full', 'quick', or 'upgrade'",
            other
        ))),
    }
}

fn index_result_retention() -> std::time::Duration {
    const DEFAULT_SECS: u64 = 10 * 60;
    const MAX_SECS: u64 = 60 * 60;
    let seconds = std::env::var("CCM_INDEX_RESULT_RETENTION_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_SECS)
        .min(MAX_SECS);
    std::time::Duration::from_secs(seconds)
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
                "Project indexing started in the background for {}. Call index_project again to check status. Read tools keep answering from the current index meanwhile; a project without an index becomes readable when indexing completes.",
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

async fn run_index_project(
    state: Arc<crate::server::ServerState>,
    project_path: &str,
    mode: IndexModeArg,
) -> ToolResult {
    let lock = state.project_index_lock(project_path);
    let _guard = lock.lock().await;

    let db_path = match state.project_db_path(project_path) {
        Ok(path) => path.to_string_lossy().to_string(),
        Err(error) => {
            return ToolResult {
                content: vec![ToolResultContent {
                    content_type: "text".to_string(),
                    text: format!("Project path could not be resolved safely: {}", error),
                }],
                is_error: Some(true),
            }
        }
    };

    match run_index_worker_process(project_path, &db_path, mode).await {
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

            let project_key = crate::server::project_key_for_path(project_path);
            state.ensure_auto_refresh(&project_key);
            state.request_refresh(&project_key);

            // Quick mod ilk yanıtı graph-only döndürür; embedding yüklendiğinde
            // arka planda aynı projede semantic upgrade başlatılır.
            if mode == IndexModeArg::Quick {
                schedule_semantic_upgrade(
                    state.clone(),
                    project_path.to_string().into(),
                    db_path.clone(),
                    crate::server::SemanticUpgradeKind::QuickIndex,
                );
            }

            ToolResult {
                content: vec![ToolResultContent {
                    content_type: "text".to_string(),
                    text: format_index_stats_result(stats, mode),
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

pub(crate) async fn run_index_worker_process(
    project_path: &str,
    db_path: &str,
    mode: IndexModeArg,
) -> Result<ccm_core::IndexStats> {
    let executable = std::env::current_exe()?;
    let mut command = tokio::process::Command::new(executable);
    let command = command
        .arg("--ccm-internal-index-worker")
        .arg(project_path)
        .arg(db_path);
    match mode {
        IndexModeArg::Quick => {
            command.arg("quick");
        }
        IndexModeArg::Upgrade => {
            command.arg("upgrade");
        }
        IndexModeArg::Full => {}
    }
    command
        .env("CCM_INTERNAL_INDEX_WORKER", "1")
        .kill_on_drop(true)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let child = command.spawn()?;
    let output = tokio::time::timeout(index_worker_timeout(), child.wait_with_output())
        .await
        .map_err(|_| anyhow::anyhow!("index execution exceeded the configured deadline"))??;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let detail = stderr.chars().take(4_000).collect::<String>();
        anyhow::bail!(
            "index worker exited with status {}: {}",
            output.status,
            detail.trim()
        );
    }
    serde_json::from_slice(&output.stdout).map_err(|error| {
        anyhow::anyhow!(
            "index worker returned invalid JSON: {} (stdout: {})",
            error,
            String::from_utf8_lossy(&output.stdout)
                .chars()
                .take(1_000)
                .collect::<String>()
        )
    })
}

fn format_index_stats_result(stats: ccm_core::IndexStats, mode: IndexModeArg) -> String {
    // Graf-yalnız aktivasyon ajana açıkça bildirilir; aksi halde search_code'un
    // yalnızca sözcüksel sonuç döndürmesinin nedeni görünmez kalır.
    let semantic_notice = stats.semantic_unavailable.as_ref().map(|reason| {
        format!(
            "Semantic search is unavailable: {}. Graph tools (map, explain, find_usages, impact_of_change, trace_call_chain) work normally; call index_project again once that is resolved.",
            reason
        )
    });
    if stats.files_indexed == 0
        && stats.files_failed == 0
        && stats.files_skipped == 0
        && stats.nodes_created == 0
    {
        let up_to_date = "No changes detected. Existing index is already up to date.".to_string();
        return match semantic_notice {
            Some(notice) => format!("{}\n\n{}", up_to_date, notice),
            None => up_to_date,
        };
    }

    let mut lines = vec![
        "Project index refreshed successfully.".to_string(),
        String::new(),
        "Stats:".to_string(),
        format!("- Files Indexed: {}", stats.files_indexed),
        format!("- Files Failed: {}", stats.files_failed),
        format!("- Files Skipped: {}", stats.files_skipped),
        format!("- Nodes Created: {}", stats.nodes_created),
    ];
    if stats.embedded_chunks + stats.reused_chunks > 0 {
        lines.push(format!(
            "- Chunks Embedded: {} (reused: {})",
            stats.embedded_chunks, stats.reused_chunks
        ));
    }

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

    if let Some(notice) = semantic_notice {
        lines.push(String::new());
        lines.push(notice);
    }

    lines.push(String::new());
    match mode {
        IndexModeArg::Quick => lines.push(
            "Quick (graph-only) index is active; semantic vectors are being built in the background."
                .to_string(),
        ),
        IndexModeArg::Upgrade => lines.push(
            "Semantic vectors are now available; hybrid search is fully enabled.".to_string(),
        ),
        IndexModeArg::Full => lines.push(
            "The project is ready for semantic search and graph navigation.".to_string(),
        ),
    }
    lines.join("\n")
}

fn index_worker_timeout() -> std::time::Duration {
    const DEFAULT_MILLIS: u64 = 2 * 60 * 60 * 1_000;
    const MAX_MILLIS: u64 = 24 * 60 * 60 * 1_000;
    let milliseconds = std::env::var("CCM_INDEX_EXECUTION_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .or_else(|| {
            std::env::var("CCM_INDEX_EXECUTION_TIMEOUT_SECS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .map(|seconds| seconds.saturating_mul(1_000))
        })
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MILLIS)
        .min(MAX_MILLIS);
    std::time::Duration::from_millis(milliseconds)
}

/// Tool: find_usages
/// Verilen node_id'yi çağıran / kullanan tüm node'ları döndürür.
pub async fn find_usages(engine: &Arc<RetrievalEngine>, args: &Value) -> Result<ToolResult> {
    let form = target_form(args, "target", "node_id")?;
    let normalized_id = match engine.resolve_target(form).await {
        Ok(id) => id,
        Err(error) => return Ok(tool_error(error.to_string())),
    };
    let limit = limit_from_args(args, 20);
    let report = match engine.find_usages(&normalized_id).await {
        Ok(report) => report,
        Err(error) => {
            return Ok(ToolResult {
                content: vec![ToolResultContent {
                    content_type: "text".to_string(),
                    text: error.to_string(),
                }],
                is_error: Some(true),
            });
        }
    };
    let shown: Vec<ccm_core::engine::ContextSuggestion> = report
        .usages
        .iter()
        .take(limit)
        .map(ccm_core::engine::usage_suggestion)
        .collect();
    let mut text = usage_summary(&report.target.name, &report.usages);
    if shown.len() < report.usages.len() {
        text.push_str(&format!(
            " (showing {} of {}; raise `limit` for more)",
            shown.len(),
            report.usages.len()
        ));
    }
    if !shown.is_empty() {
        // Özet satırı da bütçeden düşer; liste kalan bütçeyle biçimlenir.
        let list_budget = max_tokens_from_args(args)?
            .saturating_sub(char_count(&text) / CHARS_PER_TOKEN + 1)
            .max(1);
        text.push_str("\n\n");
        text.push_str(&format_suggestions_output(
            &shown,
            include_body_from_args(args),
            list_budget,
            max_tokens_from_args(args)?,
        ));
    }

    Ok(ToolResult {
        content: vec![ToolResultContent {
            content_type: "text".to_string(),
            text,
        }],
        is_error: None,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MemberCallHint {
    receiver: String,
    member: String,
    scope: Option<String>,
    definition_file: Option<String>,
}

fn identifier_part(text: &str) -> bool {
    let mut chars = text.chars();
    matches!(chars.next(), Some(first) if first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric())
}

fn backtick_spans(query: &str) -> Vec<&str> {
    query
        .split('\u{0060}')
        .enumerate()
        .filter_map(|(index, part)| (index % 2 == 1).then_some(part.trim()))
        .filter(|part| !part.is_empty())
        .collect()
}

/// Noktalı/qualified kod ve göreli yol ipuçlarını prose dilinden bağımsız ayırır.
/// Backtick zorunlu değildir; dosya erişimi yapılmaz, ipuçları indeksle doğrulanır.
fn code_spans(query: &str) -> Vec<&str> {
    let mut spans = backtick_spans(query);
    spans.extend(
        query
            .split(|ch: char| {
                !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '$' | '.' | ':' | '/' | '-'))
            })
            .map(|part| part.trim_end_matches(['.', ':']))
            .filter(|part| !part.is_empty()),
    );
    let mut seen = HashSet::new();
    spans.retain(|span| seen.insert(*span));
    spans
}

fn hint_paths(spans: &[&str]) -> (Option<String>, Option<String>) {
    let paths: Vec<&str> = spans
        .iter()
        .copied()
        .filter(|part| {
            part.contains('/')
                && !Path::new(part).is_absolute()
                && !Path::new(part)
                    .components()
                    .any(|component| matches!(component, Component::ParentDir))
        })
        .collect();
    let scope = paths
        .iter()
        .find(|part| part.ends_with('/'))
        .map(|part| part.trim_start_matches("./").to_string());
    let file = paths
        .iter()
        .find(|part| !part.ends_with('/') && Path::new(part).extension().is_some())
        .map(|part| part.trim_start_matches("./").to_string());
    (scope, file)
}

/// Bütün receiver.member adayları tanım kanıtıyla değerlendirilmeli; dışlanan
/// receiver'ın prose'da önce geçmesi onu hedef yapmaz.
fn member_call_hints(query: &str) -> Vec<MemberCallHint> {
    let spans = code_spans(query);
    let (scope, definition_file) = hint_paths(&spans);
    let mut hints = Vec::new();
    for part in spans {
        if part.contains('/') {
            continue;
        }
        let Some((receiver, member)) = part.rsplit_once('.') else {
            continue;
        };
        if identifier_part(member)
            && receiver.split('.').all(identifier_part)
            && !receiver.is_empty()
        {
            hints.push(MemberCallHint {
                receiver: receiver.to_string(),
                member: member.to_string(),
                scope: scope.clone(),
                definition_file: definition_file.clone(),
            });
        }
    }
    hints
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScopedSymbolHint {
    symbol: String,
    scope: Option<String>,
    /// Tanımın bulunabileceği dosyalar; Python modül yolu birden çok aday verir.
    definition_files: Vec<String>,
    /// Kod gibi yazılmış ad (`_`, `::`, iç büyük harf ya da backtick): yol kanıtı
    /// olmadan da projede tek tanıma çözülürse hedef olabilir. Düz İngilizce
    /// sözcükler (`get`, `run`) hiçbir zaman kapsamsız hedef olmaz.
    code_like: bool,
}

/// Ad kod gibi mi yazılmış: alt çizgi ya da iç büyük harf.
fn code_like_name(name: &str) -> bool {
    name.contains('_')
        || (name.chars().any(|c| c.is_ascii_lowercase())
            && name.chars().skip(1).any(|c| c.is_ascii_uppercase()))
}

/// `django.utils.html.escape` → (`escape`, olası tanım dosyaları).
fn python_module_hint(part: &str) -> Option<(String, Vec<String>)> {
    let segments: Vec<&str> = part.split('.').collect();
    if segments.len() < 2 || !segments.iter().all(|segment| identifier_part(segment)) {
        return None;
    }
    let (symbol, module) = segments.split_last()?;
    let module = module.join("/");
    let files = [
        format!("{module}.py"),
        format!("{module}/__init__.py"),
        format!("src/{module}.py"),
        format!("src/{module}/__init__.py"),
    ]
    .to_vec();
    Some((symbol.to_string(), files))
}

/// Açık sembolü path kanıtıyla birlikte çıkarır. Çıplak
/// `ensure_sync` ve `bound::with_bound` gibi qualified biçimler desteklenir;
/// prose dili routing kararını etkilemez.
fn scoped_symbol_hints(query: &str) -> Vec<ScopedSymbolHint> {
    let spans = code_spans(query);
    let backticked: HashSet<&str> = backtick_spans(query).into_iter().collect();
    let (scope, definition_file) = hint_paths(&spans);
    // Uzantısız göreli yol (`src/flask`) da kapsam sayılır.
    let scope = scope.or_else(|| {
        spans
            .iter()
            .find(|part| {
                part.contains('/')
                    && Path::new(part).extension().is_none()
                    && !Path::new(part).is_absolute()
                    && !part.split('/').any(|segment| segment == "..")
            })
            .map(|part| format!("{}/", part.trim_start_matches("./").trim_end_matches('/')))
    });
    let mut hints = Vec::new();
    for part in spans {
        if part.contains('/') {
            continue;
        }
        if let Some((symbol, files)) = python_module_hint(part) {
            hints.push(ScopedSymbolHint {
                symbol,
                scope: None,
                definition_files: files,
                code_like: false,
            });
            continue;
        }
        let symbol = if let Some((qualifier, symbol)) = part.rsplit_once("::") {
            if qualifier.is_empty()
                || !identifier_part(symbol)
                || !qualifier.split("::").all(identifier_part)
            {
                continue;
            }
            symbol
        } else if identifier_part(part) {
            part
        } else {
            continue;
        };
        hints.push(ScopedSymbolHint {
            symbol: symbol.to_string(),
            scope: scope.clone(),
            definition_files: definition_file.iter().cloned().collect(),
            code_like: part.contains("::") || backticked.contains(part) || code_like_name(symbol),
        });
    }
    hints
}

/// İpuçlarından tek hedef seçer: her ipucu yol kanıtıyla (tanım dosyası ya da
/// kapsam) veya kapsamsız kod-benzeri adın projedeki tek tanımıyla çözülür. Ancak
/// tek bir ipucu tek bir düğüme çözülürse hedef döner; birden çok farklı hedef ya
/// da belirsiz ad sessizce seçilmez.
/// Sorgudaki açık sembollerin çözümü: hiç, tek ya da birden çok farklı hedef.
enum ScopedTargets {
    None,
    One(String),
    /// Birden çok farklı hedef (`yol:satır Tür ad` satırları); sessiz seçim yapılmaz.
    Many(Vec<String>),
}

async fn scoped_symbol_targets(engine: &Arc<RetrievalEngine>, query: &str) -> ScopedTargets {
    let hints = scoped_symbol_hints(query);
    if hints.is_empty() {
        return ScopedTargets::None;
    }
    let graph = engine.graph.read().await;
    let callables: Vec<petgraph::graph::NodeIndex> = graph
        .graph
        .node_indices()
        .filter(|idx| {
            matches!(
                graph.graph[*idx].node_type,
                ccm_core::graph::NodeType::Function | ccm_core::graph::NodeType::Method
            )
        })
        .collect();
    let mut resolved: Vec<petgraph::graph::NodeIndex> = Vec::new();
    for hint in &hints {
        // Düz sözcükler (`call`, `HTML`) kapsam olsa bile hedef olmaz; yalnız modül
        // yoluyla ya da kod gibi yazılmış ad aday olur.
        if hint.definition_files.is_empty() && !hint.code_like {
            continue;
        }
        let matches: Vec<petgraph::graph::NodeIndex> = callables
            .iter()
            .copied()
            .filter(|idx| {
                let node = &graph.graph[*idx];
                if node.name != hint.symbol {
                    return false;
                }
                let path = ccm_core::engine::extract_file_path(&node.id);
                let path = path.trim_start_matches("./");
                if !hint.definition_files.is_empty() {
                    hint.definition_files.iter().any(|file| file == path)
                } else if let Some(scope) = hint.scope.as_deref() {
                    path.starts_with(scope)
                } else {
                    !is_test_path(path)
                }
            })
            .collect();
        if let [only] = matches.as_slice() {
            if !resolved.contains(only) {
                resolved.push(*only);
            }
        }
    }
    match resolved.as_slice() {
        [] => ScopedTargets::None,
        [only] => ScopedTargets::One(graph.graph[*only].id.clone()),
        many => ScopedTargets::Many(
            many.iter()
                .map(|idx| {
                    let node = &graph.graph[*idx];
                    format!(
                        "{}:{} {:?} {}",
                        ccm_core::engine::extract_file_path(&node.id).trim_start_matches("./"),
                        node.start_line,
                        node.node_type,
                        node.name
                    )
                })
                .collect(),
        ),
    }
}

/// Sorgu birden çok açık hedef adlandırdığında dönen metin: anlamsal aramaya
/// düşmek yerine adaylar listelenir (yanlış paket, doğru soruyu tekrar sordurur).
fn several_targets_text(candidates: &[String]) -> String {
    let mut text = format!(
        "The query names {} different code targets; ask `context` about one of them (path:line or the dotted name):",
        candidates.len()
    );
    for candidate in candidates {
        text.push_str(&format!("\n- {candidate}"));
    }
    text
}

/// Bilinen bir kaynak dosya uzantısıyla biten yol (`a/b.rs`, `x.py`, `lib/y.js`).
fn is_source_file_path(path: &str) -> bool {
    !path.contains(':')
        && matches!(
            Path::new(path)
                .extension()
                .and_then(|extension| extension.to_str()),
            Some("rs" | "py" | "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "go" | "java")
        )
}

fn is_js_ts_path(path: &str) -> bool {
    matches!(
        Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str()),
        Some("js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs")
    )
}

struct MemberTarget {
    node_id: String,
    hint: MemberCallHint,
    expressions: Vec<String>,
    /// Tanım dosyasında `this`'in takma adları (`var self = this`): `self.üye(`
    /// çağrısı `this.üye(` gibi aynı nesnenin metodudur.
    this_aliases: Vec<String>,
}

/// Satır `var|let|const AD = this` bildirimiyse takma ad.
fn this_alias_of(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    let rest = ["var ", "let ", "const "]
        .iter()
        .find_map(|keyword| trimmed.strip_prefix(keyword))?;
    let (name, value) = rest.split_once('=')?;
    let name = name.trim();
    let value = value.trim().trim_end_matches(';').trim();
    (identifier_part(name) && value == "this").then_some(name)
}

/// Tanım dosyasındaki fonksiyonlarda geçen `this` takma adları (sıralı, tekil).
fn this_aliases_in_file(graph: &ccm_core::graph::CodeGraph, file: &str) -> Vec<String> {
    let mut aliases: Vec<String> = graph
        .graph
        .node_weights()
        .filter(|node| {
            ccm_core::engine::extract_file_path(&node.id).trim_start_matches("./") == file
        })
        .flat_map(|node| {
            node.content
                .lines()
                .filter_map(this_alias_of)
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect();
    aliases.sort();
    aliases.dedup();
    aliases
}

fn js_language(path: &str) -> ccm_core::parser::SupportedLanguage {
    if matches!(
        Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str()),
        Some("jsx" | "tsx")
    ) {
        ccm_core::parser::SupportedLanguage::Tsx
    } else {
        ccm_core::parser::SupportedLanguage::TypeScript
    }
}

fn member_expression(node: tree_sitter::Node<'_>, content: &str) -> Option<String> {
    let mut parts = Vec::new();
    let mut current = node;
    while current.kind() == "member_expression" {
        let property = current.child_by_field_name("property")?;
        let name = property.utf8_text(content.as_bytes()).ok()?;
        if !identifier_part(name) {
            return None;
        }
        parts.push(name);
        current = current.child_by_field_name("object")?;
    }
    let root = current.utf8_text(content.as_bytes()).ok()?;
    if !identifier_part(root) {
        return None;
    }
    parts.push(root);
    parts.reverse();
    Some(parts.join("."))
}

/// Alias atamasının gerçekten aynı callable expression'a bağlandığını AST ile
/// kanıtlar. Yorum/string ve gövdedeki başka atamalar tanım kanıtı değildir.
fn assigned_callable<'a>(
    tree: &'a tree_sitter::Tree,
    content: &str,
    expression: &str,
) -> Option<(tree_sitter::Node<'a>, Vec<String>)> {
    let mut current = tree.root_node().named_child(0)?;
    if current.kind() == "expression_statement" {
        current = current.named_child(0)?;
    }
    let mut bound = false;
    let mut expressions = Vec::new();
    while current.kind() == "assignment_expression" {
        let left = current.child_by_field_name("left")?;
        if let Some(binding) = member_expression(left, content) {
            bound |= binding == expression;
            expressions.push(binding);
        }
        current = current.child_by_field_name("right")?;
    }
    (bound && matches!(current.kind(), "function_expression" | "arrow_function"))
        .then_some((current, expressions))
}

/// Gövdenin başındaki atama zinciri (`req.get =\nreq.header = function …`) bir
/// fonksiyona bağlanıyorsa atanan üye ifadeleri; değilse `None`.
fn assignment_members(tree: &tree_sitter::Tree, content: &str) -> Option<Vec<String>> {
    let mut current = tree.root_node().named_child(0)?;
    if current.kind() == "expression_statement" {
        current = current.named_child(0)?;
    }
    let mut expressions = Vec::new();
    while current.kind() == "assignment_expression" {
        let left = current.child_by_field_name("left")?;
        if let Some(binding) = member_expression(left, content) {
            expressions.push(binding);
        }
        current = current.child_by_field_name("right")?;
    }
    (!expressions.is_empty() && matches!(current.kind(), "function_expression" | "arrow_function"))
        .then_some(expressions)
}

/// JS/TS hedefi `alıcı.üye = function` atamasıyla tanımlandıysa onu alıcı
/// duyarlı hedefe çevirir. Sorgu tanım dosyasını söylemese de (`res.set`) çıplak
/// ad (`set`) yerine atanan ifadeler aranır: aksi halde aynı adlı başka alıcıların
/// (`app.set`) çağrıları caller ve "dışarıda kalan" satır olarak karışıyordu.
async fn assigned_member_target(
    engine: &Arc<RetrievalEngine>,
    node_id: &str,
) -> Result<Option<MemberTarget>> {
    let graph = engine.graph.read().await;
    let Some(idx) = graph.find_node_index_by_id(node_id) else {
        return Ok(None);
    };
    let node = &graph.graph[idx];
    let path = ccm_core::engine::extract_file_path(&node.id);
    let path = path.trim_start_matches("./");
    if !is_js_ts_path(path)
        || !matches!(
            node.node_type,
            ccm_core::graph::NodeType::Function | ccm_core::graph::NodeType::Method
        )
    {
        return Ok(None);
    }
    let tree = ccm_core::parser::CodeParser::new().parse_tree(&node.content, js_language(path))?;
    let Some(expressions) = assignment_members(&tree, &node.content) else {
        return Ok(None);
    };
    let Some((receiver, member)) = expressions[0].rsplit_once('.') else {
        return Ok(None);
    };
    if !receiver_identifies_callers(receiver) {
        return Ok(None);
    }
    Ok(Some(MemberTarget {
        node_id: node.id.clone(),
        hint: MemberCallHint {
            receiver: receiver.to_string(),
            member: member.to_string(),
            scope: None,
            definition_file: Some(path.to_string()),
        },
        expressions,
        this_aliases: this_aliases_in_file(&graph, path),
    }))
}

async fn member_call_target(
    engine: &Arc<RetrievalEngine>,
    query: &str,
) -> Result<Option<MemberTarget>> {
    let hints = member_call_hints(query);
    let graph = engine.graph.read().await;
    let mut candidates = Vec::new();
    for hint in hints {
        let Some(file) = hint
            .definition_file
            .as_deref()
            .filter(|file| is_js_ts_path(file))
        else {
            continue;
        };
        let expression = format!("{}.{}", hint.receiver, hint.member);
        for idx in graph.graph.node_indices() {
            let node = &graph.graph[idx];
            if !matches!(
                node.node_type,
                ccm_core::graph::NodeType::Function | ccm_core::graph::NodeType::Method
            ) || ccm_core::engine::extract_file_path(&node.id).trim_start_matches("./") != file
            {
                continue;
            }
            let tree =
                ccm_core::parser::CodeParser::new().parse_tree(&node.content, js_language(file))?;
            if let Some((callable, expressions)) =
                assigned_callable(&tree, &node.content, &expression)
            {
                candidates.push((
                    idx,
                    hint.clone(),
                    callable.utf8_text(node.content.as_bytes())?.to_string(),
                    expressions,
                ));
            }
        }
    }
    candidates.sort_by_key(|(idx, _, _, _)| {
        (
            graph.graph[*idx].start_line,
            std::cmp::Reverse(graph.graph[*idx].content.len()),
        )
    });
    let Some((first, first_hint, body, _)) = candidates.first() else {
        return Ok(None);
    };
    for (other, other_hint, other_body, _) in candidates.iter().skip(1) {
        let contains = |parent, child| {
            graph.graph.edges(parent).any(|edge| {
                edge.target() == child
                    && matches!(edge.weight(), ccm_core::graph::EdgeType::Contains)
            })
        };
        if other != first
            && !(body == other_body
                && graph.graph[*first].end_line == graph.graph[*other].end_line
                && (contains(*first, *other) || contains(*other, *first)))
        {
            return Err(input_error(format!(
                "ambiguous code hints: {}.{} and {}.{} in {}; pass one explicit target",
                first_hint.receiver,
                first_hint.member,
                other_hint.receiver,
                other_hint.member,
                first_hint.definition_file.as_deref().unwrap_or_default()
            )));
        }
    }
    let mut expressions: Vec<String> = candidates
        .iter()
        .flat_map(|(_, _, _, expressions)| expressions.iter().cloned())
        .collect();
    expressions.sort();
    expressions.dedup();
    let this_aliases = first_hint
        .definition_file
        .as_deref()
        .map_or_else(Vec::new, |file| this_aliases_in_file(&graph, file));
    Ok(Some(MemberTarget {
        node_id: graph.graph[*first].id.clone(),
        hint: first_hint.clone(),
        expressions,
        this_aliases,
    }))
}

fn contains_member_call(
    tree: &tree_sitter::Tree,
    content: &str,
    expression: &str,
    allow_suffix: bool,
) -> bool {
    let mut stack = vec![tree.root_node()];
    while let Some(node) = stack.pop() {
        if node.kind() == "call_expression"
            && node
                .child_by_field_name("function")
                .and_then(|callee| member_expression(callee, content))
                .is_some_and(|callee| {
                    callee == expression
                        || (allow_suffix
                            && callee
                                .strip_suffix(expression)
                                .is_some_and(|prefix| prefix.ends_with('.')))
                })
        {
            return true;
        }
        let mut cursor = node.walk();
        stack.extend(node.named_children(&mut cursor));
    }
    false
}

/// Graph receiver çözümü olmayan lexical diller için dar evidence: yalnız
/// Function/Method gövdeleri, query'deki path scope ve receiver.member çağrısı.
/// Tanım dosyasında this.member aynı receiver'ın metodu sayılır; başka
/// dosyalarda yalnız query'deki receiver zinciri kabul edilir.
async fn receiver_call_evidence(
    engine: &Arc<RetrievalEngine>,
    target: &MemberTarget,
) -> Result<Vec<ccm_core::engine::ContextSuggestion>> {
    let hint = &target.hint;
    if !hint.definition_file.as_deref().is_some_and(is_js_ts_path) {
        return Ok(Vec::new());
    }
    let graph = engine.graph.read().await;
    let mut suggestions = Vec::new();
    // Hedefin kendi gövdesindeki düğümler (atama zincirinin iç adı, ör.
    // `res.set = res.header = function header`) caller değil, hedefin kendisidir.
    let target_span = graph
        .find_node_index_by_id(&target.node_id)
        .map(|idx| &graph.graph[idx])
        .map(|node| {
            (
                ccm_core::engine::extract_file_path(&node.id),
                node.start_line,
                node.end_line,
            )
        });

    for idx in graph.graph.node_indices() {
        let node = &graph.graph[idx];
        if !matches!(
            node.node_type,
            ccm_core::graph::NodeType::Function | ccm_core::graph::NodeType::Method
        ) {
            continue;
        }
        if target.node_id == node.id {
            continue;
        }
        if target_span.as_ref().is_some_and(|(file, start, end)| {
            *file == ccm_core::engine::extract_file_path(&node.id)
                && *start <= node.start_line
                && node.end_line <= *end
        }) {
            continue;
        }
        let path = ccm_core::engine::extract_file_path(&node.id);
        let path = path.trim_start_matches("./");
        // Test dosyaları caller listesine girmez (graf yolundaki `tests` ayrımıyla aynı).
        if !is_js_ts_path(path) || is_test_path(path) {
            continue;
        }
        if hint
            .scope
            .as_deref()
            .is_some_and(|scope| !path.starts_with(scope))
        {
            continue;
        }
        let tree =
            ccm_core::parser::CodeParser::new().parse_tree(&node.content, js_language(path))?;
        let matching_expression =
            target.expressions.iter().find(|expression| {
                contains_member_call(&tree, &node.content, expression, true)
                    || (hint.definition_file.as_deref() == Some(path)
                        && expression.rsplit_once('.').is_some_and(|(_, member)| {
                            // Takma ad yalnız onu bildiren gövdede geçerlidir (`var self = this`
                            // başka bir fonksiyondaki `self`'i bağlamaz).
                            std::iter::once("this")
                                .chain(target.this_aliases.iter().map(String::as_str).filter(
                                    |alias| {
                                        node.content
                                            .lines()
                                            .any(|line| this_alias_of(line) == Some(*alias))
                                    },
                                ))
                                .any(|receiver| {
                                    contains_member_call(
                                        &tree,
                                        &node.content,
                                        &format!("{receiver}.{member}"),
                                        false,
                                    )
                                })
                        }))
            });
        let Some(expression) = matching_expression else {
            continue;
        };
        suggestions.push(ccm_core::engine::node_suggestion(
            node,
            format!("receiver-call evidence for {expression}"),
        ));
    }

    suggestions.sort_by(|left, right| {
        (
            left.file_path.as_deref().unwrap_or_default(),
            left.start_line.unwrap_or_default(),
            &left.title,
        )
            .cmp(&(
                right.file_path.as_deref().unwrap_or_default(),
                right.start_line.unwrap_or_default(),
                &right.title,
            ))
    });
    let mut suggestions = drop_alias_inner_callers(suggestions);
    suggestions.truncate(RECEIVER_EVIDENCE_LIMIT);
    Ok(suggestions)
}

/// Alıcı adı çağıranları tanımlar mı. `exports.f = function` ya da
/// `Foo.prototype.f = function` başka adlarla çağrılır (`utils.f(`, `obj.f(`):
/// yalnız atanan ifadeyi aramak gerçek çağıranları düşürür ve yanlış bir "tam"
/// iddiası doğururdu; bu hedefler graf yolunda kalır.
fn receiver_identifies_callers(receiver: &str) -> bool {
    receiver != "exports"
        && receiver != "module.exports"
        && receiver != "module"
        && !receiver.split('.').any(|part| part == "prototype")
}

/// Atama zincirindeki iç takma ad düğümünü atar: `res.contentType = res.type =
/// function contentType` iki düğüm üretir (619-626 ve 620-626); aynı dosyada,
/// aynı satırda biten ve başka bir caller'ın içinde başlayan düğüm aynı gövdedir.
fn drop_alias_inner_callers(
    suggestions: Vec<ccm_core::engine::ContextSuggestion>,
) -> Vec<ccm_core::engine::ContextSuggestion> {
    let spans: Vec<(Option<String>, Option<usize>, Option<usize>)> = suggestions
        .iter()
        .map(|item| (item.file_path.clone(), item.start_line, item.end_line))
        .collect();
    suggestions
        .into_iter()
        .filter(|item| {
            !spans.iter().any(|(file, start, end)| {
                *file == item.file_path
                    && *end == item.end_line
                    && start
                        .zip(item.start_line)
                        .is_some_and(|(outer, inner)| outer < inner)
            })
        })
        .collect()
}

/// Sorgunun tamamı tek bir `alıcı.üye` ifadesiyse (`res.set`) ve projede o ifadeye
/// atanmış tek bir JS/TS fonksiyonu varsa onu hedef seçer. Dosya adı eşleşmesinden
/// (`test/res.set.js`) önce gelir; birden çok aday varsa seçim yapılmaz.
async fn sole_assigned_member_target(
    engine: &Arc<RetrievalEngine>,
    query: &str,
) -> Result<Option<MemberTarget>> {
    let expression = query.trim().trim_matches('`');
    let Some((receiver, member)) = expression.rsplit_once('.') else {
        return Ok(None);
    };
    if !receiver_identifies_callers(receiver) {
        return Ok(None);
    }
    if !identifier_part(member) || receiver.is_empty() || !receiver.split('.').all(identifier_part)
    {
        return Ok(None);
    }
    let graph = engine.graph.read().await;
    let mut found: Vec<(String, String, usize, Vec<String>)> = Vec::new();
    for idx in graph.graph.node_indices() {
        let node = &graph.graph[idx];
        let path = ccm_core::engine::extract_file_path(&node.id);
        let path = path.trim_start_matches("./");
        if !is_js_ts_path(path)
            || is_test_path(path)
            || !matches!(
                node.node_type,
                ccm_core::graph::NodeType::Function | ccm_core::graph::NodeType::Method
            )
        {
            continue;
        }
        // Ucuz ön süzgeç: ifadeyi metin olarak içermeyen gövde ayrıştırılmaz.
        if !node.content.contains(expression) {
            continue;
        }
        let tree =
            ccm_core::parser::CodeParser::new().parse_tree(&node.content, js_language(path))?;
        if let Some((_, expressions)) = assigned_callable(&tree, &node.content, expression) {
            found.push((
                node.id.clone(),
                path.to_string(),
                node.end_line,
                expressions,
            ));
        }
    }
    // Atama zincirinin iç adı aynı satırda biten ikinci bir düğümdür; tek gövde sayılır.
    found.sort_by_key(|(id, path, end, _)| (path.clone(), *end, id.len()));
    found.dedup_by(|later, earlier| later.1 == earlier.1 && later.2 == earlier.2);
    let [(node_id, path, _, expressions)] = found.as_slice() else {
        return Ok(None);
    };
    Ok(Some(MemberTarget {
        node_id: node_id.clone(),
        hint: MemberCallHint {
            receiver: receiver.to_string(),
            member: member.to_string(),
            scope: None,
            definition_file: Some(path.clone()),
        },
        expressions: expressions.clone(),
        this_aliases: this_aliases_in_file(&graph, path),
    }))
}

/// `context` paketinde gösterilen en çok callee satırı. "Kim çağırıyor" cevabında
/// callee'ler ikincildir; serde `deserialize_struct` paketinde 25 satır tutuyordu.
const CONTEXT_CALLEES: usize = 10;

/// Callee satırları: önce kesin çağrılar, sonra diğerleri; en çok `CONTEXT_CALLEES`
/// satır ve kalanların sayısı.
fn capped_callee_lines(callees: &[ccm_core::graph::Usage]) -> Vec<String> {
    let mut ordered: Vec<&ccm_core::graph::Usage> = callees.iter().collect();
    ordered.sort_by_key(|usage| usage.relation != ccm_core::graph::UsageRelation::Calls);
    let mut lines: Vec<String> = ordered
        .iter()
        .take(CONTEXT_CALLEES)
        .map(|usage| suggestion_line(&ccm_core::engine::usage_suggestion(usage)))
        .collect();
    if ordered.len() > CONTEXT_CALLEES {
        lines.push(format!(
            "  + {} more callees not listed",
            ordered.len() - CONTEXT_CALLEES
        ));
    }
    lines
}

/// `context` paketinde gösterilen en çok gövde satırı. Uzun gövdeler (220 satırlık
/// `with_bound`) "kim çağırıyor" cevabını büyütüyordu; kalan satırların aralığı yazılır.
const CONTEXT_BODY_LINES: usize = 40;

/// Gövdenin ilk `max_lines` satırı ve gösterilmeyen satır sayısı.
fn body_head(content: &str, max_lines: usize) -> (String, usize) {
    let total = content.lines().count();
    if total <= max_lines {
        return (content.to_string(), 0);
    }
    let head: Vec<&str> = content.lines().take(max_lines).collect();
    (head.join("\n"), total - max_lines)
}

/// Tool: context
/// Ajanın araç seçmesini istemeden tek sorgudan yeterli görev bağlamı üretir.
/// Kesin hedefler doğrudan graf açıklamasına gider; doğal dil sorguları hibrit
/// aramayla en iyi düğümü bulur ve aynı çağrıda yapısal açıklamayla zenginleşir.
pub async fn context(
    engine: &Arc<RetrievalEngine>,
    args: &Value,
    root: Option<&Path>,
) -> Result<ToolResult> {
    let query = args
        .get("query")
        .and_then(Value::as_str)
        .ok_or_else(|| input_error("Missing 'query' argument"))?
        .trim();
    if query.is_empty() {
        return Ok(tool_error("'query' cannot be empty".to_string()));
    }

    let max_tokens = max_tokens_from_args(args)?;
    let budget_chars = max_tokens.saturating_mul(CHARS_PER_TOKEN);

    // Tek `alıcı.üye` ifadesi ve ona atanmış tek fonksiyon: alıcı duyarlı hedef.
    if let Some(target) = sole_assigned_member_target(engine, query).await? {
        let explanation = match engine.explain(&target.node_id).await {
            Ok(explanation) => explanation,
            Err(error) => return Ok(tool_error(error.to_string())),
        };
        let evidence = receiver_call_evidence(engine, &target).await?;
        let callers = context_callers(engine, &explanation, Some(&evidence)).await?;
        let text = context_packet(
            engine,
            root,
            &explanation,
            &callers,
            Some(&target),
            max_tokens,
        )
        .await;
        return Ok(ToolResult {
            content: vec![ToolResultContent {
                content_type: "text".to_string(),
                text,
            }],
            is_error: None,
        });
    }

    // Önce aynı metni hedef gibi çözmeyi dene. Bu; sembol, dosya, path:line ve
    // node ID girişlerini semantik arama yapmadan deterministik olarak cevaplar.
    // Yalnızca bulunamayan ad doğal dil aramasına düşer; belirsiz hedef sessizce
    // seçilmez.
    match engine
        .resolve_target(target_form(args, "query", "target")?)
        .await
    {
        Ok(node_id) => {
            let explanation = match engine.explain(&node_id).await {
                Ok(explanation) => explanation,
                Err(error) => return Ok(tool_error(error.to_string())),
            };
            let member = assigned_member_target(engine, &node_id).await?;
            let evidence = match member.as_ref() {
                Some(target) => Some(receiver_call_evidence(engine, target).await?),
                None => None,
            };
            let callers = context_callers(engine, &explanation, evidence.as_deref()).await?;
            let text = context_packet(
                engine,
                root,
                &explanation,
                &callers,
                member.as_ref(),
                max_tokens,
            )
            .await;
            return Ok(ToolResult {
                content: vec![ToolResultContent {
                    content_type: "text".to_string(),
                    text,
                }],
                is_error: None,
            });
        }
        Err(ccm_core::engine::TargetError::NotFound(_)) => {}
        Err(error) => return Ok(tool_error(error.to_string())),
    }

    // Caller sorusu açık bir receiver.member ve tanım dosyası taşıyorsa
    // deterministik graph hedefini seç. Bu durumda semantic arama gereksizdir:
    // model/provider kalitesi routing kararını etkilemez ve embedding maliyeti
    // ödenmez. Dosyada aynı adlı birden çok callable varsa sessiz seçim yapılmaz.
    let member_target = match member_call_target(engine, query).await {
        Ok(target) => target,
        Err(error) => return Ok(tool_error(error.to_string())),
    };
    let scoped_target = match scoped_symbol_targets(engine, query).await {
        ScopedTargets::Many(candidates) if member_target.is_none() => {
            return Ok(ToolResult {
                content: vec![ToolResultContent {
                    content_type: "text".to_string(),
                    text: several_targets_text(&candidates),
                }],
                is_error: None,
            });
        }
        ScopedTargets::One(id) => Some(id),
        ScopedTargets::Many(_) | ScopedTargets::None => None,
    };
    let hinted_target = member_target
        .as_ref()
        .map(|target| target.node_id.clone())
        .or(scoped_target);
    let hits = if hinted_target.is_some() {
        Vec::new()
    } else {
        engine.search_code_hybrid(query, 5).await?
    };
    if hinted_target.is_none() && hits.is_empty() {
        return Ok(ToolResult {
            content: vec![ToolResultContent {
                content_type: "text".to_string(),
                text: format!("No context found for query: '{query}'"),
            }],
            is_error: None,
        });
    }

    // Deterministik hint hedefi varsa onu; yoksa en iyi hibrit graph düğümünü
    // yapısal açıklamayla zenginleştir.
    let selected_node_id = hinted_target
        .as_deref()
        .or_else(|| hits.first().and_then(|hit| hit.node_id.as_deref()));
    let member_target = match (member_target, selected_node_id) {
        (Some(target), _) => Some(target),
        (None, Some(node_id)) => assigned_member_target(engine, node_id).await?,
        (None, None) => None,
    };
    let hinted_callers = if let Some(target) = member_target.as_ref() {
        Some(receiver_call_evidence(engine, target).await?)
    } else {
        None
    };
    // Pakette zaten görünen düğümler "related matches" içinde tekrar edilmez.
    let mut shown: HashSet<String> = HashSet::new();
    let mut output = if let Some(node_id) = selected_node_id {
        match engine.explain(node_id).await {
            Ok(explanation) => {
                let callers =
                    context_callers(engine, &explanation, hinted_callers.as_deref()).await?;
                shown.insert(explanation.node.id.clone());
                shown.extend(callers.iter().filter_map(|caller| caller.node_id.clone()));
                shown.extend(
                    explanation
                        .callees
                        .iter()
                        .map(|usage| usage.node.id.clone()),
                );
                context_packet(
                    engine,
                    root,
                    &explanation,
                    &callers,
                    member_target.as_ref(),
                    max_tokens,
                )
                .await
            }
            Err(_) => format_suggestions_output(&hits, false, max_tokens, max_tokens),
        }
    } else {
        format_suggestions_output(&hits, false, max_tokens, max_tokens)
    };
    let related: Vec<ccm_core::engine::ContextSuggestion> = hits
        .iter()
        .skip(1)
        .filter(|hit| hit.node_id.as_ref().is_none_or(|id| !shown.contains(id)))
        .cloned()
        .collect();

    // Kalan bütçe varsa alternatifleri de kısa handle'lar olarak ver. Ajanın
    // ikinci bir search çağrısı yapmadan belirsizliği görmesini sağlar.
    if !related.is_empty() && char_count(&output) < budget_chars {
        let header = "related matches:\n";
        let remaining_chars = budget_chars
            .saturating_sub(char_count(&output))
            .saturating_sub(char_count(header));
        let remaining_tokens = remaining_chars / CHARS_PER_TOKEN;
        if remaining_tokens > 0 {
            output.push_str(header);
            output.push_str(&format_suggestions_output(
                &related,
                false,
                remaining_tokens,
                remaining_tokens,
            ));
        }
    }

    if char_count(&output) > budget_chars {
        output = clip(&output, budget_chars);
    }

    Ok(ToolResult {
        content: vec![ToolResultContent {
            content_type: "text".to_string(),
            text: output,
        }],
        is_error: None,
    })
}

/// İlişki etiketleri ve semantic ID'ler korunur; yalnız `context` caller satırına
/// Contains zincirinden kanıtlanan kapsam eklenir. Legacy çıktıları değişmez.
async fn context_callers(
    engine: &Arc<RetrievalEngine>,
    explanation: &ccm_core::graph::Explanation,
    caller_override: Option<&[ccm_core::engine::ContextSuggestion]>,
) -> Result<Vec<ccm_core::engine::ContextSuggestion>> {
    let mut callers = caller_override.map_or_else(
        || {
            explanation
                .usages
                .iter()
                .filter(|usage| !is_test_path(&ccm_core::engine::extract_file_path(&usage.node.id)))
                .map(ccm_core::engine::usage_suggestion)
                .collect()
        },
        |callers| callers.to_vec(),
    );
    // Ada göre çıkarılan kenar dil sınırını aşamaz: Python fonksiyonunu JS kodu
    // çağırmaz (`CSS.escape` ≠ `django.utils.html.escape`).
    let target_lang = crate::call_sites::lang_of(
        ccm_core::engine::extract_file_path(&explanation.node.id).trim_start_matches("./"),
    );
    callers.retain(|caller| {
        let lang = crate::call_sites::lang_of(
            caller
                .file_path
                .as_deref()
                .unwrap_or_default()
                .trim_start_matches("./"),
        );
        lang == target_lang || target_lang == crate::call_sites::Lang::Other
    });
    // Atamayla tanımlı JS/TS caller (`res.sendfile = function`) atandığı ifadeyle
    // de anılır: aynı adlı yardımcı fonksiyonla (`function sendfile`) karışmasın.
    for caller in &mut callers {
        let path = caller
            .file_path
            .as_deref()
            .unwrap_or_default()
            .trim_start_matches("./")
            .to_string();
        if !is_js_ts_path(&path) || caller.content.is_empty() {
            continue;
        }
        let tree =
            ccm_core::parser::CodeParser::new().parse_tree(&caller.content, js_language(&path))?;
        if let Some(members) = assignment_members(&tree, &caller.content) {
            caller
                .reason
                .push_str(&format!("; assigned as {}", members.join(" = ")));
        }
    }
    let graph = engine.graph.read().await;
    for caller in &mut callers {
        let Some(id) = caller.node_id.as_deref() else {
            continue;
        };
        let mut idx = graph
            .find_node_index_by_id(id)
            .ok_or_else(|| input_error(format!("caller '{id}' is no longer indexed")))?;
        let mut seen = HashSet::new();
        let mut scopes = Vec::new();
        loop {
            if !seen.insert(idx) {
                return Err(input_error(format!(
                    "cyclic Contains scope for caller '{id}'"
                )));
            }
            let node = &graph.graph[idx];
            if matches!(
                node.node_type,
                ccm_core::graph::NodeType::Class
                    | ccm_core::graph::NodeType::Struct
                    | ccm_core::graph::NodeType::Enum
                    | ccm_core::graph::NodeType::Trait
                    | ccm_core::graph::NodeType::Function
                    | ccm_core::graph::NodeType::Method
            ) {
                scopes.push(node);
            }
            let mut parents: Vec<_> = graph
                .graph
                .edges_directed(idx, petgraph::Direction::Incoming)
                .filter(|edge| matches!(edge.weight(), ccm_core::graph::EdgeType::Contains))
                .map(|edge| edge.source())
                .collect();
            parents.sort_unstable();
            parents.dedup();
            match parents.as_slice() {
                [] => break,
                [parent] => idx = *parent,
                _ => {
                    return Err(input_error(format!(
                        "ambiguous Contains scope for caller '{id}'"
                    )))
                }
            }
        }
        if scopes.len() < 2 {
            continue;
        }
        scopes.reverse();
        let qualified: Vec<&str> = scopes.iter().map(|node| node.name.as_str()).collect();
        caller
            .reason
            .push_str(&format!("; scope: {}", qualified.join(".")));
        if let Some(outer) = scopes.iter().position(|node| {
            matches!(
                node.node_type,
                ccm_core::graph::NodeType::Function | ccm_core::graph::NodeType::Method
            )
        }) {
            if outer + 1 < scopes.len() {
                caller.reason.push_str(&format!(
                    "; enclosing callable: {}",
                    qualified[..=outer].join(".")
                ));
            }
        }
    }
    Ok(callers)
}

/// Dal alternatiflerini (`…:symbol:view:1`, `…:2`) tek anahtarda birleştirir; caller
/// listesi de aynı biçimde tekilleştirilir.
fn listed_key(node_id: &str) -> String {
    semantic_symbol_occurrence_key(node_id)
        .unwrap_or(node_id)
        .to_string()
}

/// Paketteki hedef için grep eşdeğeri denetimin tanımı. Receiver ipucu varsa onun
/// ifadeleri (ve tanım dosyasında `this.üye`) aranır; yoksa yalnız Function/Method
/// hedefinin çıplak adı. Sınıf ve dosya hedeflerinde denetim yapılmaz.
/// Tanımın bir sınıfın içinde olup olmadığı (Python'da metotlar da `Function`'dır).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DefinitionPlace {
    InsideClass,
    OutsideClass,
}

/// Aynı dosyada düğümün satır aralığını kapsayan bir sınıf varsa `InsideClass`.
async fn definition_place(
    engine: &Arc<RetrievalEngine>,
    node: &ccm_core::graph::CodeNode,
) -> DefinitionPlace {
    let graph = engine.graph.read().await;
    let file = ccm_core::engine::extract_file_path(&node.id);
    let inside = graph.graph.node_weights().any(|other| {
        other.node_type == ccm_core::graph::NodeType::Class
            && ccm_core::engine::extract_file_path(&other.id) == file
            && other.start_line <= node.start_line
            && node.end_line <= other.end_line
    });
    if inside {
        DefinitionPlace::InsideClass
    } else {
        DefinitionPlace::OutsideClass
    }
}

fn check_spec(
    node: &ccm_core::graph::CodeNode,
    member: Option<&MemberTarget>,
    place: DefinitionPlace,
) -> Option<crate::call_sites::CheckSpec> {
    use crate::call_sites::{CheckKind, CheckSpec, Needle};
    if let Some(target) = member {
        let mut needles: Vec<Needle> = target
            .expressions
            .iter()
            .map(|expression| Needle {
                text: expression.clone(),
                only_file: None,
            })
            .collect();
        let mut members: Vec<&str> = target
            .expressions
            .iter()
            .filter_map(|expression| expression.rsplit_once('.').map(|(_, member)| member))
            .collect();
        members.sort_unstable();
        members.dedup();
        for member in members {
            for receiver in
                std::iter::once("this").chain(target.this_aliases.iter().map(String::as_str))
            {
                let text = format!("{receiver}.{member}");
                // Dosyasız aynı metin (`res.send(` her dosyada) dosyalıyı zaten kapsar.
                if needles.iter().any(|needle| needle.text == text) {
                    continue;
                }
                needles.push(Needle {
                    text,
                    only_file: target.hint.definition_file.clone(),
                });
            }
        }
        return Some(CheckSpec {
            needles,
            kind: CheckKind::MemberCalls,
            scope: target.hint.scope.clone(),
        });
    }
    if !matches!(
        node.node_type,
        ccm_core::graph::NodeType::Function | ccm_core::graph::NodeType::Method
    ) {
        return None;
    }
    let path = ccm_core::engine::extract_file_path(&node.id);
    let kind = match (
        crate::call_sites::lang_of(path.trim_start_matches("./")),
        place,
    ) {
        (crate::call_sites::Lang::Rust, _)
            if !crate::call_sites::rust_takes_self(&node.content, &node.name) =>
        {
            CheckKind::RustNoSelfUses
        }
        (crate::call_sites::Lang::Python, DefinitionPlace::OutsideClass) => {
            CheckKind::PythonModuleFunctionUses
        }
        _ => CheckKind::NameUses,
    };
    Some(CheckSpec {
        needles: vec![Needle {
            text: node.name.clone(),
            only_file: None,
        }],
        kind,
        scope: None,
    })
}

/// Kesin çağrı kenarıyla aynı adlı *başka* bir tanıma bağlanan kapsamlar: kapsam
/// anahtarı → o tanımın `yol:satır` yeri. Bu kapsamlardaki geçişler hedefin eksik
/// caller'ı değil, başka fonksiyonun çağrısıdır.
fn resolved_elsewhere(
    graph: &ccm_core::graph::CodeGraph,
    target_id: &str,
    target_name: &str,
) -> std::collections::HashMap<String, String> {
    use ccm_core::graph::EdgeType;
    let mut map = std::collections::HashMap::new();
    for edge in graph.graph.edge_references() {
        if !matches!(edge.weight(), EdgeType::Calls) {
            continue;
        }
        let callee = &graph.graph[edge.target()];
        if callee.name != target_name || callee.id == target_id {
            continue;
        }
        let caller = &graph.graph[edge.source()];
        map.insert(
            listed_key(&caller.id),
            format!(
                "{}:{}",
                ccm_core::engine::extract_file_path(&callee.id).trim_start_matches("./"),
                callee.start_line
            ),
        );
    }
    map
}

/// İndeksteki kaynak dosyaları ve sembol aralıkları (denetimin en içteki kapsamı için).
fn indexed_files(graph: &ccm_core::graph::CodeGraph) -> Vec<crate::call_sites::IndexedFile> {
    use ccm_core::graph::NodeType;
    let mut files: std::collections::BTreeMap<String, crate::call_sites::IndexedFile> =
        std::collections::BTreeMap::new();
    for idx in graph.graph.node_indices() {
        let node = &graph.graph[idx];
        if node.node_type == NodeType::File {
            let path = node.id.trim_start_matches("./").to_string();
            files
                .entry(node.id.clone())
                .or_insert_with(|| crate::call_sites::IndexedFile {
                    path,
                    file_key: listed_key(&node.id),
                    symbols: Vec::new(),
                });
        }
    }
    for idx in graph.graph.node_indices() {
        let node = &graph.graph[idx];
        // Değişken ve import düğümleri kapsam değildir: çağrı satırını kendi
        // fonksiyonundan koparırlar.
        if !matches!(
            node.node_type,
            NodeType::Function
                | NodeType::Method
                | NodeType::Class
                | NodeType::Struct
                | NodeType::Enum
                | NodeType::Trait
                | NodeType::Module
        ) {
            continue;
        }
        let file_id = ccm_core::engine::extract_file_path(&node.id);
        if let Some(file) = files.get_mut(&file_id) {
            file.symbols.push(crate::call_sites::SymbolSpan {
                key: listed_key(&node.id),
                start: node.start_line,
                end: node.end_line,
                content: node.content.clone(),
            });
        }
    }
    files.into_values().collect()
}

/// Denetimi çalıştırır ve pakete yazılacak satırı döndürür. Okuma hatası sessizce
/// yutulmaz: denetimin neden yapılamadığı satırda yazılır.
async fn call_site_check(
    engine: &Arc<RetrievalEngine>,
    root: Option<&Path>,
    spec: &crate::call_sites::CheckSpec,
    target_id: &str,
    target_name: &str,
    callers: &[ccm_core::engine::ContextSuggestion],
    code_callers: usize,
) -> String {
    let Some(root) = root else {
        return "Check skipped: the server has no project root for this index.\n".to_string();
    };
    if callers.len() >= RECEIVER_EVIDENCE_LIMIT
        && spec.kind == crate::call_sites::CheckKind::MemberCalls
    {
        return format!(
            "Check skipped: receiver evidence stops at {RECEIVER_EVIDENCE_LIMIT} callers.\n"
        );
    }
    let (files, elsewhere) = {
        let graph = engine.graph.read().await;
        (
            indexed_files(&graph),
            resolved_elsewhere(&graph, target_id, target_name),
        )
    };
    let paths: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();
    let roots = crate::call_sites::python_roots(&paths);
    let listed: HashSet<String> = callers
        .iter()
        .filter_map(|caller| caller.node_id.as_deref().map(listed_key))
        .collect();
    let target_lang = crate::call_sites::lang_of(
        ccm_core::engine::extract_file_path(target_id).trim_start_matches("./"),
    );
    let files: Vec<crate::call_sites::IndexedFile> = files
        .into_iter()
        .filter(|file| crate::call_sites::lang_of(&file.path) == target_lang)
        .collect();
    let files_in_scope = files
        .iter()
        .filter(|file| {
            !is_test_path(&file.path)
                && spec
                    .scope
                    .as_deref()
                    .is_none_or(|scope| file.path.starts_with(scope))
        })
        .count();
    match crate::call_sites::read_sources(root, &files, spec) {
        Ok(scanned) => {
            let facts = crate::call_sites::IndexFacts {
                listed,
                roots,
                files_in_scope,
                elsewhere,
                target_key: listed_key(target_id),
            };
            let outcome =
                crate::call_sites::reconcile(&scanned, spec, target_name, &facts, &is_test_path);
            crate::call_sites::outcome_text(spec, &outcome, code_callers)
        }
        Err(error) => format!("Check skipped: {error:#}\n"),
    }
}

/// `context` paketi: başlık, grep eşdeğeri denetim, çağrı yeri satırlarıyla
/// caller'lar, gövde ve kalan bölümler. Caller'lar gövdeden önce gelir: "kim
/// çağırıyor" sorusunun cevabı bütçe kesilse de kalır. Liste kesilirse tamlık
/// iddiası yazılmaz.
async fn context_packet(
    engine: &Arc<RetrievalEngine>,
    root: Option<&Path>,
    explanation: &ccm_core::graph::Explanation,
    callers: &[ccm_core::engine::ContextSuggestion],
    member: Option<&MemberTarget>,
    max_tokens: usize,
) -> String {
    use ccm_core::engine::{extract_file_path, node_suggestion, usage_suggestion};
    let node = &explanation.node;
    let callers = dedupe_semantic_callers(callers.iter().cloned());
    let place = definition_place(engine, node).await;
    let spec = check_spec(node, member, place);
    let tests: Vec<&ccm_core::graph::Usage> = explanation
        .usages
        .iter()
        .filter(|usage| is_test_path(&extract_file_path(&usage.node.id)))
        .collect();
    let title = match node.node_type {
        ccm_core::graph::NodeType::File => "File".to_string(),
        _ => format!("{:?} `{}`", node.node_type, node.name),
    };
    let code_callers = callers
        .iter()
        .filter(|caller| caller.node_type.as_deref() != Some("File"))
        .count();
    let mut output = format!(
        "{title} · {}:{}-{} · {} callers, {} callees, {} tests",
        extract_file_path(&node.id).trim_start_matches("./"),
        node.start_line,
        node.end_line,
        code_callers,
        explanation.callees.len(),
        tests.len()
    );
    if !explanation.members.is_empty() {
        output.push_str(&format!(", {} members", explanation.members.len()));
    }
    output.push('\n');

    let check = match &spec {
        Some(spec) => Some(
            call_site_check(
                engine,
                root,
                spec,
                &node.id,
                &node.name,
                &callers,
                code_callers,
            )
            .await,
        ),
        None => None,
    };
    // İmport eden dosyalar denetimde sayılır ama listede tek satırdır: "kim
    // çağırıyor" sorusunda her biri bir satır token'ı harcıyordu.
    let importers = callers
        .iter()
        .filter(|caller| caller.node_type.as_deref() == Some("File"))
        .count();
    let mut caller_lines: Vec<String> = callers
        .iter()
        .filter(|caller| caller.node_type.as_deref() != Some("File"))
        .map(|caller| {
            let mut line = suggestion_line(caller).replace(
                "may call (receiver type unknown or several candidates)",
                "may call (receiver not resolved)",
            );
            if let (Some(spec), Some(path), Some(start), Some(end)) = (
                &spec,
                caller.file_path.as_deref(),
                caller.start_line,
                caller.end_line,
            ) {
                for site in crate::call_sites::caller_site_lines(
                    path.trim_start_matches("./"),
                    &caller.content,
                    start,
                    end,
                    spec,
                    &node.name,
                ) {
                    line.push('\n');
                    line.push_str(&site);
                }
            }
            line
        })
        .collect();
    if importers > 0 {
        caller_lines.push(format!(
            "  + {importers} files that import it (module-level uses there count as inside)"
        ));
    }

    let budget = max_tokens.saturating_mul(CHARS_PER_TOKEN);
    let check_chars = check.as_deref().map_or(0, char_count);
    let mut used = char_count(&output) + check_chars;
    // Caller'lar önceliklidir: gövde ve diğer bölümler için yalnız küçük bir pay kalır.
    let caller_budget = budget.saturating_sub(used).saturating_sub(budget / 8);
    let (caller_section, shown) = if caller_lines.is_empty() {
        (String::new(), 0)
    } else {
        section_text_counted("callers", &caller_lines, caller_budget)
    };
    match check {
        Some(_) if shown < caller_lines.len() => output.push_str(
            "Check skipped: the caller list below was cut; raise max_tokens for the full list and the check.\n",
        ),
        Some(check) => output.push_str(&check),
        None => {}
    }
    used = char_count(&output) + char_count(&caller_section);
    output.push_str(&caller_section);

    if !node.content.is_empty() {
        let limit = budget
            .saturating_sub(used)
            .saturating_div(2)
            .saturating_sub(8);
        let (head, hidden_lines) = body_head(&node.content, CONTEXT_BODY_LINES);
        let body = clip(&head, limit);
        if !body.is_empty() {
            output.push_str("```\n");
            output.push_str(&body);
            output.push_str("\n```\n");
            used += char_count(&body) + 8;
        }
        if hidden_lines > 0 {
            let first = node.end_line.saturating_sub(hidden_lines) + 1;
            let notice = format!(
                "… body continues for {hidden_lines} more lines ({}:{first}-{}); read them only if needed\n",
                extract_file_path(&node.id).trim_start_matches("./"),
                node.end_line
            );
            used += char_count(&notice);
            output.push_str(&notice);
        } else if char_count(&head) > limit {
            let notice = "… body clipped (raise max_tokens)\n";
            output.push_str(notice);
            used += char_count(notice);
        }
    }

    let sections: Vec<(&str, Vec<String>)> = vec![
        ("callees", capped_callee_lines(&explanation.callees)),
        (
            "members",
            explanation
                .members
                .iter()
                .map(|member| suggestion_line(&node_suggestion(member, String::new())))
                .collect(),
        ),
        (
            "tests",
            tests
                .iter()
                .map(|usage| suggestion_line(&usage_suggestion(usage)))
                .collect(),
        ),
    ];
    let filled: Vec<&(&str, Vec<String>)> = sections
        .iter()
        .filter(|(_, lines)| !lines.is_empty())
        .collect();
    for (index, (title, lines)) in filled.iter().enumerate() {
        let share = budget.saturating_sub(used) / (filled.len() - index);
        let (section, _) = section_text_counted(title, lines, share);
        used += char_count(&section);
        output.push_str(&section);
    }
    output
}

/// Tool: map
/// Projenin haritası: dosyalar diğer dosyalardan kullanımlarına göre sıralı.
pub async fn project_map(engine: &Arc<RetrievalEngine>, args: &Value) -> Result<ToolResult> {
    // `.` ve boş yol tüm projedir; metin olmayan yol sessizce yok sayılmaz.
    let prefix = match present_argument(args, "path") {
        None => String::new(),
        Some(Value::String(path)) if matches!(path.trim(), "" | "." | "./") => String::new(),
        Some(Value::String(path)) => normalize_graph_path(
            path.trim(),
            args.get("project_path").and_then(Value::as_str),
        )?,
        Some(other) => return Err(input_error(format!("'path' must be a string, got {other}"))),
    };
    let max_tokens = requested_tokens(args)?.unwrap_or(DEFAULT_MAP_TOKENS);
    match engine.project_map(&prefix, max_tokens).await {
        Ok(text) => Ok(ToolResult {
            content: vec![ToolResultContent {
                content_type: "text".to_string(),
                text,
            }],
            is_error: None,
        }),
        Err(error) => Ok(tool_error(error.to_string())),
    }
}

/// Tool: explain
/// Sembolü tek çağrıda açıklar: tanım, gövde, üyeler, çağıranlar, çağrılanlar ve testler.
pub async fn explain(engine: &Arc<RetrievalEngine>, args: &Value) -> Result<ToolResult> {
    let node_id = match engine
        .resolve_target(target_form(args, "target", "node_id")?)
        .await
    {
        Ok(id) => id,
        Err(error) => return Ok(tool_error(error.to_string())),
    };
    let explanation = match engine.explain(&node_id).await {
        Ok(explanation) => explanation,
        Err(error) => return Ok(tool_error(error.to_string())),
    };
    // explain'de gövde varsayılan olarak gösterilir; okuma çağrısını gereksiz kılar.
    let include_body = args
        .get("include_body")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    Ok(ToolResult {
        content: vec![ToolResultContent {
            content_type: "text".to_string(),
            text: format_explanation(&explanation, include_body, max_tokens_from_args(args)?),
        }],
        is_error: None,
    })
}

/// Başlık, bütçenin en çok yarısı kadar gövde, sonra `members:`, `callers:`,
/// `callees:` ve `tests:` listeleri; her liste kalan bütçeden eşit pay alır,
/// kullanmadığı pay sonraki listelere kalır.
fn format_explanation(
    explanation: &ccm_core::graph::Explanation,
    include_body: bool,
    max_tokens: usize,
) -> String {
    format_explanation_with_callers(explanation, include_body, max_tokens, None)
}

fn semantic_symbol_occurrence_key(node_id: &str) -> Option<&str> {
    let (base, occurrence) = node_id.rsplit_once(':')?;
    if !base.contains(":symbol:")
        || occurrence.is_empty()
        || !occurrence.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    Some(base)
}

fn dedupe_semantic_callers(
    callers: impl IntoIterator<Item = ccm_core::engine::ContextSuggestion>,
) -> Vec<ccm_core::engine::ContextSuggestion> {
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for caller in callers {
        if let Some(key) = caller
            .node_id
            .as_deref()
            .and_then(semantic_symbol_occurrence_key)
        {
            if !seen.insert(key.to_string()) {
                continue;
            }
        }
        deduped.push(caller);
    }
    deduped
}

fn format_explanation_with_callers(
    explanation: &ccm_core::graph::Explanation,
    include_body: bool,
    max_tokens: usize,
    caller_override: Option<&[ccm_core::engine::ContextSuggestion]>,
) -> String {
    use ccm_core::engine::{extract_file_path, node_suggestion, usage_suggestion};
    let node = &explanation.node;
    let (tests, callers): (Vec<&ccm_core::graph::Usage>, Vec<&ccm_core::graph::Usage>) =
        explanation
            .usages
            .iter()
            .partition(|usage| is_test_path(&extract_file_path(&usage.node.id)));
    let caller_suggestions = dedupe_semantic_callers(caller_override.map_or_else(
        || {
            callers
                .iter()
                .map(|usage| usage_suggestion(usage))
                .collect()
        },
        |override_callers| override_callers.to_vec(),
    ));
    // Dosya düğümünün adı yolun kendisidir; başlıkta yol bir kez yazılır.
    let title = match node.node_type {
        ccm_core::graph::NodeType::File => "File".to_string(),
        _ => format!("{:?} `{}`", node.node_type, node.name),
    };
    let caller_count = caller_suggestions.len();
    let mut output = format!(
        "{title} · {}:{}-{} · {} callers, {} callees, {} tests",
        extract_file_path(&node.id).trim_start_matches("./"),
        node.start_line,
        node.end_line,
        caller_count,
        explanation.callees.len(),
        tests.len()
    );
    if !explanation.members.is_empty() {
        output.push_str(&format!(", {} members", explanation.members.len()));
    }
    output.push('\n');

    let budget = max_tokens.saturating_mul(CHARS_PER_TOKEN);
    let mut used = char_count(&output);
    if include_body && !node.content.is_empty() {
        let limit = (budget / 2).saturating_sub(used + 8);
        let body = clip(&node.content, limit);
        if !body.is_empty() {
            output.push_str("```\n");
            output.push_str(&body);
            output.push_str("\n```\n");
            used += char_count(&body) + 8;
        }
        if char_count(&node.content) > limit {
            let notice = "… body clipped (raise max_tokens)\n";
            output.push_str(notice);
            used += char_count(notice);
        }
    }

    let caller_lines: Vec<String> = caller_suggestions.iter().map(suggestion_line).collect();
    let sections: Vec<(&str, Vec<String>)> = vec![
        (
            "members",
            explanation
                .members
                .iter()
                .map(|member| suggestion_line(&node_suggestion(member, String::new())))
                .collect(),
        ),
        ("callers", caller_lines),
        (
            "callees",
            explanation
                .callees
                .iter()
                .map(|usage| suggestion_line(&usage_suggestion(usage)))
                .collect(),
        ),
        (
            "tests",
            tests
                .iter()
                .map(|usage| suggestion_line(&usage_suggestion(usage)))
                .collect(),
        ),
    ];
    let filled: Vec<&(&str, Vec<String>)> = sections
        .iter()
        .filter(|(_, lines)| !lines.is_empty())
        .collect();
    for (index, (title, lines)) in filled.iter().enumerate() {
        let share = budget.saturating_sub(used) / (filled.len() - index);
        let section = section_text(title, lines, share);
        used += char_count(&section);
        output.push_str(&section);
    }
    output
}

/// `section_text` ile aynı; ayrıca gösterilen satır sayısını döndürür.
fn section_text_counted(title: &str, lines: &[String], budget: usize) -> (String, usize) {
    let text = section_text(title, lines, budget);
    let hidden = text
        .lines()
        .last()
        .and_then(|line| line.strip_prefix("… "))
        .and_then(|rest| rest.split_once(' '))
        .and_then(|(count, rest)| {
            rest.starts_with("more (raise max_tokens)")
                .then(|| count.parse::<usize>().ok())
                .flatten()
        })
        .unwrap_or(0);
    (text, lines.len() - hidden)
}

/// `başlık:` ve `budget` karaktere sığan satırlar; sığmayanlar `… n more` ile sayılır.
fn section_text(title: &str, lines: &[String], budget: usize) -> String {
    let mut text = format!("{title}:\n");
    let mut used = char_count(&text);
    for (index, line) in lines.iter().enumerate() {
        let line_chars = char_count(line) + 1;
        if used + line_chars > budget {
            text.push_str(&format!(
                "… {} more (raise max_tokens)\n",
                lines.len() - index
            ));
            return text;
        }
        text.push_str(line);
        text.push('\n');
        used += line_chars;
    }
    text
}

/// Test dosyası mı (büyük-küçük harf duyarsız): `tests/`, `test/`, `__tests__/`
/// ya da `spec/` altında, adı `test_` ile başlıyor ya da `_test.`, `.test.`,
/// `.spec.` veya `_spec.` içeriyor.
fn is_test_path(path: &str) -> bool {
    let rooted = format!("/{}", path.trim_start_matches("./")).to_lowercase();
    let name = rooted
        .rsplit_once('/')
        .map_or(rooted.as_str(), |(_, name)| name);
    // Python'da `test/` dizini çoğu zaman üretim paketidir (`django/test/`,
    // `unittest`); Python testleri `tests/` dizininde ya da `test_*.py` adındadır.
    let python = name.ends_with(".py");
    ["/tests/", "/test/", "/__tests__/", "/spec/"]
        .iter()
        .filter(|directory| !(python && **directory == "/test/"))
        .any(|directory| rooted.contains(directory))
        || name.starts_with("test_")
        || ["_test.", ".test.", ".spec.", "_spec."]
            .iter()
            .any(|part| name.contains(part))
}

/// `find_usages` özet satırı: ilişki başına kullanım sayısı.
fn usage_summary(target_name: &str, usages: &[ccm_core::graph::Usage]) -> String {
    use ccm_core::graph::UsageRelation;
    let count = |relation: UsageRelation| {
        usages
            .iter()
            .filter(|usage| usage.relation == relation)
            .count()
    };
    format!(
        "{} usages of `{}`: {} calls, {} inferred, {} may call, {} imports, {} inherits",
        usages.len(),
        target_name,
        count(UsageRelation::Calls),
        count(UsageRelation::CallsInferred),
        count(UsageRelation::MayCall),
        count(UsageRelation::Imports),
        count(UsageRelation::Inherits),
    )
}

/// Tool: trace_call_chain
/// from_id'den to_id'ye giden çağrı zincirini BFS ile bulur.
pub async fn trace_call_chain(engine: &Arc<RetrievalEngine>, args: &Value) -> Result<ToolResult> {
    let normalized_from = match engine
        .resolve_target(target_form(args, "from", "from_id")?)
        .await
    {
        Ok(id) => id,
        Err(error) => return Ok(tool_error(error.to_string())),
    };
    let normalized_to = match engine
        .resolve_target(target_form(args, "to", "to_id")?)
        .await
    {
        Ok(id) => id,
        Err(error) => return Ok(tool_error(error.to_string())),
    };
    let max_depth = args
        .get("max_depth")
        .and_then(|v| v.as_u64())
        .map(|value| {
            usize::try_from(value)
                .unwrap_or(MAX_GRAPH_DEPTH)
                .clamp(1, MAX_GRAPH_DEPTH)
        })
        .unwrap_or(8);

    let chain = engine
        .trace_call_chain(&normalized_from, &normalized_to, max_depth)
        .await;
    let from_label = node_label(engine, &normalized_from).await?;
    let to_label = node_label(engine, &normalized_to).await?;

    if chain.is_empty() {
        return Ok(ToolResult {
            content: vec![ToolResultContent {
                content_type: "text".to_string(),
                text: format!(
                    "No call chain from {from_label} to {to_label} within {max_depth} hops."
                ),
            }],
            is_error: None,
        });
    }

    Ok(ToolResult {
        content: vec![ToolResultContent {
            content_type: "text".to_string(),
            text: format!(
                "Call chain {from_label} → {to_label}, {} steps:\n\n{}",
                chain.len(),
                format_suggestions_output(
                    &chain,
                    include_body_from_args(args),
                    max_tokens_from_args(args)?,
                    max_tokens_from_args(args)?,
                )
            ),
        }],
        is_error: None,
    })
}

/// Düğümün kısa etiketi: `ad · yol:satır`. Çözülmüş kimlik indeksten düşmüşse
/// (bu arada yeni indeks yüklendiyse) açık hatadır.
async fn node_label(engine: &Arc<RetrievalEngine>, node_id: &str) -> Result<String> {
    let node = engine.get_node_by_id(node_id).await.ok_or_else(|| {
        anyhow::anyhow!("node '{node_id}' left the index while answering; ask again")
    })?;
    Ok(format!(
        "{} · {}:{}",
        node.name,
        ccm_core::engine::extract_file_path(&node.id).trim_start_matches("./"),
        node.start_line
    ))
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
                    max_tokens_from_args(args)?,
                    max_tokens_from_args(args)?,
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

    let days = args
        .get("days")
        .and_then(|v| v.as_u64())
        .map(|value| value.clamp(1, u64::from(MAX_DIFF_DAYS)) as u32)
        .unwrap_or(7);
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
                    max_tokens_from_args(args)?,
                    max_tokens_from_args(args)?,
                )
            ),
        }],
        is_error: None,
    })
}

#[cfg(test)]
mod tests {
    #[test]
    fn body_head_keeps_short_bodies_and_counts_hidden_lines() {
        assert_eq!(super::body_head("a\nb", 3), ("a\nb".to_string(), 0));
        assert_eq!(super::body_head("a\nb\nc\nd", 2), ("a\nb".to_string(), 2));
    }

    #[test]
    fn this_aliases_are_simple_this_bindings() {
        assert_eq!(super::this_alias_of("  var self = this;"), Some("self"));
        assert_eq!(super::this_alias_of("const that = this"), Some("that"));
        assert_eq!(super::this_alias_of("var self = this.app;"), None);
        assert_eq!(super::this_alias_of("self = this;"), None);
    }

    #[test]
    fn python_test_packages_are_production_code() {
        assert!(!super::is_test_path("django/test/selenium.py"));
        assert!(super::is_test_path("tests/test_basic.py"));
        assert!(super::is_test_path("Lib/test/test_os.py"));
        assert!(super::is_test_path("test/app.router.js"));
    }

    #[test]
    fn index_job_claims_never_return_another_mode_or_join_it() {
        use super::{decide_claim, ClaimDecision, ExistingJob, IndexModeArg};
        let full = IndexModeArg::Full;
        let quick = IndexModeArg::Quick;
        assert_eq!(decide_claim(ExistingJob::None, full), ClaimDecision::Start);
        assert_eq!(
            decide_claim(ExistingJob::Running(full), full),
            ClaimDecision::Join
        );
        assert_eq!(
            decide_claim(ExistingJob::Running(quick), full),
            ClaimDecision::RejectModeConflict(quick)
        );
        assert_eq!(
            decide_claim(ExistingJob::Finished(full), full),
            ClaimDecision::Collect
        );
        assert_eq!(
            decide_claim(ExistingJob::Finished(quick), full),
            ClaimDecision::DiscardAndStart
        );
    }

    use super::{
        index_response_timeout, member_call_hints, normalize_graph_node_id, normalize_graph_path,
        scoped_symbol_hints,
    };
    use std::time::Duration;

    #[test]
    fn explicit_code_hints_do_not_depend_on_english_call_word() {
        let bare = scoped_symbol_hints("Açıkla: `src/flask/` içindeki `ensure_sync`.").remove(0);
        assert_eq!(bare.symbol, "ensure_sync");
        assert_eq!(bare.scope.as_deref(), Some("src/flask/"));

        let qualified =
            scoped_symbol_hints("İncele: `serde_derive/src/` altında `bound::with_bound`.")
                .remove(0);
        assert_eq!(qualified.symbol, "with_bound");
        assert_eq!(qualified.scope.as_deref(), Some("serde_derive/src/"));

        let member =
            member_call_hints("İncele: `lib/request.js` dosyasındaki `req.get`.").remove(0);
        assert_eq!(member.receiver, "req");
        assert_eq!(member.member, "get");
        assert_eq!(member.definition_file.as_deref(), Some("lib/request.js"));
    }

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
