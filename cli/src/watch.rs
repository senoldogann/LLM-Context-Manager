//! `index --watch` döngüsü: açık bir durum makinesi (`WatchState`), saf geçiş
//! fonksiyonları ve kenarda kalan IO (notify izleyicisi, indeks güncellemesi).
//!
//! Her güncelleme `ccm_core::update_index` çağrısıdır: dosya ağacını manifestle
//! tam olarak karşılaştırır ve yalnızca değişen yolları uygular. İndeks eksik
//! ya da geçersizse (aktif işaretçi bozuk, artefaktlar eksik, şema eski, graf
//! okunamıyor) aynı çağrı belgelenmiş eylem olarak kendisi tam indeks kurar;
//! CLI farklı bir işleme (ör. `index_directory`) sessizce geçmez.

use anyhow::Context;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

/// İzlenen indeksin durumu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchState {
    /// İndeks son karşılaştırılan dosya durumuyla uyumlu.
    Fresh,
    /// İndeks güncellenmeli; `since` ilk bekleyen değişikliğin anı.
    Dirty { since: Instant },
    /// Art arda `attempts` güncelleme denemesi başarısız oldu; `error` sonuncusu.
    Failed { error: String, attempts: u32 },
}

/// Durumu değiştiren olaylar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchEvent {
    /// İlgili bir dosya değişti ya da tam karşılaştırma gerekiyor (rescan, izleme hatası).
    Changed { at: Instant },
    /// `update_index` başarılı oldu.
    UpdateSucceeded,
    /// `update_index` hata verdi.
    UpdateFailed { error: String },
}

/// Bir durumda döngünün yapacağı iş.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchAction {
    /// İndeks güncel; bir sonraki değişikliği bekle.
    AwaitChange,
    /// `delay` bekledikten sonra aynı artımlı güncellemeyi çalıştır.
    Update { delay: Duration },
    /// Deneme hakkı bitti; son hatayı bildir ve çık.
    GiveUp { error: String, attempts: u32 },
}

/// Başarısız güncellemeler için yeniden deneme politikası.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Toplam deneme sayısı (ilk deneme dahil).
    pub max_attempts: u32,
    /// İlk yeniden denemeden önceki bekleme; her denemede iki katına çıkar.
    pub base_delay: Duration,
}

/// `index --watch` için kullanılan politika: 1 s, 2 s, 4 s beklemeyle toplam 4 deneme.
pub const WATCH_RETRY_POLICY: RetryPolicy = RetryPolicy {
    max_attempts: 4,
    base_delay: Duration::from_secs(1),
};

/// Değişiklik algılandıktan sonra olayların durulması için beklenen süre.
const SETTLE_DELAY: Duration = Duration::from_secs(2);

/// Saf geçiş fonksiyonu: mevcut durum ve olaydan yeni durumu üretir.
pub fn next_watch_state(state: &WatchState, event: &WatchEvent) -> WatchState {
    match (state, event) {
        // Bekleyen değişikliğin ilk anı korunur.
        (WatchState::Dirty { since }, WatchEvent::Changed { .. }) => {
            WatchState::Dirty { since: *since }
        }
        (WatchState::Fresh, WatchEvent::Changed { at }) => WatchState::Dirty { since: *at },
        // Yeniden denenen güncelleme zaten tam karşılaştırma yapar; yeni
        // değişiklikler deneme sayacını sıfırlamaz.
        (WatchState::Failed { .. }, WatchEvent::Changed { .. }) => state.clone(),
        (_, WatchEvent::UpdateSucceeded) => WatchState::Fresh,
        (WatchState::Failed { attempts, .. }, WatchEvent::UpdateFailed { error }) => {
            WatchState::Failed {
                error: error.clone(),
                attempts: attempts.saturating_add(1),
            }
        }
        (_, WatchEvent::UpdateFailed { error }) => WatchState::Failed {
            error: error.clone(),
            attempts: 1,
        },
    }
}

/// Saf karar fonksiyonu: durumdan döngünün bir sonraki işini üretir.
pub fn next_watch_action(state: &WatchState, policy: &RetryPolicy) -> WatchAction {
    match state {
        WatchState::Fresh => WatchAction::AwaitChange,
        WatchState::Dirty { .. } => WatchAction::Update {
            delay: Duration::ZERO,
        },
        WatchState::Failed { error, attempts } if *attempts >= policy.max_attempts => {
            WatchAction::GiveUp {
                error: error.clone(),
                attempts: *attempts,
            }
        }
        WatchState::Failed { attempts, .. } => WatchAction::Update {
            delay: retry_delay(*attempts, policy),
        },
    }
}

/// `failed_attempts` başarısız denemeden sonraki bekleme: base * 2^(n-1).
fn retry_delay(failed_attempts: u32, policy: &RetryPolicy) -> Duration {
    let exponent = failed_attempts.saturating_sub(1).min(16);
    policy.base_delay.saturating_mul(1_u32 << exponent)
}

/// Deneme hakkı tükenen izleme döngüsünün hatası.
#[derive(Debug)]
pub struct WatchIndexError {
    pub root: PathBuf,
    pub attempts: u32,
    pub last_error: String,
}

