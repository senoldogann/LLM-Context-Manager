"""Birden çok `ccm-cli retrieval-eval` raporunu tek rapora birleştirir.

SWE-bench Lite test kümesinde her örnek deposunun kendi `base_commit` durumunda indekslenir ve
ayrı bir raporda ölçülür; `stats` ve `cases` ise tek rapor okur. Girdilerin şema sürümü, ölçüm
commit'i ve ayarları (yöntemler ve sıraları, sınır, tekrarlar, ağırlıklar, donanım) aynı
olmalıdır; görev kimlikleri tüm girdilerde benzersiz olmalıdır. Her girdinin metrikleri görev
sonuçlarından `core/src/retrieval_eval.rs` ile aynı formüllerle yeniden hesaplanır ve
rapordakilere tam eşit olmalıdır.

Birleşik raporda her yöntemin görevleri kimliğe göre sıralanır, böylece girdilerin verilme sırası
görevlerin sırasını ve bootstrap örneklerini değiştirmez. Görev sonuçları değiştirilmeden
kopyalanır; görev dosyası ve proje özetleri girdi sırasıyla art arda eklenir; `generated_at` en
yeni girdininkidir; metrikler birleşik görevlerden aynı formüllerle hesaplanır.

Kullanım:
  uv run python -m retrieval.merge_reports --report a.json --report b.json --out merged.json
"""

from __future__ import annotations

import argparse
import functools
import json
import operator
from collections import Counter
from collections.abc import Sequence
from dataclasses import asdict, dataclass
from pathlib import Path

from freshness.jsonio import as_float, as_int, as_list, as_object, as_optional_int, as_str
from freshness.model import JsonValue

SUPPORTED_SCHEMA_VERSION: int = 1
MRR_DEPTH: int = 10


class MergeError(RuntimeError):
    """Rapor okunamadı, metrikleri görevlerle tutarsız ya da raporlar birleştirilemez."""


@dataclass(frozen=True)
class Metrics:
    tasks: int
    recall_at_1: float
    recall_at_5: float
    recall_at_10: float
    mrr_at_10: float
    latency_p50_ms: float
    latency_p95_ms: float


@dataclass(frozen=True)
class TaskRecord:
    task_id: str
    rank: int | None
    latencies_ms: tuple[float, ...]
    raw: dict[str, JsonValue]


@dataclass(frozen=True)
class MethodRecord:
    method: str
    metrics: Metrics
    tasks: tuple[TaskRecord, ...]


@dataclass(frozen=True)
class InputReport:
    path: str
    generated_at: int
    commit: str
    task_files: tuple[JsonValue, ...]
    tasks: int
    projects: tuple[JsonValue, ...]
    settings: dict[str, JsonValue]
    methods: tuple[MethodRecord, ...]


def recall_at(ranks: Sequence[int | None], depth: int) -> float:
    """İlk `depth` sonuçta bulunan görevlerin payı."""
    if not ranks:
        raise MergeError(f"Recall@{depth} needs at least one task")
    hits = sum(1 for rank in ranks if rank is not None and rank <= depth)
    return hits / len(ranks)


def mrr_at(ranks: Sequence[int | None], depth: int) -> float:
    """Sıra terslerinin ortalaması; `depth`'ten sonraki ya da bulunmayan eşleşme 0."""
    if not ranks:
        raise MergeError(f"MRR@{depth} needs at least one task")
    terms = (1.0 / rank if rank is not None and rank <= depth else 0.0 for rank in ranks)
    # `sum` Python 3.12'den beri telafili toplar; Rust terimleri görev sırasıyla tek tek ekler.
    return functools.reduce(operator.add, terms, 0.0) / len(ranks)


