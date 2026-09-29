//! Proje indeksinin tazelik durumu ve okuma sonuçlarına eklenen tazelik satırı.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, watch};

use crate::protocol::{ToolResult, ToolResultContent};
use crate::server::ServerState;

/// Değişiklik olaylarının birleştirildiği sessizlik süresi.
const DEBOUNCE: Duration = Duration::from_millis(300);
/// Başarısız yenileme için toplam deneme sayısı.
const MAX_REFRESH_ATTEMPTS: usize = 3;
/// Denemeler arası bekleme: ilk hatadan sonra 1 sn, ikinciden sonra 2 sn.
const RETRY_DELAYS: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(2)];
/// Tazelik satırına yazılan hata özetinin azami uzunluğu.
const ERROR_SUMMARY_CHARS: usize = 160;

/// Otomatik yenileme için dosya izleyicinin durumu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WatcherStatus {
    /// Watcher çalışıyor; değişiklikler otomatik indekslenir.
    Active,
    /// `CCM_AUTO_REFRESH` ile kapatıldı.
    Disabled,
    /// Watcher açılamadı; sebep her sonuçta gösterilir.
    Unavailable(String),
}

/// Tek bir projenin anlık tazelik durumu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectFreshness {
    pub watcher: WatcherStatus,
    pub pending_paths: usize,
    pub refresh_in_flight: bool,
    /// Hızlı indeksin semantik yükseltmesi sürerken yenileme ertelenir;
    /// yükseltme bitince bekleyen değişiklikler tek seferde işlenir.
    pub waiting_for_upgrade: bool,
    pub last_error: Option<String>,
    pub semantic_unavailable: Option<String>,
}

/// Okumanın beklemesine gerek yoksa `true`: bekleyen iş yoktur ya da iş
/// semantik yükseltme bitene kadar ertelenmiştir (beklemek sonucu değiştirmez).
pub(crate) fn is_settled(freshness: &ProjectFreshness) -> bool {
    freshness.waiting_for_upgrade || (freshness.pending_paths == 0 && !freshness.refresh_in_flight)
}

/// Otomatik yenilemesi olmayan proje için durum.
pub(crate) fn disabled_freshness() -> ProjectFreshness {
    ProjectFreshness {
        watcher: WatcherStatus::Disabled,
        pending_paths: 0,
        refresh_in_flight: false,
        waiting_for_upgrade: false,
        last_error: None,
        semantic_unavailable: None,
    }
}

/// Saniye cinsinden süreyi kısa biçime çevirir (`42s`, `3m`, `2h`, `5d`).
pub(crate) fn format_age(seconds: u64) -> String {
    match seconds {
        0..=59 => format!("{}s", seconds),
        60..=3_599 => format!("{}m", seconds / 60),
        3_600..=86_399 => format!("{}h", seconds / 3_600),
        _ => format!("{}d", seconds / 86_400),
    }
}

/// Okuma sonuçlarının başına eklenen tek satırlık tazelik özetini üretir.
/// Taze ve izlenen bir indekste yaş gösterilmez; yaş yalnızca indeksin diskten
/// geri kalmış olabileceği durumlarda anlamlıdır.
pub(crate) fn format_freshness_line(
    freshness: &ProjectFreshness,
    indexed_at: Option<u64>,
    now_secs: u64,
) -> String {
    let fresh = freshness.watcher == WatcherStatus::Active
        && freshness.pending_paths == 0
        && !freshness.refresh_in_flight
        && freshness.last_error.is_none();
    let mut parts: Vec<String> = Vec::new();
    match &freshness.watcher {
        WatcherStatus::Active if fresh => {
            parts.push("fresh".to_string());
            parts.push("auto-refresh on".to_string());
        }
        WatcherStatus::Active => {
            parts.push("stale".to_string());
            if freshness.pending_paths > 0 {
                let plural = if freshness.pending_paths == 1 {
                    ""
                } else {
                    "s"
                };
                parts.push(format!(
                    "{} changed file{} pending",
                    freshness.pending_paths, plural
                ));
            }
            if freshness.refresh_in_flight {
                parts.push("refresh running".to_string());
            }
            if freshness.waiting_for_upgrade {
                parts.push("waiting for semantic upgrade".to_string());
            }
            if let Some(error) = &freshness.last_error {
                parts.push(format!("last refresh failed: {}", error));
            }
        }
        WatcherStatus::Disabled => parts.push("auto-refresh off".to_string()),
        WatcherStatus::Unavailable(reason) => {
            parts.push(format!("auto-refresh unavailable ({})", reason))
        }
    }
    if !fresh {
        if let Some(indexed_at) = indexed_at {
            parts.push(format!(
                "indexed {} ago",
                format_age(now_secs.saturating_sub(indexed_at))
            ));
        }
    }
    if let Some(reason) = &freshness.semantic_unavailable {
        parts.push(format!("semantic search unavailable: {}", reason));
    }
    format!("_Index: {}_", parts.join(" · "))
}

