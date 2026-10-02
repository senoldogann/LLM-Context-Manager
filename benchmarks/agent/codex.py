"""Codex CLI ajanı (ChatGPT aboneliği): `codex exec --json` komutu, ortamı ve dökümün okunması.

Codex deneye özel bir CODEX_HOME ile çalışır; o dizine bir kez `CODEX_HOME=<dizin> codex login`
ile giriş yapılır. Böylece sahibinin `~/.codex` girişi, hafızası, becerileri, eklentileri ve
küresel AGENTS.md dosyası koşulara girmez; kimlik dosyası hiçbir zaman kopyalanmaz.
"""

from __future__ import annotations

import json
import subprocess
from dataclasses import dataclass
from pathlib import Path

from freshness.jsonio import as_int, as_object, as_str
from freshness.model import JsonValue

SYSTEM_PATH = "/usr/bin:/bin:/usr/sbin:/sbin"
# Kollara yalnız kabuk ve dosya düzenleme kalsın: Claude kollarında Agent ve web araçları
# kapalı olduğu gibi burada da alt ajan, tarayıcı, uygulama ve eklenti yüzeyleri kapatılır.
DISABLED_FEATURES = (
    "apps",
    "browser_use",
    "browser_use_external",
    "browser_use_full_cdp_access",
    "computer_use",
    "goals",
    "hooks",
    "image_generation",
    "memories",
    "multi_agent",
    "plugins",
    "remote_plugin",
    "skill_mcp_dependency_install",
    "skill_search",
    "tool_suggest",
)


class CodexOutputError(ValueError):
    """`codex exec --json` çıktısı beklenen biçimde değil."""


@dataclass(frozen=True)
class McpServer:
    """B ve C kollarında Codex'e verilen CCM sunucusu."""

    name: str
    command: Path
    env: dict[str, str]
    enabled_tools: tuple[str, ...] | None
    instructions: str


@dataclass(frozen=True)
class CodexResult:
    """Codex dökümünden çıkarılanlar; `input_tokens` önbellekten okunanları da içerir.

    `failure` turun başarısız bittiğini gösterir; `warnings` Codex'in toparlandığı geçici
    hatalardır (örneğin "Reconnecting... 2/5") ve tek başına başarısızlık sayılmaz.
    """

    result_text: str
    failure: str | None
    warnings: tuple[str, ...]
    input_tokens: int
    cached_input_tokens: int
    output_tokens: int
    tool_calls: dict[str, int]


def toml_string(value: str) -> str:
    """TOML temel dizgesi; JSON kaçış kuralları TOML ile uyumludur."""
    return json.dumps(value)


def toml_table(values: dict[str, str]) -> str:
    """TOML satır içi tablosu."""
    return "{" + ", ".join(f"{key} = {toml_string(value)}" for key, value in values.items()) + "}"


def mcp_overrides(server: McpServer) -> list[str]:
    """CCM sunucusunu kullanıcı yapılandırması olmadan tanımlayan `-c` geçersiz kılmaları."""
    prefix = f"mcp_servers.{server.name}"
    overrides = [
        f"{prefix}.command={toml_string(str(server.command))}",
        f"{prefix}.args=[]",
        f"{prefix}.env={toml_table(server.env)}",
        f"{prefix}.startup_timeout_sec=60",
        f"{prefix}.tool_timeout_sec=300",
        f"developer_instructions={toml_string(server.instructions)}",
    ]
    if server.enabled_tools is not None:
        overrides.append(f"{prefix}.enabled_tools={json.dumps(list(server.enabled_tools))}")
    return [part for override in overrides for part in ("-c", override)]


def command(
    binary: Path,
    model: str,
    effort: str,
    workspace: Path,
    prompt: str,
    server: McpServer | None,
) -> list[str]:
    """`codex exec` komutu; kollar yalnız CCM sunucusu ve notunda ayrışır."""
    base = [
        str(binary),
        "exec",
        "--json",
        "--ephemeral",
        "--ignore-user-config",
        "--ignore-rules",
        "--cd",
        str(workspace),
        "--model",
        model,
        "--sandbox",
        "workspace-write",
        "-c",
        f"model_reasoning_effort={toml_string(effort)}",
        "-c",
        'web_search="disabled"',
    ]
    for feature in DISABLED_FEATURES:
        base += ["--disable", feature]
    if server is not None:
        base += mcp_overrides(server)
    return [*base, prompt]


