//! Proje indeksinin tazelik durumu ve okuma sonuçlarına eklenen tazelik satırı.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use ccm_core::live::LiveRefresh;
use tokio::sync::{mpsc, watch};

use crate::protocol::{ToolResult, ToolResultContent};
use crate::server::{EngineSource, ServerState};

/// Değişiklik olaylarının birleştirildiği sessizlik süresi.
const DEBOUNCE: Duration = Duration::from_millis(300);
/// Debounce'un üst sınırı: bekleyen ilk sinyalden bu kadar sonra, olaylar sürse de
/// yenileme turu başlatılır. Aksi halde 300 ms'den sık yazılan bir dosya turu
/// sonsuza dek erteler ve her okuma tam bekleme süresini öder.
const MAX_DEBOUNCE: Duration = Duration::from_secs(2);
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

/// Tam karşılaştırma ister ve bekleyen işi hemen yayınlar: okumalar yenilemeyi
/// bekler ya da bayat raporlanır. İşaret ve sinyal durum kilidi altında birlikte
/// verilir; yenileme döngüsü tur sonu yayınını aynı kilitte yaptığı için bu
/// istek o yayında kaybolmaz. Watcher'ı olmayan handle'da yapılacak iş yoktur.
pub(crate) fn request_rescan(handle: &FreshnessHandle, project_key: &str) {
    let Some(signals) = &handle.signals else {
        return;
    };
    handle.state.send_modify(|freshness| {
        if signals.send(RefreshSignal::Rescan).is_err() {
            tracing::warn!(
                project = %project_key,
                "Refresh loop is not running; rescan request was dropped"
            );
            return;
        }
        freshness.pending_paths = freshness.pending_paths.max(1);
    });
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
        Ok(event) => {
            let kind = event.kind;
            event
                .paths
                .into_iter()
                .filter_map(|path| signal_for_path(filter, &kind, path))
                .collect()
        }
        Err(error) => vec![RefreshSignal::WatchError(error.to_string())],
    }
}

/// Olay yolunun türü: olay türünden ya da diskteki durumdan çıkarılır.
enum EventPath {
    /// Olay türü dizin diyor ya da yol şu an bir dizin.
    Directory,
    /// Dosya; silinmişse olay türü dosya olduğunu söylüyor.
    File,
    /// Yol artık yok ve olay türü (ör. yeniden adlandırma) türünü söylemiyor:
    /// taşınıp giden bir dizin de olabilir.
    Vanished,
}

fn classify_event_path(kind: &notify::EventKind, path: &Path) -> EventPath {
    use notify::event::{CreateKind, RemoveKind};
    use notify::EventKind;
    if matches!(
        kind,
        EventKind::Create(CreateKind::Folder) | EventKind::Remove(RemoveKind::Folder)
    ) {
        return EventPath::Directory;
    }
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => EventPath::Directory,
        Ok(_) => EventPath::File,
        Err(_)
            if matches!(
                kind,
                EventKind::Create(CreateKind::File) | EventKind::Remove(RemoveKind::File)
            ) =>
        {
            EventPath::File
        }
        Err(_) => EventPath::Vanished,
    }
}

