"""Bir görevi bir kolda çalıştırır: çalışma kopyası, CCM indeksi, `claude -p`, puan ve kayıt.

Kollar aynı modeli, aynı istemi ve aynı yerleşik araçları kullanır:
A yalnız yerleşik araçlar, B ek olarak CCM'in tüm MCP araçları, C ek olarak yalnız `search_code`.
Ajan `--bare` kipinde, geçici bir HOME ile çalışır: kullanıcının ayarları, kancaları,
eklentileri ve CLAUDE.md dosyaları yüklenmez; kimlik doğrulama yalnız ANTHROPIC_API_KEY'dir.
"""

from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import time
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Literal

from agent.score import Score, marker_files, score
from agent.tasks import Task, prompt_for
from freshness.jsonio import as_bool, as_float, as_int, as_list, as_object, as_str
from freshness.model import JsonValue
from freshness.repos import export_corpus, init_git, run_git

Arm = Literal["A", "B", "C"]
Embedding = Literal["local", "openai"]

SERVER = "context-manager"
CCM_TOOLS = (
    "map",
    "explain",
    "find_usages",
    "impact_of_change",
    "search_code",
    "find_nodes",
    "trace_call_chain",
    "diff_context",
    "index_project",
    "index_now",
)
BUILTIN_TOOLS = ("Read", "Grep", "Glob", "Edit", "Write", "Bash")
SYSTEM_PATH = "/usr/bin:/bin:/usr/sbin:/sbin"
OPENAI_EMBEDDING_MODEL = "text-embedding-3-small"
# B kolu ürünün önerdiği kurulumla çalışır: SKILL.md'nin projeye eklenmesini önerdiği not.
GRAPH_NOTE = (
    "## Code navigation\n"
    "Use the context-manager MCP tools before reading files: `map` once, then `explain` or "
    "`find_usages` for symbols, and `impact_of_change` before editing a file. Read only the "
    "lines you edit."
)
SEARCH_NOTE = (
    "## Code search\n"
    "Use the context-manager `search_code` MCP tool to find code before reading files. Read "
    "only the lines you need."
)


class RunError(RuntimeError):
    """Koşu kurulamadı ya da ajan sonuç üretmeden bitti; ölçüm durur, sorun giderilip sürdürülür."""


class TranscriptError(RunError):
    """stream-json çıktısı beklenen biçimde değil."""


@dataclass(frozen=True)
class Settings:
    """Ölçümün sabitleri; sonuç dizinine yazılır ve sürdürmede aynı olmaları denetlenir."""

    model: str
    effort: str
    embedding: Embedding
    max_budget_usd: float
    timeout_s: int
    claude_bin: Path
    ccm_bin_dir: Path
    ccm_model_dir: Path
    corpus_dir: Path
    out_dir: Path


@dataclass(frozen=True)
class Secrets:
    """Ortamdan okunan anahtarlar; loglanmaz ve kayıtlara girmez.

    OpenAI seçildiğinde anahtar, koşu süresince 0600 izinli MCP yapılandırmasında durur ve
    koşu bitince çalışma diziniyle birlikte silinir.
    """

    anthropic_api_key: str
    openai_api_key: str | None


@dataclass(frozen=True)
class Usage:
    """Claude Code'un sonuç olayındaki toplam token kullanımı."""

    input_tokens: int
    output_tokens: int
    cache_creation_input_tokens: int
    cache_read_input_tokens: int


@dataclass(frozen=True)
class Transcript:
    """stream-json dökümünden çıkarılan ölçüler."""

    result_text: str
    subtype: str
    is_error: bool
    num_turns: int
    duration_ms: int
    cost_usd: float
    usage: Usage
    tool_calls: dict[str, int]
    mcp_status: str


@dataclass(frozen=True)
class RunRecord:
    """Bir koşunun kaydı; `runs/<run_id>.json` olarak yazılır."""

    run_id: str
    task_id: str
    repo: str
    category: str
    arm: Arm
    rep: int
    model: str
    effort: str
    embedding: Embedding
    index_s: float | None
    wall_s: float | None
    transcript: Transcript | None
    score: Score | None
    failure: str | None


def run_id(task: Task, arm: Arm, rep: int) -> str:
    """Koşunun dosya adlarında kullanılan kimliği."""
    return f"{task.id}__{arm}__{rep}"