/// Tazelik satırını ilk metin içeriğinin başına ekler; içerik yoksa tek
/// satırlık metin içeriği oluşturur.
pub(crate) fn with_freshness_line(result: ToolResult, line: &str) -> ToolResult {
    let mut content = result.content;
    match content.first_mut() {
        Some(first) => first.text = format!("{}\n\n{}", line, first.text),
        None => content.push(ToolResultContent {
            content_type: "text".to_string(),
            text: line.to_string(),
        }),
    }
    ToolResult {
        content,
        is_error: result.is_error,
    }
}

/// Bir projenin yayınlanan tazelik durumu ve varsa çalışan watcher'ı.
pub(crate) struct FreshnessHandle {
    pub(crate) state: watch::Sender<ProjectFreshness>,
    /// Yenileme görevine sinyal kanalı; watcher'ı olmayan handle'da `None`.
    signals: Option<mpsc::UnboundedSender<RefreshSignal>>,
    /// Watcher bu handle yaşadıkça çalışır; bırakılırsa izleme durur.
    _watcher: std::sync::Mutex<Option<notify::RecommendedWatcher>>,
}

/// Elle indeksleme sonrası durumun doğrulanması için tam karşılaştırma ister;
/// böylece önceki başarısız yenilemeden kalan hata satırı temizlenir. Watcher'ı
/// olmayan handle'da yapılacak iş yoktur.
pub(crate) fn request_rescan(handle: &FreshnessHandle) {
    if let Some(signals) = &handle.signals {
        if signals.send(RefreshSignal::Rescan).is_err() {
            tracing::warn!("Refresh loop is not running; rescan request was dropped");
        }
    }
}

/// Yenileme görevine giden sinyaller.
enum RefreshSignal {
    /// Filtreden geçen, değişmiş (eklenmiş/silinmiş/yeniden adlandırılmış) yol.
    Changed(PathBuf),
    /// Olay kaybı ihtimali veya başlangıç yakalaması: tam karşılaştırma gerekir.
    Rescan,
    /// Watcher hata bildirdi; olay kaybolmuş olabileceği için yenileme tetiklenir.
    WatchError(String),
}

/// `CCM_AUTO_REFRESH` yalnızca açıkça `0/false/no/off` ise otomatik yenilemeyi kapatır.
pub(crate) fn auto_refresh_enabled() -> bool {
    match std::env::var("CCM_AUTO_REFRESH") {
        Ok(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        ),
        Err(_) => true,
    }
}

/// Watcher'ı olmayan (kapalı veya kullanılamaz) bir handle üretir.
pub(crate) fn inactive_handle(watcher: WatcherStatus) -> Arc<FreshnessHandle> {
    let (state, _) = watch::channel(ProjectFreshness {
        watcher,
        pending_paths: 0,
        refresh_in_flight: false,
        waiting_for_upgrade: false,
        last_error: None,
        semantic_unavailable: None,
    });
    Arc::new(FreshnessHandle {
        state,
        signals: None,
        _watcher: std::sync::Mutex::new(None),
    })
}