impl std::fmt::Display for WatchIndexError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "index update for '{}' failed {} time(s) in a row; last error: {}. \
             Fix the cause and restart `ccm-cli index --watch`.",
            self.root.display(),
            self.attempts,
            self.last_error
        )
    }
}

impl std::error::Error for WatchIndexError {}

/// İzleyiciden döngüye giden sinyal.
#[derive(Debug, Clone, PartialEq, Eq)]
enum WatchTrigger {
    /// İndekslenen bir dosya değişti.
    FileChanged,
    /// Olaylar kaçmış olabilir; tam karşılaştırma gerekir.
    Rescan { reason: String },
}

/// Bir notify olayını sinyale çevirir; ilgisiz olaylar için `None`.
///
/// Rescan/taşma olaylarının yolu yoktur ya da yol proje köküdür; bunlar
/// `is_index_relevant_file` filtresinden geçemez, bu yüzden önce ayrıca ele alınır.
fn classify_event(root: &Path, event: &notify::Event) -> Option<WatchTrigger> {
    if event.need_rescan() {
        let reason = event
            .info()
            .map_or("notify requested a rescan", |info| info)
            .to_string();
        return Some(WatchTrigger::Rescan { reason });
    }
    if event.paths.is_empty() {
        return Some(WatchTrigger::Rescan {
            reason: format!("event without paths ({:?})", event.kind),
        });
    }
    if event.paths.iter().any(|path| path == root) {
        return Some(WatchTrigger::Rescan {
            reason: format!("event on the project root ({:?})", event.kind),
        });
    }
    let relevant = event.paths.iter().any(|path| {
        // Araç durum dizinleri indexleyici kapsamı dışında.
        let tool_state_dir = path.components().any(|component| {
            matches!(
                component.as_os_str().to_string_lossy().as_ref(),
                ".ccm" | ".agent"
            )
        });
        !tool_state_dir && ccm_core::is_index_relevant_file(root, path)
    });
    relevant.then_some(WatchTrigger::FileChanged)
}

/// notify sonucunu sinyale çevirir; izleme hataları da tam karşılaştırma ister.
fn trigger_for(root: &Path, result: &notify::Result<notify::Event>) -> Option<WatchTrigger> {
    match result {
        Ok(event) => classify_event(root, event),
        Err(error) => Some(WatchTrigger::Rescan {
            reason: format!("watch error: {error}"),
        }),
    }
}

/// Sinyali kanala bırakır. Kanal doluysa zaten bekleyen bir güncelleme vardır ve
/// her sinyal aynı sonucu (Dirty) doğurur; bu sinyal onunla birleşir.
fn send_trigger(sender: &mpsc::Sender<WatchTrigger>, trigger: WatchTrigger) {
    if let WatchTrigger::Rescan { reason } = &trigger {
        tracing::warn!(reason = %reason, "Watcher may have missed events; scheduling a full comparison");
    }
    match sender.try_send(trigger) {
        Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => {}
        // Alıcı yalnızca izleme döngüsü bitip süreç kapanırken düşer.
        Err(mpsc::error::TrySendError::Closed(_)) => {
            tracing::debug!("Watch loop has stopped; dropping file-system event");
        }
    }
}

/// İndeks istatistiklerini yapılandırılmış alanlarla loglar.
pub fn log_index_stats(stats: &ccm_core::IndexStats) {
    tracing::info!(
        indexed = stats.files_indexed,
        failed = stats.files_failed,
        skipped = stats.files_skipped,
        nodes = stats.nodes_created,
        embedded_chunks = stats.embedded_chunks,
        reused_chunks = stats.reused_chunks,
        "Indexing complete"
    );
    for (reason, count) in &stats.reason_counts {
        tracing::info!(reason = %reason, count = *count, "Index issue summary");
    }
    if let Some(reason) = &stats.semantic_unavailable {
        tracing::warn!(
            reason = %reason,
            "Graph index is ready; semantic search stays off until the reason is resolved and the project is indexed again"
        );
    }
}

/// Bir artımlı güncelleme çalıştırır ve sonucunu olaya çevirir.
async fn run_update(path: &str, db_path: Option<&str>) -> WatchEvent {
    match ccm_core::update_index(path, db_path).await {
        Ok(stats) => {
            log_index_stats(&stats);
            WatchEvent::UpdateSucceeded
        }
        Err(error) => WatchEvent::UpdateFailed {
            error: format!("{error:#}"),
        },
    }
}

/// Bir değişiklik sinyalini bekler, durulma süresi boyunca gelenleri birleştirir.
async fn await_change(receiver: &mut mpsc::Receiver<WatchTrigger>) -> anyhow::Result<Instant> {
    let trigger = receiver
        .recv()
        .await
        .context("file-system watcher stopped unexpectedly (event channel closed)")?;
    let at = Instant::now();
    tracing::info!(trigger = ?trigger, "Change detected; waiting for events to settle");
    tokio::time::sleep(SETTLE_DELAY).await;
    // Durulma süresinde gelen sinyaller aynı güncellemeye dahil.
    while receiver.try_recv().is_ok() {}
    Ok(at)
}

