"""CCM adapter'ı: ilk indeks, MCP oturumu ve PREREGISTRATION.md'deki araç eşlemesi."""

from __future__ import annotations

import hashlib
import logging
import re
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path

from freshness.logs import log_event
from freshness.mcp import McpClient, McpError, ServerSpec, ToolResponse
from freshness.model import (
    AdapterError,
    ArtifactInfo,
    CallersAnswer,
    ExistsAnswer,
    JsonValue,
    LabelChannel,
    LabelObservation,
    NodeAnswer,
    SystemInfo,
)

LOGGER = logging.getLogger("freshness.ccm")

PROTOCOL_VERSION = "2025-06-18"
# Ön-kayıtlı etiket kuralı: durum satırında bunlardan biri varsa prob etiketlidir.
STALE_MARKERS: tuple[str, ...] = (
    "stale",
    "pending",
    "refresh running",
    "refresh failed",
    "auto-refresh off",
    "auto-refresh unavailable",
)
FUNCTION_TYPES: frozenset[str] = frozenset({"Function", "Method"})
# Tür birden çok kelime olabilir (ör. get_context'in `## Active element: …` bloğu).
HEADING = re.compile(r"^## (?P<kind>[A-Za-z][A-Za-z ]*): (?P<name>.+?) \(Score: [^)]*\)\s*$")
FIELD = re.compile(r"^\*\*(?P<key>[A-Za-z ]+):\*\* (?P<value>.*)$")
RANGE = re.compile(r"(?P<start>\d+)-(?P<end>\d+)")
STATUS = re.compile(r"_Index: (?P<status>.*)_")


class CcmCommandError(AdapterError):
    """Bir `ccm-cli` komutu tüm denemelerde başarısız oldu."""


@dataclass(frozen=True)
class CcmConfig:
    """CCM ikilileri ve izole çalışma ortamı."""

    bin_dir: Path
    home: Path
    log_dir: Path
    request_timeout_s: float
    command_timeout_s: float
    command_attempts: int


@dataclass(frozen=True)
class NodeBlock:
    """CCM yanıtındaki tek bir düğüm bloğu."""

    kind: str
    name: str
    node_id: str
    file: str
    node_type: str
    start_line: int
    end_line: int


def node_block(fields: dict[str, str]) -> NodeBlock:
    """Toplanan alanlardan düğüm bloğu kurar; aralık yoksa 0-0 kalır."""
    span = RANGE.fullmatch(fields.get("Range", ""))
    return NodeBlock(
        kind=fields["kind"],
        name=fields["name"],
        node_id=fields.get("Node ID", ""),
        file=fields.get("File", "").removeprefix("./"),
        node_type=fields.get("Node Type", ""),
        start_line=int(span["start"]) if span else 0,
        end_line=int(span["end"]) if span else 0,
    )


def parse_blocks(text: str) -> tuple[NodeBlock, ...]:
    """`## Tür: ad (Score: …)` başlıklı blokları sırasıyla ayrıştırır."""
    blocks: list[NodeBlock] = []
    current: dict[str, str] | None = None
    for line in text.splitlines():
        heading = HEADING.match(line)
        if heading is not None:
            if current is not None:
                blocks.append(node_block(current))
            current = {"kind": heading["kind"], "name": heading["name"]}
            continue
        field = FIELD.match(line)
        if field is not None and current is not None:
            current[field["key"]] = field["value"].strip()
    if current is not None:
        blocks.append(node_block(current))
    return tuple(blocks)


def status_line(response: ToolResponse) -> str:
    """İlk metin içeriğinin `_Index: …_` durum satırı; yoksa boş."""
    if not response.texts or not response.texts[0]:
        return ""
    match = STATUS.fullmatch(response.texts[0].splitlines()[0].strip())
    return match["status"] if match else ""


def label_from(responses: tuple[ToolResponse, ...]) -> LabelObservation:
    """Probun aldığı tüm yanıtlardan bayatlık işaretini ve kanalını çıkarır."""
    statuses = tuple(status_line(response) for response in responses)
    text = " | ".join(dict.fromkeys(status for status in statuses if status))
    if any(marker in status for status in statuses for marker in STALE_MARKERS):
        return LabelObservation(channel=LabelChannel.CONTENT_TEXT, text=text)
    if any(marker in r.structured_json for r in responses for marker in STALE_MARKERS):
        return LabelObservation(channel=LabelChannel.STRUCTURED_ONLY, text=text)
    return LabelObservation(channel=LabelChannel.NONE, text=text)


