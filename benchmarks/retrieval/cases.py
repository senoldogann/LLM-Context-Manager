"""Görev bazında vaka tablosu: graf yapısının sıralamaya yardım ettiği ve zarar verdiği görevler.

Aynı `retrieval-eval` raporundan her görev için yöntemlerin sıraları yan yana konur ve bir yapılı
yöntemin (H0 ya da H1) sırası yalnızca semantik sıralamayla (S) karşılaştırılır: `helped`
(sıra iyileşti ya da yalnız yapılı yöntem buldu), `hurt` (sıra kötüleşti ya da yalnız S buldu),
`same` (aynı sıra) ve `both_missed` (ikisi de ilk 10'da bulamadı). H1 gerekçesindeki semantik ve
destek değerleri mekanizmayı ayırır: yardımda altın sonuç bir semantik tohumsa `reranked`, tohum
değilse (semantik 0) ve yalnız komşu desteğiyle geldiyse `added_by_graph`; zararda altının
önüne geçenlerden biri tohum değilse `displaced_by_neighbor`, hepsi tohumsa
`displaced_by_supported_seed`. Gerekçeler iki ondalıkla yazıldığından semantik 0,00 tohum dışı
demektir: tohumların semantik skoru 1/(1 + uzaklık) birim vektörlerde en az 1/3'tür.

Kullanım:
  uv run python -m retrieval.cases --report h1_kehitys.json --out-json cases.json \\
    --out-markdown cases.md
"""

from __future__ import annotations

import argparse
import json
import re
from dataclasses import asdict, dataclass
from pathlib import Path

from retrieval.report import (
    METHOD_LABELS,
    METRIC_DEPTH,
    EvalReport,
    RankedResult,
    ReportError,
    TaskOutcome,
    find_method,
    load_report,
    paired_tasks,
)

SCHEMA_VERSION: int = 1
STRUCTURED_METHODS: tuple[str, ...] = ("hybrid", "propagation")
PROPAGATION_REASON = re.compile(
    r"^Propagation \(semantic (?P<semantic>\d+\.\d+), support (?P<support>\d+\.\d+), "
    r"weight (?P<weight>\d+\.\d+)\)$"
)


@dataclass(frozen=True)
class PropagationEvidence:
    semantic: float
    support: float


@dataclass(frozen=True)
class MethodCase:
    method: str
    rank: int | None
    outcome: str
    mechanism: str | None
    top_node: str | None


@dataclass(frozen=True)
class TaskCase:
    task_id: str
    project: str
    gold: list[str]
    semantic_rank: int | None
    semantic_top_node: str | None
    structured: list[MethodCase]


@dataclass(frozen=True)
class CasesReport:
    schema_version: int
    report: str
    report_sha256: str
    commit: str
    depth: int
    tasks: list[TaskCase]
    counts: dict[str, dict[str, int]]


def found_rank(rank: int | None) -> int | None:
    """İlk 10'daki sıra; daha sonrası bulunamamış sayılır."""
    return rank if rank is not None and rank <= METRIC_DEPTH else None


def outcome(semantic_rank: int | None, structured_rank: int | None) -> str:
    """Yapılı yöntemin S'ye göre sonucu: helped, hurt, same ya da both_missed."""
    semantic = found_rank(semantic_rank)
    structured = found_rank(structured_rank)
    if semantic is None and structured is None:
        return "both_missed"
    if semantic is None:
        return "helped"
    if structured is None:
        return "hurt"
    if structured < semantic:
        return "helped"
    if structured > semantic:
        return "hurt"
    return "same"


def propagation_evidence(result: RankedResult) -> PropagationEvidence:
    """H1 gerekçesinden semantik skor ve komşu desteği."""
    match = PROPAGATION_REASON.match(result.reason)
    if match is None:
        raise ReportError(f"{result.node_id}: not a propagation reason: {result.reason!r}")
    return PropagationEvidence(
        semantic=float(match.group("semantic")), support=float(match.group("support"))
    )


def propagation_mechanism(task: TaskOutcome, result_outcome: str) -> str | None:
    """H1'de yardımın ya da zararın nedeni; aynı sonuçta ya da ikisi de bulamazsa yok."""
    if result_outcome not in {"helped", "hurt"}:
        return None
    rank = found_rank(task.rank)
    if result_outcome == "helped":
        if rank is None:
            raise ReportError(f"{task.task_id}: helped without a rank")
        gold = propagation_evidence(task.top_results[rank - 1])
        return "reranked" if gold.semantic > 0 else "added_by_graph"
    above = task.top_results[: (rank - 1) if rank is not None else METRIC_DEPTH]
    if any(propagation_evidence(result).semantic == 0 for result in above):
        return "displaced_by_neighbor"
    return "displaced_by_supported_seed"