def record_path(out_dir: Path, name: str) -> Path:
    """Koşu kaydının yolu; varlığı koşunun tamamlandığını gösterir."""
    return out_dir / "runs" / f"{name}.json"


def mcp_tool(name: str) -> str:
    """CCM aracının Claude Code'daki adı."""
    return f"mcp__{SERVER}__{name}"


def prepare_workspace(corpus: Path, workspace: Path) -> None:
    """Korpusu temiz, tek commit'lik bir git deposuna çıkarır."""
    export_corpus(corpus, workspace)
    init_git(workspace, "main")


def index_db_path(workspace: Path) -> Path:
    """CCM artifact'lerini ajanın kaynak keşfinden gizli, proje-içi bir yolda tutar."""
    return workspace / ".git" / "ccm-bench" / "ccm_db"


def ccm_env(settings: Settings, home: Path, secrets: Secrets) -> dict[str, str]:
    """`ccm-cli` ve `ccm-mcp` ortamı: geçici HOME ve seçilen embedding kaynağı."""
    base = {"PATH": SYSTEM_PATH, "HOME": str(home), "RUST_LOG": "warn"}
    if settings.embedding == "local":
        return {**base, "CCM_MODEL_DIR": str(settings.ccm_model_dir)}
    if secrets.openai_api_key is None:
        raise RunError("--embedding openai needs OPENAI_API_KEY in the environment")
    return {
        **base,
        "EMBEDDING_PROVIDER": "openai",
        "EMBEDDING_MODEL": OPENAI_EMBEDDING_MODEL,
        "OPENAI_API_KEY": secrets.openai_api_key,
    }


