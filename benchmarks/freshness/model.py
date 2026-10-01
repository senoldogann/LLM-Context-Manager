"""L1 tazelik ölçümünün paylaşılan tipleri: cevaplar, sınıflar ve kayıtlar."""

from __future__ import annotations

from dataclasses import dataclass
from enum import StrEnum
from pathlib import Path
from typing import Protocol, TypeAlias

JsonValue: TypeAlias = None | bool | int | float | str | list["JsonValue"] | dict[str, "JsonValue"]


class AdapterError(RuntimeError):
    """Bir adapter sistemi indeksleyemedi, yeniden indeksleyemedi ya da başlatamadı."""


class Verdict(StrEnum):
    """Bir probun tek sınıfı (bkz. PREREGISTRATION.md)."""

    CORRECT = "CORRECT"
    STALE_SILENT = "STALE_SILENT"
    STALE_LABELED = "STALE_LABELED"
    STALE_STRUCTURED_ONLY = "STALE_STRUCTURED_ONLY"
    ERROR_EMPTY = "ERROR_EMPTY"
    PRESERVED_LABELED = "PRESERVED_LABELED"
    PRESERVED_SILENT = "PRESERVED_SILENT"
    LOST = "LOST"


class LabelChannel(StrEnum):
    """Sistemin bayatlık işaretinin göründüğü kanal."""

    CONTENT_TEXT = "content_text"
    STRUCTURED_ONLY = "structured_only"
    NONE = "none"


@dataclass(frozen=True)
class LabelObservation:
    """Bir probun aldığı yanıtlardaki bayatlık işareti ve ham durum satırı."""

    channel: LabelChannel
    text: str


@dataclass(frozen=True)
class CallersAnswer:
    """`callers(symbol)` sorusunun cevabı; hata yoksa `error` boştur."""

    target_found: bool
    callers: frozenset[str]
    label: LabelObservation
    error: str


@dataclass(frozen=True)
class ExistsAnswer:
    """`exists(symbol)` sorusunun cevabı ve sembolün bulunduğu dosyalar."""

    found: bool
    files: frozenset[str]
    label: LabelObservation
    error: str


@dataclass(frozen=True)
class NodeAnswer:
    """`node_at(file, line)` cevabı; düğüm yoksa `name` boştur."""

    name: str
    start_line: int
    end_line: int
    label: LabelObservation
    error: str


@dataclass(frozen=True)
class ProbeOutcome:
    """Bir probun sınıflandırılmış sonucu."""

    verdict: Verdict
    label: LabelObservation
    partial: bool
    reflected_count: int | None
    detail: str


@dataclass(frozen=True)
class ProbeRecord:
    """Ham sonuç dosyasına yazılan tek prob."""

    phase: str
    t_target_s: float
    t_actual_s: float
    duration_s: float
    verdict: Verdict
    label_channel: LabelChannel
    label_text: str
    partial: bool
    reflected_count: int | None
    detail: str


@dataclass(frozen=True)
class RunRecord:
    """Bir sistem × senaryo × repo × tekrar koşusunun tüm probları."""

    scenario: str
    repo: str
    repetition: int
    index_seconds: float | None
    ready_seconds: float | None
    baseline_ok: bool
    baseline_detail: str
    probes: tuple[ProbeRecord, ...]
    crashed: bool
    error: str


@dataclass(frozen=True)
class ArtifactInfo:
    """Ölçülen sistemin bir ikilisi ya da paketi; yol yerine ad ve özet tutulur."""

    name: str
    sha256: str


@dataclass(frozen=True)
class SystemInfo:
    """Ölçülen sistemin kimliği; sonuç dosyasının başlığına yazılır."""

    name: str
    version: str
    artifacts: tuple[ArtifactInfo, ...]


class Session(Protocol):
    """Açık bir MCP oturumu üzerinden sisteme sorulan sistemden bağımsız sorular."""

    def callers(self, symbol: str) -> CallersAnswer:
        """`symbol`'ü çağıran fonksiyon/metot adları."""
        ...

    def exists(self, symbol: str) -> ExistsAnswer:
        """`symbol` adlı fonksiyon/metot var mı ve hangi dosyalarda?"""
        ...

    def node_at(self, relative_file: str, line: int) -> NodeAnswer:
        """Konumdaki en içteki fonksiyon/metot."""
        ...

    def alive(self) -> bool:
        """Sunucu süreci hâlâ çalışıyor mu?"""
        ...

    def close(self) -> None:
        """Oturumu kapatır."""
        ...


class Adapter(Protocol):
    """Bir sistemi indeksleyen ve oturum açan bağlayıcı."""

    def describe(self) -> SystemInfo:
        """Sistemin adı, sürümü ve ikili özeti."""
        ...

    def build_index(self, repo: Path) -> float:
        """İlk indeksi süreç dışında kurar; süreyi saniye olarak döndürür."""
        ...

    def reindex(self, repo: Path) -> float:
        """Sistemin kendi yeniden indeksleme komutunu süreç dışında çalıştırır."""
        ...

    def open_session(self, repo: Path, auto_refresh: bool, log_name: str) -> Session:
        """MCP sunucusunu başlatır ve el sıkışmasını yapar."""
        ...
