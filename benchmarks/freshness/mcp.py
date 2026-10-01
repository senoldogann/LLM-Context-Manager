"""MCP stdio sunucularıyla konuşan tipli, minimal JSON-RPC istemcisi (bağlayıcı)."""

from __future__ import annotations

import json
import logging
import queue
import subprocess
import threading
import time
from dataclasses import dataclass
from pathlib import Path
from typing import TextIO

from freshness.jsonio import as_list, as_object, as_str
from freshness.logs import log_event
from freshness.model import JsonValue

LOGGER = logging.getLogger("freshness.mcp")


class McpError(Exception):
    """MCP oturumundaki tüm hataların tabanı."""


class McpProcessExitedError(McpError):
    """Sunucu süreci beklenmedik biçimde sonlandı."""


class McpTimeoutError(McpError):
    """Bir isteğin cevabı süresi içinde gelmedi."""


class McpProtocolError(McpError):
    """Sunucu JSON-RPC ya da MCP sözleşmesine uymayan bir mesaj gönderdi."""


def parse_message(line: str) -> dict[str, JsonValue]:
    """Sunucudan gelen bir satırı JSON-RPC mesajına çevirir."""
    try:
        value: JsonValue = json.loads(line)
    except json.JSONDecodeError as error:
        raise McpProtocolError(f"invalid JSON from server: {line[:200]!r}") from error
    return as_object(value, "message", McpProtocolError)


@dataclass(frozen=True)
class ServerSpec:
    """Bir MCP sunucusunun nasıl başlatılacağı; ortam açıkça verilir."""

    argv: tuple[str, ...]
    cwd: Path
    env: tuple[tuple[str, str], ...]
    roots: tuple[Path, ...]
    stderr_log: Path


@dataclass(frozen=True)
class ToolResponse:
    """`tools/call` cevabı: model tarafından görülen metinler ve yapılandırılmış içerik."""

    texts: tuple[str, ...]
    structured_json: str
    is_error: bool
    duration_s: float


def tool_response(result: dict[str, JsonValue], duration_s: float) -> ToolResponse:
    """MCP `CallToolResult` nesnesini tipli cevaba çevirir."""
    if "content" not in result:
        raise McpProtocolError("tools/call result has no content")
    texts: list[str] = []
    for index, item in enumerate(as_list(result["content"], "content", McpProtocolError)):
        entry = as_object(item, f"content[{index}]", McpProtocolError)
        if entry.get("type") == "text":
            texts.append(as_str(entry.get("text"), f"content[{index}].text", McpProtocolError))
    structured = (
        json.dumps(result["structuredContent"], ensure_ascii=False)
        if "structuredContent" in result
        else ""
    )
    return ToolResponse(
        texts=tuple(texts),
        structured_json=structured,
        is_error=result.get("isError") is True,
        duration_s=duration_s,
    )