def method_case(semantic_task: TaskOutcome, task: TaskOutcome, method: str) -> MethodCase:
    result_outcome = outcome(semantic_task.rank, task.rank)
    return MethodCase(
        method=METHOD_LABELS[method],
        rank=task.rank,
        outcome=result_outcome,
        mechanism=propagation_mechanism(task, result_outcome) if method == "propagation" else None,
        top_node=task.top_results[0].node_id if task.top_results else None,
    )


def build_cases(report: EvalReport) -> CasesReport:
    """S'ye göre her yapılı yöntemin görev bazındaki sonuçları ve sayımları."""
    semantic = find_method(report, "semantic")
    names = {method.method for method in report.methods}
    structured = [name for name in STRUCTURED_METHODS if name in names]
    if not structured:
        raise ReportError(f"{report.path}: needs hybrid or propagation next to semantic")
    columns = [paired_tasks(semantic, find_method(report, name)) for name in structured]
    tasks = [
        TaskCase(
            task_id=semantic_task.task_id,
            project=semantic_task.project,
            gold=list(semantic_task.gold_targets),
            semantic_rank=semantic_task.rank,
            semantic_top_node=(
                semantic_task.top_results[0].node_id if semantic_task.top_results else None
            ),
            structured=[
                method_case(semantic_task, column[index][1], name)
                for name, column in zip(structured, columns, strict=True)
            ],
        )
        for index, semantic_task in enumerate(semantic.tasks)
    ]
    counts = {
        METHOD_LABELS[name]: count_outcomes(tasks, METHOD_LABELS[name]) for name in structured
    }
    return CasesReport(
        schema_version=SCHEMA_VERSION,
        report=report.path,
        report_sha256=report.sha256,
        commit=report.commit,
        depth=METRIC_DEPTH,
        tasks=tasks,
        counts=counts,
    )


def count_outcomes(tasks: list[TaskCase], label: str) -> dict[str, int]:
    """Bir yöntemin sonuç ve mekanizma sayıları."""
    counts: dict[str, int] = {}
    for task in tasks:
        for case in task.structured:
            if case.method != label:
                continue
            counts[case.outcome] = counts.get(case.outcome, 0) + 1
            if case.mechanism is not None:
                counts[case.mechanism] = counts.get(case.mechanism, 0) + 1
    return dict(sorted(counts.items()))


def rank_text(rank: int | None) -> str:
    return "-" if found_rank(rank) is None else str(rank)


def markdown(cases: CasesReport) -> str:
    """Vaka tablosu Markdown olarak."""
    labels = [case.method for case in cases.tasks[0].structured] if cases.tasks else []
    header = ["task", "project", "gold", "S", *labels, *(f"{label} vs S" for label in labels)]
    lines = [
        f"# Retrieval cases: {Path(cases.report).name}",
        "",
        f"Commit `{cases.commit}`, report sha256 `{cases.report_sha256}`, depth {cases.depth}.",
        "",
        "| " + " | ".join(header) + " |",
        "|" + "|".join("---" for _ in header) + "|",
    ]
    for task in cases.tasks:
        verdicts = [
            case.outcome if case.mechanism is None else f"{case.outcome} ({case.mechanism})"
            for case in task.structured
        ]
        row = [
            task.task_id,
            task.project,
            ", ".join(f"`{target}`" for target in task.gold),
            rank_text(task.semantic_rank),
            *(rank_text(case.rank) for case in task.structured),
            *verdicts,
        ]
        lines.append("| " + " | ".join(row) + " |")
    lines.append("")
    for label, counts in cases.counts.items():
        summary = ", ".join(f"{name} {count}" for name, count in counts.items())
        lines.append(f"- {label} vs S: {summary}")
    return "\n".join(lines) + "\n"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--out-json", type=Path, required=True)
    parser.add_argument("--out-markdown", type=Path, required=True)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    cases = build_cases(load_report(args.report))
    args.out_json.write_text(json.dumps(asdict(cases), indent=2) + "\n")
    rendered = markdown(cases)
    args.out_markdown.write_text(rendered)
    print(rendered, end="")


if __name__ == "__main__":
    main()
