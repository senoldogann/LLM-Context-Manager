"""Model çağırmadan context-first pilot paketlerinin recall/precision preflight'ı.

Bu kontrol L3 agent sonucunun yerine geçmez. Yalnız üç pilot görevin sorusunu CCM'nin
`context` aracına tek çağrıda verir ve caller satırlarını elle doğrulanmış ground truth ile
bire bir eşleştirir. Eksik, fazla veya duplicate caller başarısızdır. Embedding kapalıdır;
bu görevlerin açık kod ipuçları server-side deterministik routing ile çözülmelidir.
"""

from __future__ import annotations

import json
import os
import subprocess
import tempfile
import time
from dataclasses import dataclass, replace
from pathlib import Path
from typing import cast

from agent.check import apply_steps, check_tasks
from agent.run import SYSTEM_PATH, index_db_path, prepare_workspace
from agent.tasks import TASKS, Item, Task, label
from freshness.repos import run_git

PILOT_TASK_IDS = (
    "flask-callers-ensure-sync",
    "express-trap-req-get",
    "serde-edit-with-bound",
)
CORPUS_MANIFEST = Path(__file__).resolve().parents[1] / "corpus.json"


class PreflightError(RuntimeError):
    """Context packet preflight kurulamadı veya MCP protokolü bozuldu."""


@dataclass(frozen=True)
class PacketScore:
    """Bir context packet'inin bire-bir caller eşleşme sonucu."""

    task_id: str
    expected: int
    found: int
    missing: tuple[str, ...]
    extras: tuple[str, ...]
    index_s: float

    @property
    def ok(self) -> bool:
        """Eksik ve fazla caller yoksa paket ground truth ile aynıdır."""
        return not self.missing and not self.extras


def pilot_tasks() -> tuple[Task, ...]:
    """Üç context-first pilot görevini ana task tablosundan veri sırasıyla seç."""
    selected = tuple(task for task in TASKS if task.id in PILOT_TASK_IDS)
    if tuple(task.id for task in selected) != PILOT_TASK_IDS:
        found = {task.id for task in selected}
        missing = [task_id for task_id in PILOT_TASK_IDS if task_id not in found]
        raise PreflightError("pilot tasks missing from agent.tasks: " + ", ".join(missing))
    return selected


def task_file_paths(task: Task) -> tuple[str, ...]:
    """Task doğrulama/edit akışının erişebileceği repo-içi dosyalar."""
    paths = [item.path for item in task.expected]
    if task.edit is not None:
        paths.extend(step.file for step in task.edit.steps)
        paths.extend(marker.file for marker in (*task.edit.present, *task.edit.absent))
        paths.extend(item.path for item in (*task.edit.added, *task.edit.removed))
    return tuple(dict.fromkeys(paths))


def confined_path(root: Path, relative: str, context: str, *, directory: bool = False) -> Path:
    """Relative path'i canonical root içinde tut; symlink/.. escape'ini reddet."""
    rel = Path(relative)
    if rel.is_absolute() or ".." in rel.parts:
        raise PreflightError(f"{context}: unsafe relative path {relative!r}")
    resolved_root = root.resolve()
    candidate = (resolved_root / rel).resolve()
    try:
        candidate.relative_to(resolved_root)
    except ValueError as error:
        raise PreflightError(f"{context}: path escapes root: {relative!r}") from error
    exists = candidate.is_dir() if directory else candidate.is_file()
    if not exists:
        kind = "directory" if directory else "file"
        raise PreflightError(f"{context}: {kind} not found: {relative!r}")
    return candidate