/// Tek bir olay yolunun sinyali. Dizinin oluşması, silinmesi ya da taşınması
/// altındaki dosyalar için ayrı olay üretmeyebilir (içeriğiyle taşınan dizin
/// yalnızca kendisi için olay üretir); bu yüzden dizin olayları tam
/// karşılaştırma ister ve dizinlere dosya uzantısı süzgeci uygulanmaz. Türü
/// bilinmeyen kaybolmuş yol, dizin olarak ilgiliyse hedefli taramaya girer:
/// tarama o yolun altındaki bütün indekslenmiş dosyaları düşürür, kaybolan bir
/// ikili dosya içinse bir şey yapmaz (tam karşılaştırmaya gerek kalmaz).
fn signal_for_path(
    filter: &ccm_core::WatchFilter,
    kind: &notify::EventKind,
    path: PathBuf,
) -> Option<RefreshSignal> {
    match classify_event_path(kind, &path) {
        EventPath::Directory => {
            ccm_core::is_watch_relevant_dir(filter, &path).then_some(RefreshSignal::Rescan)
        }
        EventPath::File => {
            ccm_core::is_watch_relevant_path(filter, &path).then_some(RefreshSignal::Changed(path))
        }
        EventPath::Vanished => (ccm_core::is_watch_relevant_path(filter, &path)
            || ccm_core::is_watch_relevant_dir(filter, &path))
        .then_some(RefreshSignal::Changed(path)),
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

/// Proje başına tek yenileme görevi: olayları debounce eder, değişiklikleri
/// canlı engine'e sırayla uygular ve durumu yayınlar; ardından diske yazımı arka
/// planda başlatır. Kuyruk boşalmadan durum "taze" yayınlanmaz.
async fn run_refresh_loop(
    server: Arc<ServerState>,
    project_key: String,
    handle: Arc<FreshnessHandle>,
    mut signals: mpsc::UnboundedReceiver<RefreshSignal>,
) {
    let root = PathBuf::from(&project_key);
    let mut pending: HashSet<PathBuf> = HashSet::new();
    let mut waiting_for_upgrade = false;
    // Başarısız bir tur değişiklikleri uygulayamadan bekleyen kümeyi boşaltır;
    // sonraki tur bu yüzden tam karşılaştırma yapar, kaybolan değişiklik kalmaz.
    let mut rescan_after_failure = false;
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
        let batch_started = tokio::time::Instant::now();
        loop {
            let window = DEBOUNCE.min(MAX_DEBOUNCE.saturating_sub(batch_started.elapsed()));
            if window.is_zero() {
                break;
            }
            match tokio::time::timeout(window, signals.recv()).await {
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
        if rescan_after_failure {
            pending.insert(root.clone());
        }
        let request = refresh_request(&pending, &root);
        let outcome = match refresh_with_retries(&server, &project_key, &request).await {
            Ok(RefreshOutcome::Refreshed { stats, live }) => Ok((stats, live)),
            // Kilit beklenirken yükseltme kaydolmuş: hiçbir şey uygulanmadığı için
            // bekleyen değişiklikler korunur ve yükseltme bitince tek turda işlenir.
            Ok(RefreshOutcome::Deferred) => {
                waiting_for_upgrade = true;
                publish_waiting_for_upgrade(&handle, count);
                continue;
            }
            Err(error) => Err(error),
        };
        rescan_after_failure = outcome.is_err();
        pending.clear();
        handle.state.send_modify(|freshness| {
            // Yenileme sırasında kuyruğa düşen olaylar bir sonraki turu başlatır.
            // Kuyruk durum kilidi altında boşaltılır: `request_rescan` işaretini
            // aynı kilitte koyduğu için arada gelen bir istek silinemez.
            while let Ok(signal) = signals.try_recv() {
                log_signal(&project_key, &signal);
                pending.insert(pending_path(&signal, &root));
            }
            let queued = pending.len();
            freshness.pending_paths = queued;
            freshness.refresh_in_flight = queued > 0;
            match &outcome {
                Ok((stats, _)) => {
                    freshness.last_error = None;
                    freshness.semantic_unavailable = stats.semantic_unavailable.clone();
                }
                Err(error) => {
                    freshness.last_error = Some(summarize_error(&error.to_string()));
                }
            }
        });
        // Taze durum yayınlandıktan sonra diske yazılır; okumalar yazımı beklemez.
        if let Ok((_, live)) = outcome {
            spawn_persist(live, project_key.clone(), handle.clone());
        }
    }
}

/// Bir yenileme turunda uygulanacak iş.
enum RefreshRequest {
    /// Watcher'ın bildirdiği değişmiş yollar.
    Paths(Vec<PathBuf>),
    /// Tam karşılaştırma: başlangıç yakalaması, olay kaybı ya da elle indeksleme sonrası.
    Rescan,
}

/// Bekleyen kümeden turun işini üretir; proje kökü tam karşılaştırma işaretidir.
fn refresh_request(pending: &HashSet<PathBuf>, root: &Path) -> RefreshRequest {
    if pending.contains(root) {
        RefreshRequest::Rescan
    } else {
        RefreshRequest::Paths(pending.iter().cloned().collect())
    }
}

/// Canlı indeksi arka planda etkin generation'a yazar. Yazım hatası tazelik
/// satırına düşer (bellekteki indeks güncel, diskteki geride kalır; sonraki
/// başarılı yazım ya da tam karşılaştırma onu yakalar). Başka bir süreç yeni
/// generation kurduysa canlı durum bırakılır ve yeni generation için tam
/// karşılaştırma istenir: o generation bu durumun uyguladığı değişiklikleri
/// içermeyebilir.
fn spawn_persist(
    live: Arc<ccm_core::live::LiveIndex>,
    project_key: String,
    handle: Arc<FreshnessHandle>,
) {
    tokio::spawn(async move {
        match live.persist().await {
            Ok(ccm_core::live::LivePersist::Persisted)
            | Ok(ccm_core::live::LivePersist::UpToDate) => {}
            Ok(ccm_core::live::LivePersist::Superseded) => {
                tracing::info!(
                    project = %project_key,
                    generation = ?live.generation_id(),
                    "Live index was not persisted: another process activated a new generation; scheduling a full comparison"
                );
                request_rescan(&handle, &project_key);
            }
            Err(error) => {
                tracing::warn!(
                    project = %project_key,
                    error = %error,
                    "Live index could not be persisted; the next full comparison reconciles the index on disk"
                );
                let summary = summarize_error(&format!("index could not be saved: {}", error));
                handle
                    .state
                    .send_modify(|freshness| freshness.last_error = Some(summary));
            }
        }
    });
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

/// Etkin indeks diskten silinmiş: otomatik yenileme indekslenmemiş bir projeyi
/// kendiliğinden tam indekslemez. Kalıcı bir durum olduğu için yeniden denenmez.
#[derive(Debug)]
struct IndexRemovedError;

impl std::fmt::Display for IndexRemovedError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(
            "Project index was removed; auto-refresh does not rebuild it. Call index_project to re-index.",
        )
    }
}

impl std::error::Error for IndexRemovedError {}

/// Tek bir yenileme turunun sonucu.
enum RefreshOutcome {
    /// Değişiklikler canlı engine'e uygulandı; istatistikleri tazelik durumuna
    /// yansıtılır, canlı indeks ardından diske yazılır.
    Refreshed {
        stats: Box<ccm_core::IndexStats>,
        live: Arc<ccm_core::live::LiveIndex>,
    },
    /// Proje kilidi beklenirken hızlı indeksin semantik yükseltmesi kaydoldu;
    /// hiçbir şey uygulanmadı.
    Deferred,
}

/// Yenilemeyi en fazla üç kez dener; başarısız denemeleri uyarı olarak log'lar
/// ve 1 sn / 2 sn bekler. Son hata olduğu gibi döner.
async fn refresh_with_retries(
    server: &Arc<ServerState>,
    project_key: &str,
    request: &RefreshRequest,
) -> anyhow::Result<RefreshOutcome> {
    let mut attempt = 1;
    loop {
        match refresh_once(server, project_key, request).await {
            Ok(outcome) => return Ok(outcome),
            Err(error) if error.is::<IndexRemovedError>() => {
                tracing::warn!(
                    project = %project_key,
                    error = %error,
                    "Auto-refresh skipped: the project index was removed"
                );
                return Err(error);
            }
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

/// Proje kilidi altında değişiklikleri önbellekteki canlı engine'e süreç içinde
/// uygular; worker süreci ve generation kopyası kullanılmaz. Başka bir süreç
/// yeni generation kurduysa o generation yüklenip tam karşılaştırılır: eski canlı
/// durumun uyguladığı ama yeni generation'da olmayan değişiklikler böylece
/// yakalanır. Semantik yükseltme sürüyorsa hiçbir şey uygulamadan `Deferred`
/// döner.
async fn refresh_once(
    server: &Arc<ServerState>,
    project_key: &str,
    request: &RefreshRequest,
) -> anyhow::Result<RefreshOutcome> {
    let lock = server.project_index_lock(project_key);
    let _guard = lock.lock().await;
    // Etkin indeks silinmişse otomatik yenileme onu yeniden kurmaz; yalnızca var
    // olan indeksi günceller.
    if !server.project_index_exists(project_key)? {
        return Err(anyhow::Error::new(IndexRemovedError));
    }
    // Döngünün yükseltme denetimi kilit beklenirken eskimiş olabilir: `index_now`
    // hızlı indeksi kilit altında bitirip yükseltmeyi kaydeder. Kilit bizdeyken
    // yeni yükseltme başlayamayacağı için bu denetim yarışı kapatır; aksi halde
    // yenileme yükseltmenin kurduğu generation'ın yerine eskisini günceller.
    if server.semantic_upgrade_running(project_key) {
        tracing::info!(
            project = %project_key,
            "Auto-refresh deferred: a semantic upgrade started while waiting for the project lock"
        );
        return Ok(RefreshOutcome::Deferred);
    }
    let live = match server.refresh_engine(project_key).await?.source {
        EngineSource::Live(live) => live,
        EngineSource::NeedsMigration { reason, .. } => {
            return migrate_with_worker(server, project_key, &reason).await;
        }
        EngineSource::Rootless => {
            anyhow::bail!("Project '{}' has no index to refresh", project_key)
        }
    };
    // Test kancası canlı indeks yüklendikten sonra bekler: başka bir sürecin
    // generation kurması yükleme ile uygulama arasına düşer.
    apply_test_delay().await?;
    inject_targeted_refresh_failure(request)?;
    match apply_request(&live, request).await? {
        LiveRefresh::Applied(stats) => Ok(RefreshOutcome::Refreshed { stats, live }),
        LiveRefresh::Superseded => {
            tracing::info!(
                project = %project_key,
                generation = ?live.generation_id(),
                "Another process activated a new index generation; reloading it for a full comparison"
            );
            let reloaded = match server.refresh_engine(project_key).await?.source {
                EngineSource::Live(reloaded) => reloaded,
                EngineSource::NeedsMigration { reason, .. } => {
                    return migrate_with_worker(server, project_key, &reason).await;
                }
                EngineSource::Rootless => {
                    anyhow::bail!("Project '{}' has no index to refresh", project_key)
                }
            };
            match reloaded.apply_rescan().await? {
                LiveRefresh::Applied(stats) => Ok(RefreshOutcome::Refreshed {
                    stats,
                    live: reloaded,
                }),
                LiveRefresh::Superseded => anyhow::bail!(
                    "The index generation changed again while it was being reloaded; retrying the refresh"
                ),
            }
        }
    }
}

/// Canlı güncellenemeyen indeksi (eski şema ya da dosya yolu biçimi) worker'ın
/// `update_index`'iyle taşır; o, `ccm-cli index` gibi tam yeniden indeksleyip yeni
/// generation kurar. Ardından yeni generation canlı yüklenir.
async fn migrate_with_worker(
    server: &Arc<ServerState>,
    project_key: &str,
    reason: &ccm_core::live::IndexMigrationRequired,
) -> anyhow::Result<RefreshOutcome> {
    tracing::info!(
        project = %project_key,
        reason = %reason,
        "Auto-refresh migrates the index with the index worker before refreshing it live"
    );
    let db_path = server.project_db_path(project_key)?;
    let stats = crate::tools::run_index_worker_process(
        project_key,
        &db_path.to_string_lossy(),
        crate::tools::IndexModeArg::Full,
    )
    .await?;
    match server.refresh_engine(project_key).await?.source {
        EngineSource::Live(live) => Ok(RefreshOutcome::Refreshed {
            stats: Box::new(stats),
            live,
        }),
        EngineSource::NeedsMigration { reason, .. } => anyhow::bail!(
            "The index still needs a migration after a full update ({}); run index_project",
            reason
        ),
        EngineSource::Rootless => {
            anyhow::bail!("Project '{}' has no index to refresh", project_key)
        }
    }
}

async fn apply_request(
    live: &ccm_core::live::LiveIndex,
    request: &RefreshRequest,
) -> anyhow::Result<LiveRefresh> {
    match request {
        RefreshRequest::Paths(paths) => live.apply_paths(paths).await,
        RefreshRequest::Rescan => live.apply_rescan().await,
    }
}

/// Yavaş bir yenilemeyi taklit eden test kancası
/// (`CCM_INTERNAL_REFRESH_TEST_DELAY_MS`); proje kilidi tutulurken bekler.
async fn apply_test_delay() -> anyhow::Result<()> {
    let Ok(delay) = std::env::var("CCM_INTERNAL_REFRESH_TEST_DELAY_MS") else {
        return Ok(());
    };
    let delay_ms = delay.parse::<u64>().map_err(|error| {
        anyhow::anyhow!(
            "CCM_INTERNAL_REFRESH_TEST_DELAY_MS must be an integer, got '{}': {}",
            delay,
            error
        )
    })?;
    tokio::time::sleep(Duration::from_millis(delay_ms)).await;
    Ok(())
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

/// Test kancası: `CCM_INTERNAL_REFRESH_TEST_FAIL_TARGETED=<n>` ilk `n` hedefli
/// yenileme denemesini tarama hatasıyla düşürür (tam karşılaştırmalar etkilenmez).
fn inject_targeted_refresh_failure(request: &RefreshRequest) -> anyhow::Result<()> {
    static REMAINING: std::sync::OnceLock<std::sync::atomic::AtomicUsize> =
        std::sync::OnceLock::new();
    if !matches!(request, RefreshRequest::Paths(_)) {
        return Ok(());
    }
    let remaining = REMAINING.get_or_init(|| {
        let count = std::env::var("CCM_INTERNAL_REFRESH_TEST_FAIL_TARGETED")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0);
        std::sync::atomic::AtomicUsize::new(count)
    });
    let injected = remaining
        .fetch_update(
            std::sync::atomic::Ordering::SeqCst,
            std::sync::atomic::Ordering::SeqCst,
            |count| count.checked_sub(1),
        )
        .is_ok();
    if injected {
        anyhow::bail!("injected targeted scan failure (CCM_INTERNAL_REFRESH_TEST_FAIL_TARGETED)");
    }
    Ok(())
}
