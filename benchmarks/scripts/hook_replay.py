"""Kaydedilmiş Claude Code Grep çağrılarını CCM hook'undan geçirir (çevrimdışı, ajan yok).

Kullanım:
    uv run python scripts/hook_replay.py <ccm bin dizini> <korpus dizini> <sonuç dizini> \
        <döküm deseni> <araçlar>

Sonuç dizini `<set>/claude/transcripts/<run>.jsonl` dökümlerini içerir (ör. `results/agent`).
`<araçlar>` oynatılacak araçlardır: `Grep` (D kolunun hook'u) ya da `Grep,Bash` (E kolu;
Bash'te `grep`/`rg` geçen komutlar, kayıttaki çalışma kopyası yolu yenisiyle değiştirilerek).
Bir koşunun aramaları kayıttaki sırayla aynı hook oturumunda oynatılır: tekrarlanan Grep'in
cevabı (ilk paket, ikinci kez çağrı satırları) koşudaki gibi görünür.
Döküm deseni hangi koşuların oynatılacağını seçer, sonuç dizinine göre bir glob'dur: örneğin
`*/claude/transcripts/*__[ABFH]__1.jsonl` (pilotların ilk tekrarları) ya da
`large-repo-20261111/claude/transcripts/*__D__*.jsonl`.
Her depo için temiz bir çalışma kopyası graph-only indekslenir; her Grep, kaydedildiği desen,
yol ve kiple `ccm-mcp hook claude-pre-tool-use`'a verilir. Çıktı: her çağrının kararı, hook'un
modele vereceği metnin boyutu ve dökümdeki gerçek Grep sonucunun boyutu.
"""

import collections
import json
import subprocess
import sys
import tempfile
import uuid
from dataclasses import dataclass
from pathlib import Path

import agent.context_preflight as preflight
from agent.run import index_db_path, prepare_workspace
from agent.tasks import KNOWN_TASKS


@dataclass(frozen=True)
class RecordedGrep:
    """Dökümdeki bir Grep çağrısı ve gerçek sonucunun karakter sayısı."""

    run: str
    tool: str
    tool_input: dict[str, str | bool]
    result_chars: int


@dataclass(frozen=True)
class Replayed:
    """Hook'un bir kayıtlı Grep için kararı."""

    grep: RecordedGrep
    decision: str
    detail: str
    answer_chars: int | None
    # Reddedilen Grep'e verilen cevap: `packet` (ilk) ya da `call-site lines` (tekrar).
    answer_kind: str


def result_text(content: str | list[dict[str, str]]) -> str:
    """Araç sonucunun metni (düz metin ya da metin blokları)."""
    return (
        content if isinstance(content, str) else "".join(part.get("text", "") for part in content)
    )


# Bash komutunda kayıttaki çalışma kopyası yolunun yerine geçen işaret.
WORKSPACE_MARK = "{WORKSPACE}"


def searches_text(command: str) -> bool:
    """Bash komutunda `grep` ya da `rg` geçiyor mu."""
    return any(word in ("grep", "rg") for word in command.replace("|", " ").split())


def recorded_greps(transcript: Path, tools: set[str]) -> list[RecordedGrep]:
    """Bir Claude dökümündeki aramalar (Grep ve istenirse Bash), kayıt sırasıyla; yollar
    çalışma kopyasına göre görelidir."""
    run = transcript.name.split(".")[0]
    cwd = ""
    inputs: dict[str, dict[str, str | bool]] = {}
    names: dict[str, str] = {}
    sizes: dict[str, int] = {}
    for line in transcript.read_text().splitlines():
        event = json.loads(line)
        if event.get("type") == "system" and event.get("subtype") == "init":
            cwd = event["cwd"]
        if event.get("type") == "assistant":
            for block in event["message"]["content"]:
                if block.get("type") != "tool_use" or block["name"] not in tools:
                    continue
                if block["name"] == "Bash" and not searches_text(block["input"].get("command", "")):
                    continue
                inputs[block["id"]] = block["input"]
                names[block["id"]] = block["name"]
        content = event.get("message", {}).get("content") if event.get("type") == "user" else None
        if isinstance(content, list):
            for block in content:
                if block.get("type") == "tool_result" and block["tool_use_id"] in inputs:
                    sizes[block["tool_use_id"]] = len(result_text(block["content"]))
    greps = []
    for use_id, tool_input in inputs.items():
        path = tool_input.get("path")
        relative = dict(tool_input)
        if isinstance(path, str) and cwd and path.startswith(cwd):
            relative["path"] = path[len(cwd) :].lstrip("/") or "."
        command = tool_input.get("command")
        if isinstance(command, str) and cwd:
            relative["command"] = command.replace(cwd, WORKSPACE_MARK)
        greps.append(RecordedGrep(run, names[use_id], relative, sizes.get(use_id, 0)))
    return greps