def percentile_nearest_rank(samples: Sequence[float], percent: int) -> float:
    """En yakın sıra yüzdeliği: artan sıralı örneklerin ⌈p·n/100⌉. elemanı (en az ilki)."""
    if not samples:
        raise MergeError("percentile needs at least one sample")
    if not 1 <= percent <= 100:
        raise MergeError(f"percentile {percent} is outside 1..=100")
    ordered = sorted(samples)
    rank = max((percent * len(ordered) + 99) // 100, 1)
    return ordered[rank - 1]


def method_metrics(tasks: Sequence[TaskRecord]) -> Metrics:
    """Görevlerin metrikleri; gecikmeler tüm görevlerin tüm tekrarları üzerinden."""
    ranks = [task.rank for task in tasks]
    latencies = [latency for task in tasks for latency in task.latencies_ms]
    return Metrics(
        tasks=len(tasks),
        recall_at_1=recall_at(ranks, 1),
        recall_at_5=recall_at(ranks, 5),
        recall_at_10=recall_at(ranks, 10),
        mrr_at_10=mrr_at(ranks, MRR_DEPTH),
        latency_p50_ms=percentile_nearest_rank(latencies, 50),
        latency_p95_ms=percentile_nearest_rank(latencies, 95),
    )


def read_report(path: Path) -> InputReport:
    """Raporu okur; yöntemlerin görevleri ve metrikleri tutarlı olmalı."""
    try:
        loaded: JsonValue = json.loads(path.read_bytes())
    except json.JSONDecodeError as error:
        raise MergeError(f"{path} is not valid JSON: {error}") from error
    root = as_object(loaded, str(path), MergeError)
    schema_version = as_int(root.get("schema_version"), f"{path}: schema_version", MergeError)
    if schema_version != SUPPORTED_SCHEMA_VERSION:
        raise MergeError(f"{path}: schema_version {schema_version} is not supported")
    dataset = as_object(root.get("dataset"), f"{path}: dataset", MergeError)
    settings = as_object(root.get("settings"), f"{path}: settings", MergeError)
    methods = tuple(
        _method_record(value, path)
        for value in as_list(root.get("methods"), f"{path}: methods", MergeError)
    )
    if not methods:
        raise MergeError(f"{path}: report has no methods")
    listed = [
        as_str(value, f"{path}: settings.methods", MergeError)
        for value in as_list(settings.get("methods"), f"{path}: settings.methods", MergeError)
    ]
    if listed != [method.method for method in methods]:
        raise MergeError(f"{path}: settings.methods {listed} do not match the method reports")
    task_count = as_int(dataset.get("tasks"), f"{path}: dataset.tasks", MergeError)
    task_ids = [task.task_id for task in methods[0].tasks]
    for method in methods:
        if [task.task_id for task in method.tasks] != task_ids:
            raise MergeError(f"{path}: method '{method.method}' has different tasks")
        if len(method.tasks) != task_count:
            raise MergeError(
                f"{path}: method '{method.method}' has {len(method.tasks)} tasks, "
                f"dataset.tasks is {task_count}"
            )
        recomputed = method_metrics(method.tasks)
        if recomputed != method.metrics:
            raise MergeError(
                f"{path}: method '{method.method}' metrics {method.metrics} differ from "
                f"recomputed {recomputed}"
            )
    return InputReport(
        path=str(path),
        generated_at=as_int(root.get("generated_at"), f"{path}: generated_at", MergeError),
        commit=as_str(root.get("commit"), f"{path}: commit", MergeError),
        task_files=tuple(
            as_list(dataset.get("task_files"), f"{path}: dataset.task_files", MergeError)
        ),
        tasks=task_count,
        projects=tuple(as_list(dataset.get("projects"), f"{path}: dataset.projects", MergeError)),
        settings=settings,
        methods=methods,
    )


def merge_reports(reports: Sequence[InputReport]) -> dict[str, JsonValue]:
    """Raporların birleşimi `retrieval-eval` raporunun biçiminde."""
    if not reports:
        raise MergeError("at least one report is required")
    first = reports[0]
    for report in reports[1:]:
        if report.commit != first.commit:
            raise MergeError(
                f"{report.path}: commit {report.commit} differs from {first.path}: {first.commit}"
            )
        if report.settings != first.settings:
            raise MergeError(f"{report.path}: settings differ from {first.path}")
    counts = Counter(task.task_id for report in reports for task in report.methods[0].tasks)
    duplicates = sorted(task_id for task_id, count in counts.items() if count > 1)
    if duplicates:
        raise MergeError(f"task ids occur in more than one report: {duplicates}")
    methods: list[JsonValue] = []
    for index, method in enumerate(first.methods):
        tasks = sorted(
            (task for report in reports for task in report.methods[index].tasks),
            key=lambda task: task.task_id,
        )
        raw_tasks: list[JsonValue] = [task.raw for task in tasks]
        methods.append(
            {
                "method": method.method,
                "metrics": asdict(method_metrics(tasks)),
                "tasks": raw_tasks,
            }
        )
    task_files: list[JsonValue] = [entry for report in reports for entry in report.task_files]
    projects: list[JsonValue] = [entry for report in reports for entry in report.projects]
    return {
        "schema_version": SUPPORTED_SCHEMA_VERSION,
        "generated_at": max(report.generated_at for report in reports),
        "commit": first.commit,
        "dataset": {
            "task_files": task_files,
            "tasks": sum(report.tasks for report in reports),
            "projects": projects,
        },
        "settings": first.settings,
        "methods": methods,
    }


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--report", type=Path, action="append", required=True)
    parser.add_argument("--out", type=Path, required=True)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    paths: list[Path] = args.report
    merged = merge_reports([read_report(path) for path in paths])
    out: Path = args.out
    out.write_text(json.dumps(merged, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
    dataset = as_object(merged["dataset"], "dataset", MergeError)
    print(f"merged {len(paths)} reports, {dataset['tasks']} tasks -> {out}")


def _method_record(value: JsonValue, path: Path) -> MethodRecord:
    method = as_object(value, f"{path}: method", MergeError)
    name = as_str(method.get("method"), f"{path}: method.method", MergeError)
    metrics = as_object(method.get("metrics"), f"{path}: {name}.metrics", MergeError)
    return MethodRecord(
        method=name,
        metrics=Metrics(
            tasks=as_int(metrics.get("tasks"), f"{path}: {name}.tasks", MergeError),
            recall_at_1=_metric(metrics, "recall_at_1", path, name),
            recall_at_5=_metric(metrics, "recall_at_5", path, name),
            recall_at_10=_metric(metrics, "recall_at_10", path, name),
            mrr_at_10=_metric(metrics, "mrr_at_10", path, name),
            latency_p50_ms=_metric(metrics, "latency_p50_ms", path, name),
            latency_p95_ms=_metric(metrics, "latency_p95_ms", path, name),
        ),
        tasks=tuple(
            _task_record(task, path)
            for task in as_list(method.get("tasks"), f"{path}: {name}.tasks", MergeError)
        ),
    )


def _metric(metrics: dict[str, JsonValue], key: str, path: Path, method: str) -> float:
    return as_float(metrics.get(key), f"{path}: {method}.{key}", MergeError)


def _task_record(value: JsonValue, path: Path) -> TaskRecord:
    task = as_object(value, f"{path}: task", MergeError)
    task_id = as_str(task.get("id"), f"{path}: task.id", MergeError)
    rank = as_optional_int(task.get("rank"), f"{path}: {task_id}.rank", MergeError)
    if rank is not None and rank < 1:
        raise MergeError(f"{path}: {task_id}.rank must be at least 1, got {rank}")
    return TaskRecord(
        task_id=task_id,
        rank=rank,
        latencies_ms=tuple(
            as_float(latency, f"{path}: {task_id}.latencies_ms", MergeError)
            for latency in as_list(
                task.get("latencies_ms"), f"{path}: {task_id}.latencies_ms", MergeError
            )
        ),
        raw=task,
    )


if __name__ == "__main__":
    main()