/// Hata metnini tazelik satırı için ilk satırına ve sınırlı uzunluğa indirir.
fn summarize_error(message: &str) -> String {
    let first_line = message.lines().next().unwrap_or_default().trim();
    let mut summary: String = first_line.chars().take(ERROR_SUMMARY_CHARS).collect();
    if first_line.chars().count() > ERROR_SUMMARY_CHARS {
        summary.push('…');
    }
    summary
}

/// Proje için watcher'ı ve tek yenileme görevini başlatır; sunucu kapalıyken
/// yapılan değişiklikleri yakalamak için hemen bir tam karşılaştırma ister.
/// Filtre veya watcher kurulamazsa sebebiyle `Unavailable` handle döner.
pub(crate) fn start_auto_refresh(
    server: Arc<ServerState>,
    project_key: String,
    db_path: PathBuf,
) -> Arc<FreshnessHandle> {
    let root = PathBuf::from(&project_key);
    let filter = match ccm_core::build_watch_filter(&root, &db_path) {
        Ok(filter) => filter,
        Err(error) => {
            tracing::warn!(project = %project_key, error = %error, "Auto-refresh unavailable: watch filter could not be built");
            return inactive_handle(WatcherStatus::Unavailable(summarize_error(
                &error.to_string(),
            )));
        }
    };
    let (signals_tx, signals_rx) = mpsc::unbounded_channel();
    let watcher = match start_watcher(filter, &root, signals_tx.clone()) {
        Ok(watcher) => watcher,
        Err(error) => {
            tracing::warn!(project = %project_key, error = %error, "Auto-refresh unavailable: file watcher could not start");
            return inactive_handle(WatcherStatus::Unavailable(summarize_error(
                &error.to_string(),
            )));
        }
    };
    // Başlangıç yakalaması hemen kuyruğa girer; okumalar onu bekler.
    let (state, _) = watch::channel(ProjectFreshness {
        watcher: WatcherStatus::Active,
        pending_paths: 0,
        refresh_in_flight: true,
        waiting_for_upgrade: false,
        last_error: None,
        semantic_unavailable: None,
    });
    let handle = Arc::new(FreshnessHandle {
        state,
        signals: Some(signals_tx.clone()),
        _watcher: std::sync::Mutex::new(Some(watcher)),
    });
    signals_tx
        .send(RefreshSignal::Rescan)
        .expect("refresh loop receiver is owned by this function");
    tokio::spawn(run_refresh_loop(
        server,
        project_key,
        db_path,
        handle.clone(),
        signals_rx,
    ));
    handle
}

/// `notify` watcher'ını açar; filtreden geçen her yol için sinyal yollar.
fn start_watcher(
    filter: ccm_core::WatchFilter,
    root: &Path,
    signals: mpsc::UnboundedSender<RefreshSignal>,
) -> notify::Result<notify::RecommendedWatcher> {
    use notify::Watcher;
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        for signal in signals_for_event(&filter, result) {
            // Alıcı yalnızca yenileme görevi bittiğinde kapanır: sunucu kapanıyordur.
            if signals.send(signal).is_err() {
                return;
            }
        }
    })?;
    watcher.watch(root, notify::RecursiveMode::Recursive)?;
    Ok(watcher)
}

/// Tek bir watcher olayını yenileme sinyallerine çevirir.
fn signals_for_event(
    filter: &ccm_core::WatchFilter,
    result: notify::Result<notify::Event>,
) -> Vec<RefreshSignal> {
    match result {
        Ok(event) if event.need_rescan() => vec![RefreshSignal::Rescan],
        // Erişim olayları içerik değiştirmez; yazmalar ayrıca Modify olarak gelir.
        Ok(event) if matches!(event.kind, notify::EventKind::Access(_)) => Vec::new(),
        Ok(event) => event
            .paths
            .into_iter()
            .filter(|path| ccm_core::is_watch_relevant_path(filter, path))
            .map(RefreshSignal::Changed)
            .collect(),
        Err(error) => vec![RefreshSignal::WatchError(error.to_string())],
    }
}

/// Sinyalin bekleyen değişiklik kümesindeki anahtarı; tam karşılaştırma
/// isteyen sinyaller proje kökünü işaretler.
fn pending_path(signal: &RefreshSignal, root: &Path) -> PathBuf {
    match signal {
        RefreshSignal::Changed(path) => path.clone(),
        RefreshSignal::Rescan | RefreshSignal::WatchError(_) => root.to_path_buf(),
    }
}