def replay_repo(
    bin_dir: Path, corpus: Path, repo: str, greps: list[RecordedGrep]
) -> list[Replayed]:
    """Bir deponun kayıtlı Grep'lerini hook'tan geçirir; her çağrı ayrı oturumdur."""
    replayed = []
    with tempfile.TemporaryDirectory() as scratch:
        root = Path(scratch)
        home = root / "home"
        home.mkdir()
        workspace = root / "ws"
        prepare_workspace(corpus / repo, workspace)
        preflight.index_workspace(bin_dir / "ccm-cli", workspace, home)
        log = root / "hook.jsonl"
        env = {
            **preflight.base_env(home),
            "CCM_PROJECT_ROOT": str(workspace),
            "CCM_ALLOWED_ROOTS": str(workspace),
            "CCM_DB_PATH": str(index_db_path(workspace)),
            "CCM_HOOK_LOG": str(log),
        }
        sessions: dict[str, str] = {}
        for grep in greps:
            tool_input = dict(grep.tool_input)
            path = tool_input.get("path")
            if isinstance(path, str):
                tool_input["path"] = str(workspace / path)
            command = tool_input.get("command")
            if isinstance(command, str):
                tool_input["command"] = command.replace(WORKSPACE_MARK, str(workspace))
            payload = {
                "session_id": sessions.setdefault(grep.run, f"replay-{uuid.uuid4().hex}"),
                "cwd": str(workspace),
                "hook_event_name": "PreToolUse",
                "tool_name": grep.tool,
                "tool_input": tool_input,
            }
            completed = subprocess.run(
                [str(bin_dir / "ccm-mcp"), "hook", "claude-pre-tool-use"],
                input=json.dumps(payload),
                capture_output=True,
                text=True,
                env=env,
                cwd=workspace,
                check=True,
            )
            decision = json.loads(log.read_text().splitlines()[-1])
            answer = (
                json.loads(completed.stdout)["hookSpecificOutput"]["permissionDecisionReason"]
                if completed.stdout.strip()
                else None
            )
            kind = (
                "-"
                if answer is None
                else "call-site lines"
                if answer.startswith("CCM: this repeats")
                else "packet"
            )
            replayed.append(
                Replayed(
                    grep=grep,
                    decision=decision["decision"],
                    detail=decision["detail"],
                    answer_chars=None if answer is None else len(answer),
                    answer_kind=kind,
                )
            )
    return replayed


def main() -> None:
    """Tüm dökümleri depo depo yeniden oynatır ve özetler."""
    bin_dir, corpus, results = (Path(argument) for argument in sys.argv[1:4])
    pattern = sys.argv[4]
    tools = set(sys.argv[5].split(","))
    repo_of = {task.id: task.repo for task in KNOWN_TASKS}
    by_repo: dict[str, list[RecordedGrep]] = collections.defaultdict(list)
    for transcript in sorted(results.glob(pattern)):
        task = transcript.name.split("__")[0]
        by_repo[repo_of[task]].extend(recorded_greps(transcript, tools))
    replayed = [
        item
        for repo, greps in sorted(by_repo.items())
        for item in replay_repo(bin_dir, corpus, repo, greps)
    ]
    for item in replayed:
        searched = item.grep.tool_input.get("pattern") or item.grep.tool_input.get("command")
        pattern = str(searched).replace("\n", " ")[:28]
        detail = "" if item.decision == "deny" else item.detail[:70]
        print(
            f"{item.decision:4} {item.grep.tool:4} {item.grep.run[:42]:42} {pattern:28} "
            f"grep={item.grep.result_chars:6} answer={item.answer_chars} "
            f"[{item.answer_kind}] {detail}"
        )
    denied = [item for item in replayed if item.decision == "deny"]
    print(dict(collections.Counter(item.decision for item in replayed)))
    recorded = sum(item.grep.result_chars for item in denied)
    answered = sum(item.answer_chars or 0 for item in denied)
    print(f"answered greps: recorded grep output {recorded} chars, hook answers {answered} chars")


if __name__ == "__main__":
    main()
