"""Ön-kayıtlı araç planları (v1, v2) ve cevap ayrıştırma.

v1 cevapları blok biçimindedir (`## Tür: ad (Score: …)`, `**Node ID:**`, `**File:**`,
`**Range:**`); v2 cevapları satır biçimindedir (`- Tür: ad · yol:başlangıç-bitiş · …`)
ve belirsiz hedefte adayları `- yol:satır …` satırlarıyla listeler.
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass

from freshness.mcp import McpClient
from freshness.model import JsonValue
from tokens.model import CallRecord, QuestionResult

V1_HEADING = re.compile(r"^## (?P<kind>[A-Za-z][A-Za-z ]*): (?P<name>.+?) \(Score: [^)]*\)\s*$")
V1_NODE_ID = re.compile(r"^\*\*Node ID:\*\* (?P<id>\S+)")
V1_FILE = re.compile(r"^\*\*File:\*\* (?P<file>\S+)")
V1_RANGE = re.compile(r"^\*\*Range:\*\* (?P<start>\d+)-(?P<end>\d+)")
V2_LINE = re.compile(r"^- .+? · (?P<path>\S+):(?P<start>\d+)-(?P<end>\d+)")
V2_CANDIDATE = re.compile(r"^- (?P<path>[^\s:]+):(?P<line>\d+)\b")


@dataclass(frozen=True)
class V1Block:
    """v1 blok cevabındaki bir düğüm."""

    name: str
    node_id: str
    file: str
    start: int


class PlanSession:
    """Araç çağrılarını yapan ve her çağrıyı kaydeden oturum."""

    def __init__(self, client: McpClient, timeout_s: float) -> None:
        self._client = client
        self._timeout_s = timeout_s

    def call(self, tool: str, arguments: dict[str, JsonValue]) -> tuple[CallRecord, str]:
        """Aracı çağırır; kayıt ve cevabın metni döner."""
        response = self._client.call_tool(tool, arguments, self._timeout_s)
        text = "\n".join(response.texts)
        record = CallRecord(
            tool=tool,
            arguments=json.dumps(arguments, sort_keys=True),
            response_bytes=len(text.encode("utf-8")),
            is_error=response.is_error,
        )
        return record, text


def strip_dot(path: str) -> str:
    """Dosya kimliğinin baştaki `./` önekini atar."""
    return path.removeprefix("./")


def v1_blocks(text: str) -> tuple[V1Block, ...]:
    """v1 blok cevabındaki düğümler (ad, kimlik, dosya, başlangıç satırı)."""
    blocks: list[V1Block] = []
    name: str | None = None
    node_id: str | None = None
    file: str | None = None
    for line in text.splitlines():
        heading = V1_HEADING.match(line)
        if heading is not None:
            name, node_id, file = heading["name"], None, None
            continue
        if (match := V1_NODE_ID.match(line)) is not None:
            node_id = match["id"]
        elif (match := V1_FILE.match(line)) is not None:
            file = match["file"]
        elif (match := V1_RANGE.match(line)) is not None:
            if name is not None and node_id is not None and file is not None:
                blocks.append(
                    V1Block(name=name, node_id=node_id, file=file, start=int(match["start"]))
                )
            name, node_id, file = None, None, None
    return tuple(blocks)


def v1_locations(text: str) -> tuple[str, ...]:
    """v1 cevabındaki düğüm konumları (`yol:başlangıç`)."""
    return tuple(f"{strip_dot(block.file)}:{block.start}" for block in v1_blocks(text))


def v2_locations(text: str) -> tuple[str, ...]:
    """v2 satır cevabındaki konumlar (`yol:başlangıç`)."""
    locations: list[str] = []
    for line in text.splitlines():
        match = V2_LINE.match(line)
        if match is not None:
            locations.append(f"{strip_dot(match['path'])}:{match['start']}")
    return tuple(locations)


def v2_candidates(text: str) -> tuple[str, ...]:
    """Belirsiz hedef hatasındaki adaylar (`yol:satır`)."""
    candidates: list[str] = []
    for line in text.splitlines():
        match = V2_CANDIDATE.match(line)
        if match is not None:
            candidates.append(f"{match['path']}:{match['line']}")
    return tuple(candidates)


def exact_matches(text: str, symbol: str, max_matches: int) -> tuple[V1Block, ...]:
    """`find_nodes` cevabında adı tam olarak `symbol` olan ilk `max_matches` düğüm."""
    return tuple(block for block in v1_blocks(text) if block.name == symbol)[:max_matches]


def v1_callers(session: PlanSession, symbol: str, max_matches: int) -> QuestionResult:
    """v1: `find_nodes`, sonra her tam eşleşme için `find_usages(node_id)`."""
    records: list[CallRecord] = []
    locations: list[str] = []
    record, text = session.call("find_nodes", {"query": symbol, "limit": 50})
    records.append(record)
    for block in exact_matches(text, symbol, max_matches):
        record, usages = session.call("find_usages", {"node_id": block.node_id})
        records.append(record)
        if not record.is_error:
            locations.extend(v1_locations(usages))
    return QuestionResult("callers", symbol, tuple(records), tuple(locations))


def v1_explain(session: PlanSession, symbol: str, max_matches: int) -> QuestionResult:
    """v1: `find_nodes`, sonra her tam eşleşme için `read_graph` ve `get_context`."""
    records: list[CallRecord] = []
    record, text = session.call("find_nodes", {"query": symbol, "limit": 50})
    records.append(record)
    for block in exact_matches(text, symbol, max_matches):
        record, _ = session.call("read_graph", {"node_id": block.node_id})
        records.append(record)
        record, _ = session.call(
            "get_context",
            {"file": strip_dot(block.file), "line": block.start, "include_body": True},
        )
        records.append(record)
    return QuestionResult("explain", symbol, tuple(records), ())


def v2_targeted(
    session: PlanSession, tool: str, kind: str, symbol: str, max_matches: int
) -> QuestionResult:
    """v2: `tool(target=symbol)`; belirsizse listelenen her aday için yeniden çağrı."""
    records: list[CallRecord] = []
    locations: list[str] = []
    record, text = session.call(tool, {"target": symbol})
    records.append(record)
    if record.is_error:
        for candidate in v2_candidates(text)[:max_matches]:
            record, answer = session.call(tool, {"target": candidate})
            records.append(record)
            if not record.is_error:
                locations.extend(v2_locations(answer))
    else:
        locations.extend(v2_locations(text))
    return QuestionResult(
        kind, symbol, tuple(records), tuple(locations) if kind == "callers" else ()
    )


def impact(session: PlanSession, file: str) -> QuestionResult:
    """Her iki sürüm: `impact_of_change(file)`."""
    record, _ = session.call("impact_of_change", {"file": file})
    return QuestionResult("impact", file, (record,), ())


def overview(session: PlanSession) -> QuestionResult:
    """Yalnız v2: `map()`."""
    record, _ = session.call("map", {})
    return QuestionResult("map", "", (record,), ())
