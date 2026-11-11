"""Kayıtlı Codex `context` sorgularını bir CCM ikilisinden yeniden geçirir (çevrimdışı, ajan yok).

Kullanım (`benchmarks/` içinde):
    PYTHONPATH=. uv run python scripts/context_replay.py <ccm bin dizini> <korpus dizini> \\
        <sonuç dizini> <döküm deseni>

Döküm deseni sonuç dizinine göre bir glob'dur (ör.
`heldout17-20261111/codex/transcripts/tokio-*__D__*.jsonl`). Her depo bir kez graph-only
indekslenir; her kayıtlı `context` sorgusu aynı argümanlarla MCP üzerinden sorulur. Her sorgu
için şu satır yazılır: koşu, sorgu, paket uzunluğu, paket türü (`complete`, `not complete`,
`not in index`, `candidates`, `other`), görünmeyen çağrı sayan "+N more" satırı var mı, bir
çağrı satırı tekrar ediyor mu ve "Files with calls" satırındaki dosyalar.
"""

import json
import re
import subprocess
import sys
import tempfile
from collections import defaultdict
from dataclasses import dataclass
from pathlib import Path

import agent.context_preflight as preflight
from agent.run import index_db_path, prepare_workspace
from agent.tasks import KNOWN_TASKS
from freshness.model import JsonValue


@dataclass(frozen=True)
class RecordedQuery:
    """Dökümdeki bir `context` çağrısının argümanları."""

    run: str
    query: str
    max_tokens: int | None


@dataclass(frozen=True)
class Replayed:
    """Bir sorgunun yeniden üretilmiş paketi hakkında ölçüler."""

    query: RecordedQuery
    chars: int
    kind: str
    more_lines: bool
    repeated_site: bool
    files: tuple[str, ...]


def recorded_queries(transcript: Path) -> list[RecordedQuery]:
    """Codex dökümündeki `context` çağrıları, sırasıyla."""
    run = transcript.name.split(".")[0]
    queries = []
    for line in transcript.read_text().splitlines():
        event = json.loads(line)
        item = event.get("item") or {}
        if (
            event.get("type") == "item.completed"
            and item.get("type") == "mcp_tool_call"
            and item.get("tool") == "context"
        ):
            arguments = item.get("arguments") or {}
            queries.append(RecordedQuery(run, arguments["query"], arguments.get("max_tokens")))
    return queries


def packet_kind(text: str) -> str:
    """Paketin türü."""
    if "is not in the index" in text:
        return "not in index"
    if "matches 2 symbols" in text or "different code targets" in text or "matches " in text[:200]:
        return "candidates"
    if "Check: call list complete." in text:
        return "complete"
    if "Check: NOT complete" in text:
        return "not complete"
    return "other"


def measure(query: RecordedQuery, text: str) -> Replayed:
    """Paket ölçüleri."""
    site_lines = re.findall(r"^  (\d+): .*$", text, re.MULTILINE)
    callers = text.split("callers:", 1)[1] if "callers:" in text else ""
    repeated = any(
        block.count(line) > 1
        for block in callers.split("\n- ")
        for line in re.findall(r"^  \d+: .*$", block, re.MULTILINE)
    )
    files_match = re.search(r"^Files with calls \(\d+\): (.*)$", text, re.MULTILINE)
    return Replayed(
        query=query,
        chars=len(text),
        kind=packet_kind(text),
        more_lines=re.search(r"^  \+\d+ more$", text, re.MULTILINE) is not None
        and bool(site_lines),
        repeated_site=repeated,
        files=tuple(files_match.group(1).split(", ")) if files_match else (),
    )


def replay_repo(
    bin_dir: Path, corpus: Path, repo: str, queries: list[RecordedQuery]
) -> list[Replayed]:
    """Bir deponun sorgularını tek MCP oturumunda yeniden sorar."""
    replayed = []
    with tempfile.TemporaryDirectory() as scratch:
        root = Path(scratch)
        home = root / "home"
        home.mkdir()
        workspace = root / "ws"
        prepare_workspace(corpus / repo, workspace)
        preflight.index_workspace(bin_dir / "ccm-cli", workspace, home)
        env = {
            **preflight.base_env(home),
            "CCM_PROJECT_ROOT": str(workspace),
            "CCM_ALLOWED_ROOTS": str(workspace),
            "CCM_DB_PATH": str(index_db_path(workspace)),
            "CCM_AUTO_REFRESH": "0",
        }
        process = subprocess.Popen(
            [str(bin_dir / "ccm-mcp")],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            text=True,
            env=env,
            cwd=workspace,
        )
        if process.stdin is None or process.stdout is None:
            raise RuntimeError("ccm-mcp pipes were not opened")

        def call(number: int, method: str, params: dict[str, JsonValue]) -> dict[str, JsonValue]:
            assert process.stdin is not None and process.stdout is not None
            message = {"jsonrpc": "2.0", "id": number, "method": method, "params": params}
            process.stdin.write(json.dumps(message) + "\n")
            process.stdin.flush()
            while True:
                response: dict[str, JsonValue] = json.loads(process.stdout.readline())
                if response.get("id") == number:
                    return response

        call(1, "initialize", {"protocolVersion": "2025-06-18", "capabilities": {}})
        for number, query in enumerate(queries, 2):
            arguments: dict[str, JsonValue] = {"query": query.query, "project_path": str(workspace)}
            if query.max_tokens is not None:
                arguments["max_tokens"] = query.max_tokens
            response = call(number, "tools/call", {"name": "context", "arguments": arguments})
            result = response.get("result")
            content = result.get("content") if isinstance(result, dict) else None
            if not isinstance(content, list):
                raise RuntimeError(f"{query.run}: unexpected response {response}")
            text = "".join(str(item.get("text", "")) for item in content if isinstance(item, dict))
            replayed.append(measure(query, text))
        process.kill()
    return replayed


def main() -> None:
    """Dökümleri depo depo yeniden oynatır ve satır satır yazar."""
    bin_dir, corpus, results = (Path(argument) for argument in sys.argv[1:4])
    repo_of = {task.id: task.repo for task in KNOWN_TASKS}
    by_repo: dict[str, list[RecordedQuery]] = defaultdict(list)
    for transcript in sorted(results.glob(sys.argv[4])):
        by_repo[repo_of[transcript.name.split("__")[0]]].extend(recorded_queries(transcript))
    for repo, queries in sorted(by_repo.items()):
        for item in replay_repo(bin_dir, corpus, repo, queries):
            print(
                f"{item.query.run}\t{item.query.query[:60]}\t{item.chars}\t{item.kind}\t"
                f"more={item.more_lines}\trepeat={item.repeated_site}\tfiles={','.join(item.files)}"
            )


if __name__ == "__main__":
    main()