def bundled_path(binary: Path) -> Path:
    """npm paketinin Codex'e verdiği araç dizini (paketlenmiş `rg`)."""
    path = binary.parent.parent / "codex-path"
    if not path.is_dir():
        raise CodexOutputError(f"no codex-path directory next to {binary}; pass the native binary")
    return path


def environment(binary: Path, home: Path, codex_home: Path) -> dict[str, str]:
    """Ajan ortamı: geçici HOME, deneye özel CODEX_HOME ve Codex'in kendi `rg`'si."""
    return {
        "PATH": f"{bundled_path(binary)}:{SYSTEM_PATH}",
        "HOME": str(home),
        "CODEX_HOME": str(codex_home),
        "CODEX_MANAGED_BY_NPM": "1",
        "LANG": "en_US.UTF-8",
    }


def login_status(binary: Path, codex_home: Path) -> str:
    """Deneye özel CODEX_HOME'un giriş durumu; model çağırmaz."""
    completed = subprocess.run(
        [str(binary), "login", "status"],
        env={"PATH": SYSTEM_PATH, "HOME": str(codex_home.parent), "CODEX_HOME": str(codex_home)},
        capture_output=True,
        text=True,
        check=False,
    )
    return (completed.stdout + completed.stderr).strip()


def json_event(line: str, number: int) -> dict[str, JsonValue]:
    """Döküm satırını JSON nesnesi olarak okur."""
    try:
        value: JsonValue = json.loads(line)
    except json.JSONDecodeError as error:
        raise CodexOutputError(f"codex output line {number} is not JSON: {line[:200]}") from error
    return as_object(value, f"codex output line {number}", CodexOutputError)


def error_message(event: dict[str, JsonValue]) -> str:
    """`turn.failed` ya da `error` olayının iletisi."""
    nested = event.get("error")
    if isinstance(nested, dict):
        return as_str(nested.get("message"), "turn.failed.error.message", CodexOutputError)
    return as_str(event.get("message"), "error.message", CodexOutputError)


def parse(lines: list[str]) -> CodexResult:
    """`codex exec --json` dökümünü okur; tamamlanmış ya da başarısız bir tur yoksa geçersizdir."""
    messages: list[str] = []
    warnings: list[str] = []
    failure: str | None = None
    calls: dict[str, int] = {}
    tokens = {"input_tokens": 0, "cached_input_tokens": 0, "output_tokens": 0}
    completed = False
    for number, line in enumerate(lines, 1):
        if not line.strip():
            continue
        event = json_event(line, number)
        kind = event.get("type")
        if kind == "item.completed":
            item = as_object(event.get("item"), "item.completed.item", CodexOutputError)
            item_type = as_str(item.get("type"), "item.type", CodexOutputError)
            if item_type == "agent_message":
                messages.append(as_str(item.get("text"), "agent_message.text", CodexOutputError))
            elif item_type == "mcp_tool_call":
                server = as_str(item.get("server"), "mcp_tool_call.server", CodexOutputError)
                tool = as_str(item.get("tool"), "mcp_tool_call.tool", CodexOutputError)
                name = f"mcp__{server}__{tool}"
                calls[name] = calls.get(name, 0) + 1
            elif item_type != "reasoning":
                calls[item_type] = calls.get(item_type, 0) + 1
        elif kind == "turn.completed":
            usage = as_object(event.get("usage"), "turn.completed.usage", CodexOutputError)
            for field in tokens:
                tokens[field] += as_int(usage.get(field), f"usage.{field}", CodexOutputError)
            completed = True
        elif kind == "turn.failed":
            failure = error_message(event)
        elif kind == "error":
            warnings.append(error_message(event))
    if failure is None and not completed:
        if not warnings:
            raise CodexOutputError("the codex output has neither turn.completed nor turn.failed")
        failure = warnings[-1]
    return CodexResult(
        result_text=messages[-1] if messages else "",
        failure=failure,
        warnings=tuple(warnings),
        input_tokens=tokens["input_tokens"],
        cached_input_tokens=tokens["cached_input_tokens"],
        output_tokens=tokens["output_tokens"],
        tool_calls=dict(sorted(calls.items())),
    )