/// İzleyiciyi başlatır, ilk indeksi kurar ve değişiklikleri izler.
///
/// İzleyici ilk indeksten önce başlar; ilk indeks sırasında olan değişiklikler
/// kaçmaz. İlk indeks ve sonraki her güncelleme aynı politikayla yeniden denenir;
/// deneme hakkı biterse son hata `WatchIndexError` olarak döner ve süreç biter.
pub async fn watch_and_index(
    root: &Path,
    db_path: Option<&str>,
    policy: &RetryPolicy,
) -> anyhow::Result<()> {
    use notify::Watcher;

    let canonical_root = std::fs::canonicalize(root)
        .with_context(|| format!("project root '{}' could not be resolved", root.display()))?;
    let path_str = root.to_string_lossy().to_string();
    let (sender, mut receiver) = mpsc::channel::<WatchTrigger>(1);
    let event_root = canonical_root.clone();
    let mut watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        if let Some(trigger) = trigger_for(&event_root, &result) {
            send_trigger(&sender, trigger);
        }
    })
    .context("file-system watcher could not be created")?;
    watcher
        .watch(&canonical_root, notify::RecursiveMode::Recursive)
        .with_context(|| format!("could not watch '{}'", canonical_root.display()))?;
    tracing::info!(path = %canonical_root.display(), "Watching for changes");

    let mut state = WatchState::Dirty {
        since: Instant::now(),
    };
    loop {
        state = match next_watch_action(&state, policy) {
            WatchAction::AwaitChange => {
                let at = await_change(&mut receiver).await?;
                next_watch_state(&state, &WatchEvent::Changed { at })
            }
            WatchAction::Update { delay } => {
                if !delay.is_zero() {
                    tracing::info!(delay_ms = delay.as_millis() as u64, "Retrying index update");
                    tokio::time::sleep(delay).await;
                }
                if let WatchState::Dirty { since } = &state {
                    tracing::info!(
                        pending_ms = since.elapsed().as_millis() as u64,
                        "Updating index"
                    );
                }
                let event = run_update(&path_str, db_path).await;
                let next = next_watch_state(&state, &event);
                if let WatchState::Failed { error, attempts } = &next {
                    tracing::warn!(
                        attempt = *attempts,
                        max_attempts = policy.max_attempts,
                        error = %error,
                        "Index update failed"
                    );
                }
                next
            }
            WatchAction::GiveUp { error, attempts } => {
                return Err(WatchIndexError {
                    root: canonical_root,
                    attempts,
                    last_error: error,
                }
                .into());
            }
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POLICY: RetryPolicy = RetryPolicy {
        max_attempts: 3,
        base_delay: Duration::from_secs(1),
    };

    fn failed(attempts: u32) -> WatchState {
        WatchState::Failed {
            error: "boom".to_string(),
            attempts,
        }
    }

    #[test]
    fn change_marks_fresh_index_dirty_and_keeps_first_change_time() {
        let first = Instant::now();
        let later = first + Duration::from_secs(5);
        let dirty = next_watch_state(&WatchState::Fresh, &WatchEvent::Changed { at: first });
        assert_eq!(dirty, WatchState::Dirty { since: first });
        assert_eq!(
            next_watch_state(&dirty, &WatchEvent::Changed { at: later }),
            WatchState::Dirty { since: first }
        );
        assert_eq!(
            next_watch_action(&dirty, &POLICY),
            WatchAction::Update {
                delay: Duration::ZERO
            }
        );
    }

    #[test]
    fn failures_retry_with_backoff_then_give_up_with_last_error() {
        let dirty = WatchState::Dirty {
            since: Instant::now(),
        };
        let first = next_watch_state(
            &dirty,
            &WatchEvent::UpdateFailed {
                error: "first".to_string(),
            },
        );
        assert_eq!(
            next_watch_action(&first, &POLICY),
            WatchAction::Update {
                delay: Duration::from_secs(1)
            }
        );
        let second = next_watch_state(
            &first,
            &WatchEvent::UpdateFailed {
                error: "second".to_string(),
            },
        );
        assert_eq!(
            next_watch_action(&second, &POLICY),
            WatchAction::Update {
                delay: Duration::from_secs(2)
            }
        );
        let third = next_watch_state(
            &second,
            &WatchEvent::UpdateFailed {
                error: "third".to_string(),
            },
        );
        assert_eq!(
            next_watch_action(&third, &POLICY),
            WatchAction::GiveUp {
                error: "third".to_string(),
                attempts: 3
            }
        );
    }

    #[test]
    fn success_resets_failures_and_changes_do_not_reset_the_budget() {
        assert_eq!(
            next_watch_state(&failed(2), &WatchEvent::UpdateSucceeded),
            WatchState::Fresh
        );
        assert_eq!(
            next_watch_state(&failed(2), &WatchEvent::Changed { at: Instant::now() }),
            failed(2)
        );
        assert_eq!(
            next_watch_action(&WatchState::Fresh, &POLICY),
            WatchAction::AwaitChange
        );
    }
}