/// Watcher hatalarını görünür kılar (yenileme ayrıca tetiklenir).
fn log_signal(project_key: &str, signal: &RefreshSignal) {
    if let RefreshSignal::WatchError(message) = signal {
        tracing::warn!(project = %project_key, error = %message, "File watcher reported an error; scheduling a full refresh");
    }
}

/// Proje başına tek yenileme görevi: olayları debounce eder, worker'ı sırayla
/// çalıştırır ve durumu yayınlar. Kuyruk boşalmadan durum "taze" yayınlanmaz.
async fn run_refresh_loop(
    server: Arc<ServerState>,
    project_key: String,
    db_path: PathBuf,
    handle: Arc<FreshnessHandle>,
    mut signals: mpsc::UnboundedReceiver<RefreshSignal>,
) {
    let root = PathBuf::from(&project_key);
    let mut pending: HashSet<PathBuf> = HashSet::new();
    let mut waiting_for_upgrade = false;
    loop {
        // Bekleyen iş yoksa ya da iş yükseltme yüzünden ertelendiyse yeni sinyal
        // beklenir; yükseltme bitince `end_semantic_upgrade` döngüyü uyandırır.
        if pending.is_empty() || waiting_for_upgrade {
            let Some(signal) = signals.recv().await else {
                return;
            };
            log_signal(&project_key, &signal);
            pending.insert(pending_path(&signal, &root));
            let count = pending.len();
            handle
                .state
                .send_modify(|freshness| freshness.pending_paths = count);
        }
        loop {
            match tokio::time::timeout(DEBOUNCE, signals.recv()).await {
                Ok(Some(signal)) => {
                    log_signal(&project_key, &signal);
                    pending.insert(pending_path(&signal, &root));
                    let count = pending.len();
                    handle
                        .state
                        .send_modify(|freshness| freshness.pending_paths = count);
                }
                Ok(None) => return,
                Err(_) => break,
            }
        }
        // Hızlı indeksin semantik yükseltmesi sürerken `update_index` eksik
        // vektör tablosunu onarmaya ya da tam yeniden indekslemeye girip
        // yükseltmenin embedding işini ikinci kez yapar; yenileme ertelenir.
        waiting_for_upgrade = server.semantic_upgrade_running(&project_key);
        let count = pending.len();
        if waiting_for_upgrade {
            publish_waiting_for_upgrade(&handle, count);
            continue;
        }
        handle.state.send_modify(|freshness| {
            freshness.pending_paths = count;
            freshness.refresh_in_flight = true;
            freshness.waiting_for_upgrade = false;
        });
        let outcome = match refresh_with_retries(&server, &project_key, &db_path).await {
            Ok(RefreshOutcome::Refreshed(stats)) => Ok(*stats),
            // Kilit beklenirken yükseltme kaydolmuş: worker çalışmadığı için bekleyen
            // değişiklikler korunur ve yükseltme bitince tek turda işlenir.
            Ok(RefreshOutcome::Deferred) => {
                waiting_for_upgrade = true;
                publish_waiting_for_upgrade(&handle, count);
                continue;
            }
            Err(error) => Err(error),
        };
        pending.clear();
        // Yenileme sırasında kuyruğa düşen olaylar bir sonraki turu başlatır.
        while let Ok(signal) = signals.try_recv() {
            log_signal(&project_key, &signal);
            pending.insert(pending_path(&signal, &root));
        }
        let queued = pending.len();
        handle.state.send_modify(|freshness| {
            freshness.pending_paths = queued;
            freshness.refresh_in_flight = queued > 0;
            match &outcome {
                Ok(stats) => {
                    freshness.last_error = None;
                    freshness.semantic_unavailable = stats.semantic_unavailable.clone();
                }
                Err(error) => {
                    freshness.last_error = Some(summarize_error(&error.to_string()));
                }
            }
        });
    }
}

