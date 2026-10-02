"""Token kıyasının veri modeli ve JSON biçimi (şema 1)."""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path

from freshness.jsonio import as_bool, as_int, as_list, as_object, as_str
from freshness.model import JsonValue

SCHEMA_VERSION = 1


class TokenBenchError(RuntimeError):
    """Token kıyası tutarsız bir girdi ya da durumla karşılaştı."""


@dataclass(frozen=True)
class CallRecord:
    """Bir MCP araç çağrısı: araç, argümanlar (JSON) ve cevap boyutu."""

    tool: str
    arguments: str
    response_bytes: int
    is_error: bool


@dataclass(frozen=True)
class QuestionResult:
    """Bir sorunun cevabı için yapılan çağrılar ve cevaplarda görülen konumlar."""

    kind: str
    subject: str
    calls: tuple[CallRecord, ...]
    locations: tuple[str, ...]


@dataclass(frozen=True)
class RepoResult:
    """Bir repo için oturum sabit yükü ve soruların sonuçları."""

    repo: str
    tools_list_bytes: int
    questions: tuple[QuestionResult, ...]


@dataclass(frozen=True)
class RunResult:
    """Bir sürümün (v1 ya da v2) tüm ölçümü."""

    schema_version: int
    version: str
    harness_revision: str
    started_at: str
    skill_bytes: int
    repos: tuple[RepoResult, ...]


def call_to_json(call: CallRecord) -> dict[str, JsonValue]:
    """Çağrı kaydını JSON nesnesine çevirir."""
    return {
        "tool": call.tool,
        "arguments": call.arguments,
        "response_bytes": call.response_bytes,
        "is_error": call.is_error,
    }


def run_to_json(run: RunResult) -> dict[str, JsonValue]:
    """Ölçümü JSON nesnesine çevirir."""
    repos: list[JsonValue] = []
    for repo in run.repos:
        questions: list[JsonValue] = []
        for question in repo.questions:
            calls: list[JsonValue] = [call_to_json(call) for call in question.calls]
            locations: list[JsonValue] = list(question.locations)
            questions.append(
                {
                    "kind": question.kind,
                    "subject": question.subject,
                    "calls": calls,
                    "locations": locations,
                }
            )
        repos.append(
            {
                "repo": repo.repo,
                "tools_list_bytes": repo.tools_list_bytes,
                "questions": questions,
            }
        )
    return {
        "schema_version": run.schema_version,
        "version": run.version,
        "harness_revision": run.harness_revision,
        "started_at": run.started_at,
        "skill_bytes": run.skill_bytes,
        "repos": repos,
    }


def call_from_json(value: JsonValue) -> CallRecord:
    """JSON nesnesinden çağrı kaydı."""
    data = as_object(value, "call", TokenBenchError)
    return CallRecord(
        tool=as_str(data.get("tool"), "call.tool", TokenBenchError),
        arguments=as_str(data.get("arguments"), "call.arguments", TokenBenchError),
        response_bytes=as_int(data.get("response_bytes"), "call.response_bytes", TokenBenchError),
        is_error=as_bool(data.get("is_error"), "call.is_error", TokenBenchError),
    )


def question_from_json(value: JsonValue) -> QuestionResult:
    """JSON nesnesinden soru sonucu."""
    data = as_object(value, "question", TokenBenchError)
    return QuestionResult(
        kind=as_str(data.get("kind"), "question.kind", TokenBenchError),
        subject=as_str(data.get("subject"), "question.subject", TokenBenchError),
        calls=tuple(
            call_from_json(item)
            for item in as_list(data.get("calls"), "question.calls", TokenBenchError)
        ),
        locations=tuple(
            as_str(item, "question.location", TokenBenchError)
            for item in as_list(data.get("locations"), "question.locations", TokenBenchError)
        ),
    )


def run_from_json(value: JsonValue) -> RunResult:
    """JSON nesnesinden ölçüm; şema sürümü uyuşmazsa açık hata."""
    data = as_object(value, "run", TokenBenchError)
    schema_version = as_int(data.get("schema_version"), "schema_version", TokenBenchError)
    if schema_version != SCHEMA_VERSION:
        raise TokenBenchError(f"results schema {schema_version} != {SCHEMA_VERSION}")
    repos: list[RepoResult] = []
    for item in as_list(data.get("repos"), "repos", TokenBenchError):
        repo = as_object(item, "repo", TokenBenchError)
        repos.append(
            RepoResult(
                repo=as_str(repo.get("repo"), "repo.repo", TokenBenchError),
                tools_list_bytes=as_int(
                    repo.get("tools_list_bytes"), "repo.tools_list_bytes", TokenBenchError
                ),
                questions=tuple(
                    question_from_json(question)
                    for question in as_list(
                        repo.get("questions"), "repo.questions", TokenBenchError
                    )
                ),
            )
        )
    return RunResult(
        schema_version=schema_version,
        version=as_str(data.get("version"), "version", TokenBenchError),
        harness_revision=as_str(data.get("harness_revision"), "harness_revision", TokenBenchError),
        started_at=as_str(data.get("started_at"), "started_at", TokenBenchError),
        skill_bytes=as_int(data.get("skill_bytes"), "skill_bytes", TokenBenchError),
        repos=tuple(repos),
    )


def write_run(path: Path, run: RunResult) -> None:
    """Ölçümü atomik olarak yazar (geçici dosya + yeniden adlandırma)."""
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(path.suffix + ".tmp")
    temporary.write_text(json.dumps(run_to_json(run), indent=1) + "\n", encoding="utf-8")
    temporary.replace(path)


def read_run(path: Path) -> RunResult:
    """Ölçüm dosyasını okur."""
    loaded: JsonValue = json.loads(path.read_text(encoding="utf-8"))
    return run_from_json(loaded)
