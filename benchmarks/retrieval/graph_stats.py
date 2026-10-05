"""İndekslenmiş projelerin kod grafı istatistikleri: düğüm ve kenar sayıları türüne göre.

Etkin generation, `resolve_index_artifacts` ile aynı yerleşimden çözülür: `data/ccm_current`
işaretçisi `data/.ccm-generations/<kimlik>/` dizinini gösterir; graf `ccm_graph.json`, indekslenen
commit `ccm_manifest.json` dosyasındadır. İşaretçisiz eski yerleşim desteklenmez: ölçülen indeks
generation kimliğiyle raporlanabilmelidir. Çıktı proje başına toplamları ve tür dağılımlarını
içerir.

Kullanım:
  uv run python -m retrieval.graph_stats --project express=corpus/express \\
    --project flask=corpus/flask --out graph_stats.json
"""

from __future__ import annotations

import argparse
import json
from collections import Counter
from dataclasses import asdict, dataclass
from pathlib import Path

from freshness.jsonio import as_int, as_list, as_object, as_str
from freshness.model import JsonValue

SCHEMA_VERSION: int = 1
CURRENT_GENERATION_FILE: str = "ccm_current"
GENERATIONS_DIRECTORY: str = ".ccm-generations"
EDGE_FIELDS: int = 3


class GraphStatsError(RuntimeError):
    """İndeks artefaktı eksik ya da beklenen biçimde değil."""


@dataclass(frozen=True)
class ProjectArgument:
    name: str
    root: Path


@dataclass(frozen=True)
class ProjectGraphStats:
    name: str
    root: str
    generation_id: str
    indexed_commit: str
    nodes: int
    edges: int
    node_types: dict[str, int]
    edge_types: dict[str, int]


@dataclass(frozen=True)
class GraphStatsReport:
    schema_version: int
    projects: list[ProjectGraphStats]


def parse_project(raw: str) -> ProjectArgument:
    """`ad=yol` biçimindeki proje argümanı."""
    name, separator, root = raw.partition("=")
    if not separator or not name or not root:
        raise argparse.ArgumentTypeError(f"expected NAME=PATH, got {raw!r}")
    return ProjectArgument(name=name, root=Path(root))


def generation_dir(root: Path) -> tuple[str, Path]:
    """Etkin generation kimliği ve dizini."""
    pointer = root / "data" / CURRENT_GENERATION_FILE
    if not pointer.is_file():
        raise GraphStatsError(f"{pointer} is missing; index the project first")
    generation_id = pointer.read_text().strip()
    directory = root / "data" / GENERATIONS_DIRECTORY / generation_id
    if not generation_id or not directory.is_dir():
        raise GraphStatsError(f"active generation '{generation_id}' is missing at {directory}")
    return generation_id, directory


def read_json(path: Path) -> JsonValue:
    try:
        loaded: JsonValue = json.loads(path.read_bytes())
    except json.JSONDecodeError as error:
        raise GraphStatsError(f"{path} is not valid JSON: {error}") from error
    return loaded


def type_counts(graph: dict[str, JsonValue]) -> tuple[Counter[str], Counter[str]]:
    """Düğüm ve kenar türlerinin sayıları; kenarlar `[kaynak, hedef, tür]` üçlüleridir."""
    node_types: Counter[str] = Counter(
        as_str(
            as_object(node, "node", GraphStatsError).get("node_type"), "node_type", GraphStatsError
        )
        for node in as_list(graph.get("nodes"), "nodes", GraphStatsError)
    )
    edge_types: Counter[str] = Counter()
    for edge in as_list(graph.get("edges"), "edges", GraphStatsError):
        fields = as_list(edge, "edge", GraphStatsError)
        if len(fields) != EDGE_FIELDS:
            raise GraphStatsError(f"edge must have {EDGE_FIELDS} fields, got {fields}")
        as_int(fields[0], "edge source", GraphStatsError)
        as_int(fields[1], "edge target", GraphStatsError)
        edge_types[as_str(fields[2], "edge type", GraphStatsError)] += 1
    return node_types, edge_types


def project_stats(project: ProjectArgument) -> ProjectGraphStats:
    """Bir projenin etkin generation'ındaki graf istatistikleri."""
    generation_id, directory = generation_dir(project.root)
    graph = as_object(read_json(directory / "ccm_graph.json"), "graph", GraphStatsError)
    manifest = as_object(read_json(directory / "ccm_manifest.json"), "manifest", GraphStatsError)
    node_types, edge_types = type_counts(graph)
    return ProjectGraphStats(
        name=project.name,
        root=str(project.root),
        generation_id=generation_id,
        indexed_commit=as_str(
            manifest.get("indexed_commit"), "manifest.indexed_commit", GraphStatsError
        ),
        nodes=node_types.total(),
        edges=edge_types.total(),
        node_types=dict(sorted(node_types.items())),
        edge_types=dict(sorted(edge_types.items())),
    )


def summary_lines(report: GraphStatsReport) -> list[str]:
    lines = ["project  nodes  edges  generation  indexed_commit"]
    lines.extend(
        f"{project.name}  {project.nodes}  {project.edges}  {project.generation_id}  "
        f"{project.indexed_commit}"
        for project in report.projects
    )
    return lines


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--project", type=parse_project, action="append", required=True)
    parser.add_argument("--out", type=Path, required=True)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    projects: list[ProjectArgument] = args.project
    report = GraphStatsReport(
        schema_version=SCHEMA_VERSION, projects=[project_stats(p) for p in projects]
    )
    args.out.write_text(json.dumps(asdict(report), indent=2) + "\n")
    print("\n".join(summary_lines(report)))


if __name__ == "__main__":
    main()