def index_workspace(settings: Settings, workspace: Path, env: dict[str, str]) -> float:
    """Ajan başlamadan önce tam indeks kurar (kullanıcının indeksi hazır bulduğu durum)."""
    started = time.monotonic()
    completed = subprocess.run(
        [
            str(settings.ccm_bin_dir / "ccm-cli"),
            "index",
            "--path",
            str(workspace),
            "--db-path",
            str(index_db_path(workspace)),
        ],
        cwd=workspace,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        raise RunError(
            f"ccm-cli index failed for {workspace} (exit {completed.returncode}): "
            f"{completed.stderr[-2000:]}"
        )
    return time.monotonic() - started


def mcp_config(settings: Settings, workspace: Path, env: dict[str, str]) -> dict[str, JsonValue]:
    """`--mcp-config` dosyasının içeriği: yalnız CCM sunucusu."""
    server_env: dict[str, JsonValue] = {
        **env,
        "CCM_PROJECT_ROOT": str(workspace),
        "CCM_ALLOWED_ROOTS": str(workspace),
        "CCM_DB_PATH": str(index_db_path(workspace)),
    }
    server: dict[str, JsonValue] = {
        "type": "stdio",
        "command": str(settings.ccm_bin_dir / "ccm-mcp"),
        "args": [],
        "env": server_env,
    }
    return {"mcpServers": {SERVER: server}}


def write_private(path: Path, text: str) -> None:
    """Dosyayı yalnız sahibinin okuyabileceği izinlerle yazar."""
    descriptor = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
    with os.fdopen(descriptor, "w") as handle:
        handle.write(text)


def claude_command(settings: Settings, task: Task, arm: Arm, config_path: Path) -> list[str]:
    """Kolun `claude -p` komutu; kollar yalnız MCP araçları ve proje notunda ayrışır."""
    command = [
        str(settings.claude_bin),
        "-p",
        prompt_for(task),
        "--bare",
        "--output-format",
        "stream-json",
        "--verbose",
        "--model",
        settings.model,
        "--effort",
        settings.effort,
        "--max-budget-usd",
        f"{settings.max_budget_usd:.2f}",
        "--no-session-persistence",
        "--permission-mode",
        "dontAsk",
        "--permission-prompts",
        "none",
        "--strict-mcp-config",
        "--tools",
        ",".join(BUILTIN_TOOLS),
    ]
    if arm == "A":
        return [*command, "--allowedTools", ",".join(BUILTIN_TOOLS)]
    granted = CCM_TOOLS if arm == "B" else ("search_code",)
    denied = [mcp_tool(tool) for tool in CCM_TOOLS if tool not in granted]
    command += [
        "--mcp-config",
        str(config_path),
        "--append-system-prompt",
        GRAPH_NOTE if arm == "B" else SEARCH_NOTE,
        "--allowedTools",
        ",".join([*BUILTIN_TOOLS, *(mcp_tool(tool) for tool in granted)]),
    ]
    return [*command, "--disallowedTools", ",".join(denied)] if denied else command


def claude_env(home: Path, secrets: Secrets) -> dict[str, str]:
    """Ajan ortamı: geçici HOME ve yalnız Anthropic anahtarı."""
    return {
        "PATH": SYSTEM_PATH,
        "HOME": str(home),
        "CLAUDE_CONFIG_DIR": str(home / ".claude"),
        "ANTHROPIC_API_KEY": secrets.anthropic_api_key,
        "LANG": "en_US.UTF-8",
        "DISABLE_AUTOUPDATER": "1",
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC": "1",
    }


def stop_group(pid: int) -> None:
    """Sürecin oturum grubunu (MCP sunucusu gibi alt süreçlerle) sonlandırır."""
    try:
        os.killpg(pid, signal.SIGKILL)
    except ProcessLookupError:
        # Grup zaten kapanmış: sonlandırılacak süreç kalmamış.
        return


def run_claude(
    command: list[str],
    workspace: Path,
    env: dict[str, str],
    timeout_s: int,
    transcript: Path,
    stderr: Path,
) -> float:
    """Ajanı çalıştırır, dökümü dosyaya yazar ve duvar saatini döner."""
    transcript.parent.mkdir(parents=True, exist_ok=True)
    started = time.monotonic()
    with transcript.open("w") as out, stderr.open("w") as err:
        process = subprocess.Popen(
            command, cwd=workspace, env=env, stdout=out, stderr=err, start_new_session=True
        )
        try:
            process.wait(timeout=timeout_s)
        except subprocess.TimeoutExpired as error:
            stop_group(process.pid)
            process.wait()
            raise RunError(
                f"claude did not finish within {timeout_s} s; see {transcript}"
            ) from error
    stop_group(process.pid)
    return time.monotonic() - started


def json_line(line: str, number: int) -> JsonValue:
    """Döküm satırını JSON olarak okur."""
    try:
        value: JsonValue = json.loads(line)
    except json.JSONDecodeError as error:
        raise TranscriptError(f"transcript line {number} is not JSON: {line[:200]}") from error
    return value


def server_status(event: dict[str, JsonValue]) -> str:
    """`init` olayındaki CCM sunucusunun durumu; sunucu yoksa `none`."""
    servers = event.get("mcp_servers")
    if servers is None:
        return "none"
    for server in as_list(servers, "init.mcp_servers", TranscriptError):
        entry = as_object(server, "init.mcp_servers[]", TranscriptError)
        if entry.get("name") == SERVER:
            return as_str(entry.get("status"), "init.mcp_servers[].status", TranscriptError)
    return "none"


def tool_uses(event: dict[str, JsonValue]) -> list[str]:
    """Asistan mesajındaki araç çağrılarının adları."""
    message = as_object(event.get("message"), "assistant.message", TranscriptError)
    content = message.get("content")
    if not isinstance(content, list):
        return []
    names: list[str] = []
    for block in content:
        entry = as_object(block, "assistant.message.content[]", TranscriptError)
        if entry.get("type") == "tool_use":
            names.append(as_str(entry.get("name"), "tool_use.name", TranscriptError))
    return names


def transcript_from(
    result: dict[str, JsonValue], tool_calls: dict[str, int], mcp_status: str
) -> Transcript:
    """Sonuç olayını ölçülere çevirir; eksik alan biçim hatasıdır."""
    usage = as_object(result.get("usage"), "result.usage", TranscriptError)
    text = result.get("result")
    return Transcript(
        result_text="" if text is None else as_str(text, "result.result", TranscriptError),
        subtype=as_str(result.get("subtype"), "result.subtype", TranscriptError),
        is_error=as_bool(result.get("is_error"), "result.is_error", TranscriptError),
        num_turns=as_int(result.get("num_turns"), "result.num_turns", TranscriptError),
        duration_ms=as_int(result.get("duration_ms"), "result.duration_ms", TranscriptError),
        cost_usd=as_float(result.get("total_cost_usd"), "result.total_cost_usd", TranscriptError),
        usage=Usage(
            input_tokens=as_int(usage.get("input_tokens"), "usage.input_tokens", TranscriptError),
            output_tokens=as_int(
                usage.get("output_tokens"), "usage.output_tokens", TranscriptError
            ),
            cache_creation_input_tokens=as_int(
                usage.get("cache_creation_input_tokens"),
                "usage.cache_creation_input_tokens",
                TranscriptError,
            ),
            cache_read_input_tokens=as_int(
                usage.get("cache_read_input_tokens"),
                "usage.cache_read_input_tokens",
                TranscriptError,
            ),
        ),
        tool_calls=dict(sorted(tool_calls.items())),
        mcp_status=mcp_status,
    )


def parse_transcript(lines: list[str]) -> Transcript:
    """stream-json dökümünü okur; sonuç olayı yoksa koşu geçersizdir."""
    tool_calls: dict[str, int] = {}
    mcp_status = "none"
    result: dict[str, JsonValue] | None = None
    for number, line in enumerate(lines, 1):
        if not line.strip():
            continue
        event = as_object(json_line(line, number), f"transcript line {number}", TranscriptError)
        kind = event.get("type")
        if kind == "system" and event.get("subtype") == "init":
            mcp_status = server_status(event)
        elif kind == "assistant":
            for name in tool_uses(event):
                tool_calls[name] = tool_calls.get(name, 0) + 1
        elif kind == "result":
            result = event
    if result is None:
        raise TranscriptError("the transcript has no result event")
    return transcript_from(result, tool_calls, mcp_status)


def write_record(path: Path, record: RunRecord) -> None:
    """Kaydı atomik yazar: yarım dosya hiçbir zaman tamamlanmış koşu sayılmaz."""
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(asdict(record), indent=2, ensure_ascii=False))
    temporary.replace(path)


