"""`ccm-cli retrieval-eval` raporunun tipli okuyucusu.

Alanlar `core/src/retrieval_eval.rs` yapılarıyla aynıdır; yalnız istatistik ve vaka tablosunun
kullandığı alanlar okunur. Beklenmeyen biçim, desteklenmeyen şema sürümü ya da eşleşmeyen görev
kümeleri `ReportError` verir. Sıra tersleri kesir olarak döner; eşit farklar float yuvarlamasıyla
bozulmaz.
"""

from __future__ import annotations

import hashlib
import json
from dataclasses import dataclass
from fractions import Fraction
from pathlib import Path

from freshness.jsonio import (
    as_float,
    as_int,
    as_list,
    as_object,
    as_optional_int,
    as_str,
)
from freshness.model import JsonValue

SUPPORTED_SCHEMA_VERSION: int = 1
METRIC_DEPTH: int = 10
METHOD_LABELS: dict[str, str] = {"semantic": "S", "hybrid": "H0", "propagation": "H1"}


class ReportError(RuntimeError):
    """Rapor dosyası okunamadı ya da beklenen biçimde değil."""


@dataclass(frozen=True)
class RankedResult:
    node_id: str
    file_path: str
    score: float
    reason: str


@dataclass(frozen=True)
class TaskOutcome:
    task_id: str
    project: str
    gold_level: str
    gold_targets: tuple[str, ...]
    rank: int | None
    top_results: tuple[RankedResult, ...]


@dataclass(frozen=True)
class MethodReport:
    method: str
    mrr_at_10: float
    tasks: tuple[TaskOutcome, ...]


@dataclass(frozen=True)
class EvalReport:
    path: str
    sha256: str
    commit: str
    propagation_weight: float
    methods: tuple[MethodReport, ...]


def load_report(path: Path) -> EvalReport:
    """Raporu okur ve doğrular."""
    raw = path.read_bytes()
    try:
        loaded: JsonValue = json.loads(raw)
    except json.JSONDecodeError as error:
        raise ReportError(f"{path} is not valid JSON: {error}") from error
    root = as_object(loaded, str(path), ReportError)
    schema_version = as_int(root.get("schema_version"), "schema_version", ReportError)
    if schema_version != SUPPORTED_SCHEMA_VERSION:
        raise ReportError(
            f"{path}: schema_version {schema_version} is not {SUPPORTED_SCHEMA_VERSION}"
        )
    settings = as_object(root.get("settings"), "settings", ReportError)
    methods = tuple(
        _method_report(value) for value in as_list(root.get("methods"), "methods", ReportError)
    )
    if not methods:
        raise ReportError(f"{path}: report has no methods")
    return EvalReport(
        path=str(path),
        sha256=hashlib.sha256(raw).hexdigest(),
        commit=as_str(root.get("commit"), "commit", ReportError),
        propagation_weight=as_float(
            settings.get("propagation_weight"), "settings.propagation_weight", ReportError
        ),
        methods=methods,
    )


def find_method(report: EvalReport, name: str) -> MethodReport:
    """Adı verilen yöntemin sonuçları; raporda yoksa hata."""
    for method in report.methods:
        if method.method == name:
            return method
    raise ReportError(f"{report.path}: method '{name}' is not in the report")


def reciprocal_rank(rank: int | None, depth: int) -> Fraction:
    """Sıra tersi; `depth`'ten sonraki ya da bulunmayan eşleşme 0."""
    if rank is None or rank > depth:
        return Fraction(0)
    if rank < 1:
        raise ReportError(f"rank must be at least 1, got {rank}")
    return Fraction(1, rank)


def paired_tasks(
    first: MethodReport, second: MethodReport
) -> list[tuple[TaskOutcome, TaskOutcome]]:
    """İki yöntemin aynı görevlerdeki sonuçları, raporun görev sırasında."""
    first_ids = [task.task_id for task in first.tasks]
    second_ids = [task.task_id for task in second.tasks]
    if first_ids != second_ids:
        raise ReportError(
            f"methods '{first.method}' and '{second.method}' were measured on different tasks"
        )
    return list(zip(first.tasks, second.tasks, strict=True))


def _method_report(value: JsonValue) -> MethodReport:
    method = as_object(value, "method", ReportError)
    name = as_str(method.get("method"), "method.method", ReportError)
    metrics = as_object(method.get("metrics"), f"{name}.metrics", ReportError)
    tasks = tuple(
        _task_outcome(task) for task in as_list(method.get("tasks"), f"{name}.tasks", ReportError)
    )
    task_count = as_int(metrics.get("tasks"), f"{name}.metrics.tasks", ReportError)
    if task_count != len(tasks):
        raise ReportError(f"{name}: metrics count {task_count} tasks, outcomes {len(tasks)}")
    return MethodReport(
        method=name,
        mrr_at_10=as_float(metrics.get("mrr_at_10"), f"{name}.metrics.mrr_at_10", ReportError),
        tasks=tasks,
    )


def _task_outcome(value: JsonValue) -> TaskOutcome:
    task = as_object(value, "task", ReportError)
    task_id = as_str(task.get("id"), "task.id", ReportError)
    gold = as_object(task.get("gold"), f"{task_id}.gold", ReportError)
    return TaskOutcome(
        task_id=task_id,
        project=as_str(task.get("project"), f"{task_id}.project", ReportError),
        gold_level=as_str(gold.get("level"), f"{task_id}.gold.level", ReportError),
        gold_targets=tuple(
            as_str(target, f"{task_id}.gold.targets", ReportError)
            for target in as_list(gold.get("targets"), f"{task_id}.gold.targets", ReportError)
        ),
        rank=as_optional_int(task.get("rank"), f"{task_id}.rank", ReportError),
        top_results=tuple(
            _ranked_result(result, task_id)
            for result in as_list(task.get("top_results"), f"{task_id}.top_results", ReportError)
        ),
    )


def _ranked_result(value: JsonValue, task_id: str) -> RankedResult:
    result = as_object(value, f"{task_id}.top_results[]", ReportError)
    return RankedResult(
        node_id=as_str(result.get("node_id"), f"{task_id}.node_id", ReportError),
        file_path=as_str(result.get("file_path"), f"{task_id}.file_path", ReportError),
        score=as_float(result.get("score"), f"{task_id}.score", ReportError),
        reason=as_str(result.get("reason"), f"{task_id}.reason", ReportError),
    )