class McpClient:
    """Tek bir MCP stdio sunucu sürecini yönetir ve sıralı istek gönderir."""

    def __init__(self, spec: ServerSpec) -> None:
        self._spec = spec
        self._stderr: TextIO = spec.stderr_log.open("w", encoding="utf-8")
        self._process = subprocess.Popen(
            list(spec.argv),
            cwd=spec.cwd,
            env=dict(spec.env),
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=self._stderr,
            text=True,
            encoding="utf-8",
            bufsize=1,
        )
        self._lines: queue.Queue[str | None] = queue.Queue()
        self._reader = threading.Thread(target=self._read_stdout, daemon=True)
        self._reader.start()
        self._next_id = 0

    def _read_stdout(self) -> None:
        # Okuyucu iş parçacığı: satırları kuyruğa koyar, akış bitince None ile işaretler.
        stdout = self._process.stdout
        if stdout is not None:
            for line in stdout:
                self._lines.put(line)
        self._lines.put(None)

    def _send(self, message: dict[str, JsonValue]) -> None:
        stdin = self._process.stdin
        if stdin is None or self._process.poll() is not None:
            raise McpProcessExitedError(
                f"server exited with code {self._process.poll()} before a send"
            )
        try:
            stdin.write(json.dumps(message, ensure_ascii=False) + "\n")
            stdin.flush()
        except BrokenPipeError as error:
            raise McpProcessExitedError("server closed its stdin") from error

    def _answer_server_request(self, message: dict[str, JsonValue]) -> None:
        # Sunucunun istemciye sorduğu istekler (ör. roots/list) burada cevaplanır.
        method = message.get("method")
        request_id = message.get("id")
        if method == "roots/list":
            roots: list[JsonValue] = [
                {"uri": root.as_uri(), "name": root.name} for root in self._spec.roots
            ]
            self._send({"jsonrpc": "2.0", "id": request_id, "result": {"roots": roots}})
        elif method == "ping":
            self._send({"jsonrpc": "2.0", "id": request_id, "result": {}})
        else:
            log_event(LOGGER, logging.WARNING, "unsupported_server_request", {"method": method})
            self._send(
                {
                    "jsonrpc": "2.0",
                    "id": request_id,
                    "error": {"code": -32601, "message": "method not supported by client"},
                }
            )

    def _request(
        self, method: str, params: dict[str, JsonValue], timeout_s: float
    ) -> dict[str, JsonValue]:
        self._next_id += 1
        request_id = self._next_id
        self._send({"jsonrpc": "2.0", "id": request_id, "method": method, "params": params})
        deadline = time.monotonic() + timeout_s
        while True:
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                raise McpTimeoutError(f"{method} (id {request_id}) timed out after {timeout_s}s")
            try:
                line = self._lines.get(timeout=remaining)
            except queue.Empty as error:
                raise McpTimeoutError(
                    f"{method} (id {request_id}) timed out after {timeout_s}s"
                ) from error
            if line is None:
                raise McpProcessExitedError(
                    f"server exited with code {self._process.poll()} during {method}"
                )
            if not line.strip():
                continue
            message = parse_message(line)
            if "method" in message and "id" in message:
                self._answer_server_request(message)
                continue
            if "method" in message:
                continue
            if message.get("id") != request_id:
                log_event(LOGGER, logging.WARNING, "stray_response", {"id": message.get("id")})
                continue
            if "error" in message:
                raise McpProtocolError(
                    f"{method} failed: {json.dumps(message['error'], ensure_ascii=False)[:500]}"
                )
            return as_object(message.get("result"), f"{method} result", McpProtocolError)

    def initialize(self, protocol_version: str, timeout_s: float) -> str:
        """MCP el sıkışmasını yapar ve sunucunun `ad sürüm` bilgisini döndürür."""
        result = self._request(
            "initialize",
            {
                "protocolVersion": protocol_version,
                "capabilities": {"roots": {"listChanged": False}},
                "clientInfo": {"name": "ccm-bench-freshness", "version": "0.1.0"},
            },
            timeout_s,
        )
        self._send({"jsonrpc": "2.0", "method": "notifications/initialized"})
        info = as_object(result.get("serverInfo"), "serverInfo", McpProtocolError)
        name = as_str(info.get("name"), "serverInfo.name", McpProtocolError)
        version = as_str(info.get("version"), "serverInfo.version", McpProtocolError)
        return f"{name} {version}"

    def call_tool(
        self, name: str, arguments: dict[str, JsonValue], timeout_s: float
    ) -> ToolResponse:
        """Bir aracı çağırır; ölçüm amaçlı olduğundan yeniden denemez."""
        started = time.monotonic()
        result = self._request("tools/call", {"name": name, "arguments": arguments}, timeout_s)
        return tool_response(result, time.monotonic() - started)

    def alive(self) -> bool:
        """Sunucu süreci hâlâ çalışıyor mu?"""
        return self._process.poll() is None

    def close(self, timeout_s: float) -> None:
        """Stdin'i kapatır, süreci bekler; çıkmazsa sonlandırır."""
        stdin = self._process.stdin
        if stdin is not None and not stdin.closed:
            try:
                stdin.close()
            except BrokenPipeError:
                log_event(LOGGER, logging.INFO, "stdin_already_closed", {})
        try:
            self._process.wait(timeout=timeout_s)
        except subprocess.TimeoutExpired:
            self._process.terminate()
            try:
                self._process.wait(timeout=timeout_s)
            except subprocess.TimeoutExpired:
                self._process.kill()
                self._process.wait(timeout=timeout_s)
        self._reader.join(timeout=timeout_s)
        self._stderr.close()