def run_one(settings: Settings, task: Task, arm: Arm, rep: int, secrets: Secrets) -> RunRecord:
    """Bir koşuyu yürütür; ajan/ürün hataları da tamamlanmış kayıt olarak saklanır."""
    name = run_id(task, arm, rep)
    work = settings.out_dir / "work" / name
    if work.exists():
        # Yarıda kalmış önceki denemenin kalıntısı; kaydı olmadığı için yeniden koşulur.
        shutil.rmtree(work)
    home = work / "home"
    home.mkdir(parents=True)
    workspace = work / f"ws-{name}"
    transcript_path = settings.out_dir / "transcripts" / f"{name}.jsonl"
    index_s: float | None = None
    wall_s: float | None = None
    transcript: Transcript | None = None
    result: Score | None = None
    failure: str | None = None
    agent_started: float | None = None
    try:
        prepare_workspace(settings.corpus_dir / task.repo, workspace)
        config_path = work / "mcp.json"
        if arm != "A":
            env = ccm_env(settings, home, secrets)
            index_s = index_workspace(settings, workspace, env)
            write_private(config_path, json.dumps(mcp_config(settings, workspace, env), indent=2))
        agent_started = time.monotonic()
        wall_s = run_claude(
            claude_command(settings, task, arm, config_path),
            workspace,
            claude_env(home, secrets),
            settings.timeout_s,
            transcript_path,
            transcript_path.with_suffix(".stderr"),
        )
        transcript = parse_transcript(transcript_path.read_text().splitlines())
        if transcript.is_error:
            failure = f"claude result error ({transcript.subtype})"
        if arm != "A" and transcript.mcp_status != "connected":
            mcp_failure = (
                f"CCM MCP server did not connect (status {transcript.mcp_status}); "
                f"see {transcript_path.with_suffix('.stderr')}"
            )
            failure = mcp_failure if failure is None else f"{failure}; {mcp_failure}"
        contents = (
            {}
            if task.edit is None
            else {path: (workspace / path).read_text() for path in marker_files(task.edit)}
        )
        diff = run_git(workspace, ("diff",))
        if diff:
            diff_path = settings.out_dir / "diffs" / f"{name}.diff"
            diff_path.parent.mkdir(parents=True, exist_ok=True)
            diff_path.write_text(diff)
        result = score(task, transcript.result_text, workspace.name, contents)
    except RunError as error:
        if agent_started is not None:
            wall_s = time.monotonic() - agent_started
        failure = str(error)
    finally:
        shutil.rmtree(work)
    record = RunRecord(
        run_id=name,
        task_id=task.id,
        repo=task.repo,
        category=task.category,
        arm=arm,
        rep=rep,
        model=settings.model,
        effort=settings.effort,
        embedding=settings.embedding,
        index_s=index_s,
        wall_s=wall_s,
        transcript=transcript,
        score=result,
        failure=failure,
    )
    write_record(record_path(settings.out_dir, name), record)
    return record