def joined_text(response: ToolResponse) -> str:
    """Yanıtın tüm metin içerikleri."""
    return "\n".join(response.texts)


def error_text(response: ToolResponse) -> str:
    """Hatalı araç yanıtının kısaltılmış metni."""
    return joined_text(response)[:300]


def function_blocks(response: ToolResponse, symbol: str) -> tuple[NodeBlock, ...]:
    """Yanıttaki, adı tam olarak `symbol` olan fonksiyon/metot blokları."""
    return tuple(
        block
        for block in parse_blocks(joined_text(response))
        if block.name == symbol and block.node_type in FUNCTION_TYPES
    )


def run_command(
    argv: tuple[str, ...],
    cwd: Path,
    env: dict[str, str],
    attempts: int,
    timeout_s: float,
) -> float:
    """Komutu çalıştırır; başarısızsa uyarıyla yeniden dener, sonunda son hatayı verir."""
    if attempts < 1:
        raise ValueError(f"attempts must be at least 1, got {attempts}")
    last_error = CcmCommandError(f"{argv[0]} was not run")
    for attempt in range(1, attempts + 1):
        started = time.monotonic()
        try:
            completed = subprocess.run(
                list(argv),
                cwd=cwd,
                env=env,
                capture_output=True,
                text=True,
                timeout=timeout_s,
                check=False,
            )
        except subprocess.TimeoutExpired:
            last_error = CcmCommandError(f"{' '.join(argv)} timed out after {timeout_s}s in {cwd}")
            log_event(
                LOGGER,
                logging.WARNING,
                "command_timed_out",
                {"argv": list(argv), "attempt": attempt, "timeout_s": timeout_s},
            )
            continue
        elapsed = time.monotonic() - started
        if completed.returncode == 0:
            return elapsed
        last_error = CcmCommandError(
            f"{' '.join(argv)} exited {completed.returncode} in {cwd}: "
            f"{completed.stderr.strip()[-1500:]}"
        )
        log_event(
            LOGGER,
            logging.WARNING,
            "command_failed",
            {"argv": list(argv), "attempt": attempt, "returncode": completed.returncode},
        )
    raise last_error


def sha256_of(path: Path) -> str:
    """Dosyanın SHA-256 özeti."""
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


class CcmSession:
    """CCM MCP oturumu üzerinden sistemden bağımsız soruları cevaplar."""

    def __init__(self, client: McpClient, repo: Path, timeout_s: float) -> None:
        self._client = client
        self._repo = repo
        self._timeout_s = timeout_s

    def _call(self, tool: str, arguments: dict[str, JsonValue]) -> ToolResponse:
        return self._client.call_tool(
            tool, {**arguments, "project_path": str(self._repo)}, self._timeout_s
        )

    def callers(self, symbol: str) -> CallersAnswer:
        """`find_nodes` ile düğümü bulur, `find_usages` ile çağıranları toplar."""
        try:
            found = self._call("find_nodes", {"query": symbol, "limit": 50})
            if found.is_error:
                return CallersAnswer(
                    target_found=False,
                    callers=frozenset(),
                    label=label_from((found,)),
                    error=f"find_nodes: {error_text(found)}",
                )
            targets = function_blocks(found, symbol)
            if not targets:
                return CallersAnswer(
                    target_found=False, callers=frozenset(), label=label_from((found,)), error=""
                )
            usages = self._call("find_usages", {"node_id": targets[0].node_id, "limit": 200})
            label = label_from((found, usages))
            if usages.is_error:
                return CallersAnswer(
                    target_found=True,
                    callers=frozenset(),
                    label=label,
                    error=f"find_usages: {error_text(usages)}",
                )
            names = frozenset(block.name for block in parse_blocks(joined_text(usages)))
            return CallersAnswer(target_found=True, callers=names, label=label, error="")
        except McpError as error:
            return CallersAnswer(
                target_found=False,
                callers=frozenset(),
                label=LabelObservation(channel=LabelChannel.NONE, text=""),
                error=f"{type(error).__name__}: {error}",
            )

    def exists(self, symbol: str) -> ExistsAnswer:
        """`find_nodes` sonucunda adı tam eşleşen fonksiyon/metot var mı?"""
        try:
            found = self._call("find_nodes", {"query": symbol, "limit": 50})
            if found.is_error:
                return ExistsAnswer(
                    found=False,
                    files=frozenset(),
                    label=label_from((found,)),
                    error=f"find_nodes: {error_text(found)}",
                )
            blocks = function_blocks(found, symbol)
            return ExistsAnswer(
                found=bool(blocks),
                files=frozenset(block.file for block in blocks),
                label=label_from((found,)),
                error="",
            )
        except McpError as error:
            return ExistsAnswer(
                found=False,
                files=frozenset(),
                label=LabelObservation(channel=LabelChannel.NONE, text=""),
                error=f"{type(error).__name__}: {error}",
            )

    def node_at(self, relative_file: str, line: int) -> NodeAnswer:
        """`get_context` yanıtındaki `## Current:` bloğu."""
        try:
            response = self._call("get_context", {"file": relative_file, "line": line})
            label = label_from((response,))
            if response.is_error:
                return NodeAnswer(
                    name="",
                    start_line=0,
                    end_line=0,
                    label=label,
                    error=f"get_context: {error_text(response)}",
                )
            current = tuple(
                block for block in parse_blocks(joined_text(response)) if block.kind == "Current"
            )
            if not current:
                return NodeAnswer(name="", start_line=0, end_line=0, label=label, error="")
            return NodeAnswer(
                name=current[0].name,
                start_line=current[0].start_line,
                end_line=current[0].end_line,
                label=label,
                error="",
            )
        except McpError as error:
            return NodeAnswer(
                name="",
                start_line=0,
                end_line=0,
                label=LabelObservation(channel=LabelChannel.NONE, text=""),
                error=f"{type(error).__name__}: {error}",
            )

    def alive(self) -> bool:
        """Sunucu süreci hâlâ çalışıyor mu?"""
        return self._client.alive()

    def close(self) -> None:
        """Oturumu kapatır."""
        self._client.close(10.0)


