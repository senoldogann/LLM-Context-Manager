//! Proje indeksinin tazelik durumu ve okuma sonuçlarına eklenen tazelik satırı.

use crate::protocol::{ToolResult, ToolResultContent};

/// Otomatik yenileme için dosya izleyicinin durumu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WatcherStatus {
    /// Watcher çalışıyor; değişiklikler otomatik indekslenir.
    Active,
    /// `CCM_AUTO_REFRESH` ile kapatıldı.
    Disabled,
    /// Watcher açılamadı; sebep her sonuçta gösterilir.
    // Bu varyantı yalnızca izleyici kurulumu üretir; izleyici bağlanana kadar kurulmaz.
    #[allow(dead_code)]
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
// Okuma öncesi bekleme adımı izleyiciyle birlikte bağlanır; o zamana kadar çağrılmaz.
#[allow(dead_code)]
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