def verify_pinned_corpus(tasks: tuple[Task, ...], corpus: Path) -> None:
    """Pilot repolarını resmi corpus manifest pinlerine ve clean HEAD'e sabitle."""
    try:
        manifest: object = json.loads(CORPUS_MANIFEST.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise PreflightError(f"cannot read corpus manifest {CORPUS_MANIFEST}: {error}") from error
    if not isinstance(manifest, dict) or not isinstance(manifest.get("repos"), list):
        raise PreflightError(f"invalid corpus manifest: {CORPUS_MANIFEST}")

    pins: dict[str, str] = {}
    for entry in cast(list[object], manifest["repos"]):
        if not isinstance(entry, dict):
            continue
        name = entry.get("name")
        commit = entry.get("commit")
        if isinstance(name, str) and isinstance(commit, str):
            pins[name] = commit

    for task in tasks:
        expected_commit = pins.get(task.repo)
        if expected_commit is None:
            raise PreflightError(
                f"{task.id}: repo {task.repo!r} is not pinned in {CORPUS_MANIFEST}"
            )
        repo = confined_path(corpus, task.repo, task.id, directory=True)
        actual_commit = run_git(repo, ("rev-parse", "HEAD")).strip()
        if actual_commit != expected_commit:
            raise PreflightError(
                f"{task.id}: corpus HEAD mismatch for {task.repo}: "
                f"expected {expected_commit}, got {actual_commit}"
            )
        if run_git(repo, ("status", "--porcelain")).strip():
            raise PreflightError(f"{task.id}: corpus repo is dirty: {repo}")
        for relative in task_file_paths(task):
            confined_path(repo, relative, task.id)


def parse_callers(text: str) -> list[tuple[str, str]]:
    """Compact explanation içindeki `callers:` satırlarını sırasıyla koru."""
    lines = iter(text.splitlines())
    for line in lines:
        if line == "callers:":
            break
    else:
        return []

    callers: list[tuple[str, str]] = []
    for line in lines:
        if not line.startswith("- "):
            break
        fields = line[2:].split(" · ")
        if len(fields) < 2 or ": " not in fields[0] or ":" not in fields[1]:
            raise PreflightError(f"unrecognised caller line: {line}")
        _kind, name = fields[0].split(": ", 1)
        path, _location = fields[1].rsplit(":", 1)
        callers.append((path, name))
    return callers


def item_matches(item: Item, candidate: tuple[str, str]) -> bool:
    """Caller path/name'i Item'ın canonical adı veya alias'larından biri mi?"""
    path, name = candidate
    return path == item.path and name in (item.name, *item.aliases)


def score_callers(task: Task, callers: list[tuple[str, str]], index_s: float) -> PacketScore:
    """Caller listesini ground truth ile bire-bir eşleştir; duplicate actual extra sayılır."""
    unmatched = list(range(len(callers)))
    missing: list[str] = []
    found = 0

    for item in task.expected:
        match_index = next(
            (index for index in unmatched if item_matches(item, callers[index])),
            None,
        )
        if match_index is None:
            missing.append(label(item))
            continue
        unmatched.remove(match_index)
        found += 1

    extras = tuple(f"{callers[index][0]}:{callers[index][1]}" for index in unmatched)
    return PacketScore(
        task_id=task.id,
        expected=len(task.expected),
        found=found,
        missing=tuple(missing),
        extras=extras,
        index_s=index_s,
    )


def executable(path: Path) -> Path:
    """Var ve executable olan path'i resolve et; değilse açık hata ver."""
    resolved = path.expanduser().resolve()
    if not resolved.is_file() or not os.access(resolved, os.X_OK):
        raise PreflightError(f"executable not found: {resolved}")
    return resolved


def base_env(home: Path) -> dict[str, str]:
    """Preflight'ın secretsiz, ağsız CCM ortamı."""
    return {
        "PATH": SYSTEM_PATH,
        "HOME": str(home),
        "RUST_LOG": "warn",
        "CCM_DISABLE_EMBEDDER": "1",
    }


def index_workspace(cli: Path, workspace: Path, home: Path) -> float:
    """Graph-only indeks kur; semantic model/API kullanma."""
    started = time.monotonic()
    completed = subprocess.run(
        [
            str(cli),
            "index",
            "--path",
            str(workspace),
            "--db-path",
            str(index_db_path(workspace)),
        ],
        cwd=workspace,
        env=base_env(home),
        capture_output=True,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        raise PreflightError(
            f"ccm-cli index failed for {workspace} (exit {completed.returncode}): "
            f"{completed.stderr[-2000:]}"
        )
    return time.monotonic() - started


def send_message(process: subprocess.Popen[str], message: dict[str, object]) -> None:
    """Bir JSON-RPC satırı gönder."""
    if process.stdin is None:
        raise PreflightError("MCP stdin is unavailable")
    process.stdin.write(json.dumps(message, separators=(",", ":")) + "\n")
    process.stdin.flush()


def next_message(process: subprocess.Popen[str]) -> dict[str, object]:
    """Bir JSON-RPC object satırı oku."""
    if process.stdout is None:
        raise PreflightError("MCP stdout is unavailable")
    line = process.stdout.readline()
    if not line:
        raise PreflightError("MCP server closed stdout")
    parsed: object = json.loads(line)
    if not isinstance(parsed, dict):
        raise PreflightError("MCP response is not an object")
    return cast(dict[str, object], parsed)


def response_for(
    process: subprocess.Popen[str],
    request_id: int,
    workspace: Path,
) -> dict[str, object]:
    """İstenen response gelene kadar roots/list handshake'ini cevapla."""
    while True:
        message = next_message(process)
        if message.get("method") == "roots/list":
            roots_id = message.get("id")
            send_message(
                process,
                {
                    "jsonrpc": "2.0",
                    "id": roots_id,
                    "result": {"roots": [{"uri": workspace.as_uri()}]},
                },
            )
            continue
        if message.get("id") == request_id:
            return message


def request_context(
    process: subprocess.Popen[str], workspace: Path, query: str, request_id: int
) -> str:
    """Açık MCP oturumunda tek context isteği yap; protokol/araç hatasını reddet."""
    send_message(
        process,
        {
            "jsonrpc": "2.0",
            "id": request_id,
            "method": "tools/call",
            "params": {
                "name": "context",
                "arguments": {"query": query, "max_tokens": 1500},
            },
        },
    )
    response = response_for(process, request_id, workspace)
    result = response.get("result")
    if not isinstance(result, dict):
        raise PreflightError(f"context returned no result: {response}")
    typed_result = cast(dict[str, object], result)
    if typed_result.get("isError") is True:
        raise PreflightError(f"context returned a tool error: {response}")
    content = typed_result.get("content")
    if not isinstance(content, list) or not content:
        raise PreflightError(f"context returned no content: {response}")
    first = content[0]
    if not isinstance(first, dict):
        raise PreflightError(f"context content has unexpected shape: {response}")
    text = cast(dict[str, object], first).get("text")
    if not isinstance(text, str):
        raise PreflightError(f"context content has no text: {response}")
    return text


def context_packet(
    mcp: Path, workspace: Path, home: Path, query: str, live_edit: Task | None
) -> tuple[str, float]:
    """İzole MCP oturumunda paket al; canlı edit varsa ilk post-edit çağrısını ölç."""
    env = {
        **base_env(home),
        "CCM_PROJECT_ROOT": str(workspace),
        "CCM_ALLOWED_ROOTS": str(workspace),
        "CCM_REQUIRE_ALLOWED_ROOTS": "1",
        "CCM_DB_PATH": str(index_db_path(workspace)),
        "CCM_AUTO_REFRESH": "1" if live_edit is not None else "0",
    }
    process = subprocess.Popen(
        [str(mcp)],
        cwd=workspace,
        env=env,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
    )
    try:
        send_message(process, {"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}})
        response_for(process, 1, workspace)
        request_id = 2
        if live_edit is not None:
            edit = live_edit.edit
            if edit is None:
                raise PreflightError(f"{live_edit.id}: live edit requires an edit task")
            before = request_context(process, workspace, query, request_id)
            expected_before = tuple(item for item in live_edit.expected if item not in edit.added)
            baseline = replace(live_edit, expected=(*expected_before, *edit.removed))
            before_score = score_callers(baseline, parse_callers(before), 0.0)
            if not before.startswith("_Index: fresh · auto-refresh on_") or not before_score.ok:
                raise PreflightError(f"{live_edit.id}: pre-edit packet is not current: {before}")
            problems = apply_steps(live_edit, workspace)
            if problems:
                raise PreflightError("\n".join(problems))
            request_id += 1
        started = time.monotonic()
        text = request_context(process, workspace, query, request_id)
        read_s = time.monotonic() - started
        if live_edit is not None and not text.startswith("_Index: fresh · auto-refresh on_"):
            raise PreflightError(f"{live_edit.id}: first post-edit packet is not fresh: {text}")
        return text, read_s
    finally:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


def context_text(mcp: Path, workspace: Path, home: Path, query: str) -> str:
    """Release MCP'ye tek context çağrısı yap ve text payload'ı döndür."""
    text, _read_s = context_packet(mcp, workspace, home, query, None)
    return text


def run_task(
    task: Task,
    corpus_dir: Path,
    scratch: Path,
    cli: Path,
    mcp: Path,
    home: Path,
    live_edits: bool,
) -> PacketScore:
    """Workspace hazırla; edit'i seçilen sırada uygula ve context'i puanla."""
    workspace = scratch / task.id
    repo = confined_path(corpus_dir, task.repo, task.id, directory=True)
    prepare_workspace(repo, workspace)
    for relative in task_file_paths(task):
        confined_path(workspace, relative, task.id)
    if not live_edits:
        edit_problems = apply_steps(task, workspace)
        if edit_problems:
            raise PreflightError("\n".join(edit_problems))
    index_s = index_workspace(cli, workspace, home)
    text, read_s = context_packet(mcp, workspace, home, task.question, task if live_edits else None)
    if live_edits:
        print(f"{task.id}: first_post_edit_context_s={read_s:.3f} (no polling or reindex)")
    return score_callers(task, parse_callers(text), index_s)


def command_preflight(ccm_bin_dir: Path, corpus_dir: Path, live_edits: bool) -> int:
    """Üç pilot context packet'ini model çağırmadan doğrula."""
    bin_dir = ccm_bin_dir.expanduser().resolve()
    corpus = corpus_dir.expanduser().resolve()
    cli = executable(bin_dir / "ccm-cli")
    mcp = executable(bin_dir / "ccm-mcp")
    if not corpus.is_dir():
        raise PreflightError(f"corpus directory not found: {corpus}")

    tasks = pilot_tasks()
    verify_pinned_corpus(tasks, corpus)
    problems = check_tasks(tasks, corpus)
    if problems:
        raise PreflightError("task validation failed:\n" + "\n".join(problems))

    if live_edits:
        tasks = tuple(task for task in tasks if task.edit is not None)
        if not tasks:
            raise PreflightError("no pilot edit tasks available for live preflight")

    all_ok = True
    with tempfile.TemporaryDirectory(prefix="ccm-context-preflight-") as directory:
        scratch = Path(directory)
        home = scratch / "home"
        home.mkdir()
        for task in tasks:
            result = run_task(task, corpus, scratch, cli, mcp, home, live_edits)
            precision_denominator = result.found + len(result.extras)
            precision = result.found / precision_denominator if precision_denominator else 1.0
            recall = result.found / result.expected if result.expected else 1.0
            print(
                f"{result.task_id}: callers={result.found}/{result.expected} "
                f"recall={recall:.3f} precision={precision:.3f} "
                f"index_s={result.index_s:.2f}"
            )
            if result.missing:
                print("  missing: " + ", ".join(result.missing))
            if result.extras:
                print("  extras: " + ", ".join(result.extras))
            all_ok = all_ok and result.ok

    mode = "live edit context preflight" if live_edits else "context preflight"
    print(mode + ": " + ("PASS" if all_ok else "FAIL"))
    return 0 if all_ok else 1