class CcmAdapter:
    """CCM ikililerini izole bir ortamda çalıştıran bağlayıcı."""

    def __init__(self, config: CcmConfig) -> None:
        self._config = config

    def _env(self, repo: Path, auto_refresh: bool) -> dict[str, str]:
        # Yalnız gerekli değişkenler: ev dizinindeki ayarlar ve sırlar okunmaz.
        return {
            "PATH": "/usr/bin:/bin:/usr/sbin:/sbin",
            "HOME": str(self._config.home),
            "CCM_DISABLE_EMBEDDER": "1",
            "CCM_PROJECT_ROOT": str(repo),
            "CCM_ALLOWED_ROOTS": str(repo),
            "CCM_MCP_DEBUG": "0",
            "CCM_AUTO_REFRESH": "1" if auto_refresh else "0",
            "RUST_LOG": "warn",
        }

    def describe(self) -> SystemInfo:
        """`ccm-cli --version` ve `ccm-mcp` ikilisinin özeti."""
        cli = self._config.bin_dir / "ccm-cli"
        server = self._config.bin_dir / "ccm-mcp"
        completed = subprocess.run(
            [str(cli), "--version"],
            capture_output=True,
            text=True,
            timeout=self._config.command_timeout_s,
            check=False,
            env={"PATH": "/usr/bin:/bin", "HOME": str(self._config.home)},
        )
        if completed.returncode != 0:
            raise CcmCommandError(f"{cli} --version exited {completed.returncode}")
        return SystemInfo(
            name="ccm",
            version=completed.stdout.strip(),
            artifacts=(
                ArtifactInfo(name=cli.name, sha256=sha256_of(cli)),
                ArtifactInfo(name=server.name, sha256=sha256_of(server)),
            ),
        )

    def build_index(self, repo: Path) -> float:
        """İlk indeksi `ccm-cli index` ile senkron kurar."""
        return run_command(
            (str(self._config.bin_dir / "ccm-cli"), "index", "--path", str(repo)),
            repo,
            self._env(repo, True),
            self._config.command_attempts,
            self._config.command_timeout_s,
        )

    def reindex(self, repo: Path) -> float:
        """S2: aynı komutla süreç dışı yeniden indeksleme."""
        return self.build_index(repo)

    def open_session(self, repo: Path, auto_refresh: bool, log_name: str) -> CcmSession:
        """`ccm-mcp` sürecini başlatır ve MCP el sıkışmasını yapar."""
        spec = ServerSpec(
            argv=(str(self._config.bin_dir / "ccm-mcp"),),
            cwd=repo,
            env=tuple(sorted(self._env(repo, auto_refresh).items())),
            roots=(repo,),
            stderr_log=self._config.log_dir / f"{log_name}.stderr.log",
        )
        client = McpClient(spec)
        try:
            client.initialize(PROTOCOL_VERSION, self._config.request_timeout_s)
        except McpError:
            client.close(10.0)
            raise
        return CcmSession(client, repo, self._config.request_timeout_s)