/// Bekleyen değişiklikler semantik yükseltme bitene kadar ertelendiğinde durumu
/// yayınlar: okumalar beklemez, tazelik satırı yükseltmeyi gösterir.
fn publish_waiting_for_upgrade(handle: &FreshnessHandle, pending_paths: usize) {
    handle.state.send_modify(|freshness| {
        freshness.pending_paths = pending_paths;
        freshness.refresh_in_flight = false;
        freshness.waiting_for_upgrade = true;
    });
}

/// Tek bir yenileme turunun sonucu.
enum RefreshOutcome {
    /// Worker çalıştı; istatistikleri tazelik durumuna yansıtılır.
    Refreshed(Box<ccm_core::IndexStats>),
    /// Proje kilidi beklenirken hızlı indeksin semantik yükseltmesi kaydoldu;
    /// worker çalıştırılmadı.
    Deferred,
}

/// Worker'ı en fazla üç kez çalıştırır; başarısız denemeleri uyarı olarak
/// log'lar ve 1 sn / 2 sn bekler. Son hata olduğu gibi döner.
async fn refresh_with_retries(
    server: &Arc<ServerState>,
    project_key: &str,
    db_path: &Path,
) -> anyhow::Result<RefreshOutcome> {
    let db_path = db_path.to_string_lossy().to_string();
    let mut attempt = 1;
    loop {
        match refresh_once(server, project_key, &db_path).await {
            Ok(outcome) => return Ok(outcome),
            Err(error) if attempt < MAX_REFRESH_ATTEMPTS => {
                let delay = RETRY_DELAYS[attempt - 1];
                tracing::warn!(
                    project = %project_key,
                    attempt,
                    retry_in_ms = delay.as_millis() as u64,
                    error = %error,
                    "Auto-refresh failed; retrying"
                );
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
            Err(error) => {
                tracing::warn!(
                    project = %project_key,
                    attempt,
                    error = %error,
                    "Auto-refresh failed; the active generation keeps serving reads"
                );
                return Err(error);
            }
        }
    }
}

/// Proje kilidi altında tek artımlı güncelleme çalıştırır ve yeni generation'ı
/// önbelleğe alır (generation değişmediyse `get_engine` önbellekten döner).
/// Semantik yükseltme sürüyorsa worker'ı çalıştırmadan `Deferred` döner.
async fn refresh_once(
    server: &Arc<ServerState>,
    project_key: &str,
    db_path: &str,
) -> anyhow::Result<RefreshOutcome> {
    let lock = server.project_index_lock(project_key);
    let _guard = lock.lock().await;
    // Döngünün yükseltme denetimi kilit beklenirken eskimiş olabilir: `index_now`
    // hızlı indeksi kilit altında bitirip yükseltmeyi kaydeder. Kilit bizdeyken
    // yeni yükseltme başlayamayacağı için bu denetim yarışı kapatır; aksi halde
    // worker yükseltmeyle eşzamanlı çalışıp embedding işini ikiye katlar.
    if server.semantic_upgrade_running(project_key) {
        tracing::info!(
            project = %project_key,
            "Auto-refresh deferred: a semantic upgrade started while waiting for the project lock"
        );
        return Ok(RefreshOutcome::Deferred);
    }
    let stats = crate::tools::run_index_worker_process(
        project_key,
        db_path,
        crate::tools::IndexModeArg::Full,
    )
    .await?;
    server.get_engine(Some(project_key)).await?;
    Ok(RefreshOutcome::Refreshed(Box::new(stats)))
}

/// Süren yenilemenin bitmesini en fazla `budget` kadar bekler; süre dolarsa o
/// anki durumu döndürür (okuma bayat işaretlenir ama hata vermez).
pub(crate) async fn wait_until_fresh(
    handle: &FreshnessHandle,
    budget: Duration,
) -> ProjectFreshness {
    let mut receiver = handle.state.subscribe();
    match tokio::time::timeout(budget, receiver.wait_for(is_settled)).await {
        Ok(Ok(_)) | Err(_) => {}
        Ok(Err(error)) => tracing::warn!(
            error = %error,
            "Freshness channel closed; reporting the last known state"
        ),
    }
    handle.state.borrow().clone()
}
