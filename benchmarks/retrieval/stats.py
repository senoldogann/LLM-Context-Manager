"""Sıralama yöntemlerinin görev bazında eşleştirilmiş karşılaştırması: MRR@10 farkı.

Aynı `retrieval-eval` raporundaki her yöntem çifti (H0−S, H1−S, H1−H0) için görev başına sıra
tersleri (RR@10; 10'dan sonraki ya da bulunmayan eşleşme 0) eşleştirilir. Fark ilk yöntemin
MRR@10'u eksi ikincininkidir. Anlamlılık iki yönlü Wilcoxon işaretli sıralar testiyle tam
dağılımdan hesaplanır: sıfır farklar atılır, eşit mutlak farklar ortalama sıra alır ve dağılım
bu sıralarla koşullu olarak sayılır. Güven aralığı, görevlerin yerine koyarak yeniden
örneklendiği yüzdelik bootstrap'tir; her çift aynı tohumla aynı örnekleri kullanır ve %95
aralığın sınırları sıralanmış B ortalamanın ⌊0,025·B⌋ ve ⌈0,975·B⌉−1 indeksli değerleridir.
Sıra tersleri kesirle hesaplanır; raporun MRR@10 değeri yeniden hesaplanandan sapıyorsa hata
verilir.

Kullanım:
  uv run python -m retrieval.stats --report h1_kehitys.json --resamples 10000 \\
    --seed 20261005 --out h1_kehitys.stats.json
"""

from __future__ import annotations

import argparse
import json
import math
import random
from collections.abc import Sequence
from dataclasses import asdict, dataclass
from fractions import Fraction
from pathlib import Path

from retrieval.report import (
    METHOD_LABELS,
    METRIC_DEPTH,
    EvalReport,
    MethodReport,
    ReportError,
    find_method,
    load_report,
    paired_tasks,
    reciprocal_rank,
)

SCHEMA_VERSION: int = 1
CONFIDENCE: Fraction = Fraction(95, 100)
PAIRS: tuple[tuple[str, str], ...] = (
    ("hybrid", "semantic"),
    ("propagation", "semantic"),
    ("propagation", "hybrid"),
)
MRR_TOLERANCE: float = 1e-9


@dataclass(frozen=True)
class WilcoxonResult:
    nonzero: int
    w_plus: float
    w_minus: float
    p_value: float


@dataclass(frozen=True)
class PairComparison:
    pair: str
    first: str
    second: str
    tasks: int
    delta_mrr10: float
    ci_low: float
    ci_high: float
    wins: int
    losses: int
    ties: int
    wilcoxon: WilcoxonResult


@dataclass(frozen=True)
class StatsReport:
    schema_version: int
    report: str
    report_sha256: str
    commit: str
    depth: int
    resamples: int
    seed: int
    confidence: float
    mrr10: dict[str, float]
    pairs: list[PairComparison]


def doubled_midranks(values: Sequence[Fraction]) -> list[int]:
    """Değerlerin ortalama sıralarının iki katı, girdi sırasında; eşitler aynı sırayı paylaşır.

    1'den başlayan sıralarda i..j konumlarını kaplayan eşit grubun ortalama sırası
    (i + j + 2) / 2'dir; iki katı tamsayı olduğundan dağılım tamsayılarla sayılır.
    """
    order = sorted(range(len(values)), key=lambda index: values[index])
    ranks = [0] * len(values)
    start = 0
    while start < len(order):
        end = start
        while end + 1 < len(order) and values[order[end + 1]] == values[order[start]]:
            end += 1
        for position in range(start, end + 1):
            ranks[order[position]] = start + end + 2
        start = end + 1
    return ranks


def wilcoxon_signed_rank(differences: Sequence[Fraction]) -> WilcoxonResult:
    """İki yönlü tam Wilcoxon işaretli sıralar testi; fark yoksa p = 1."""
    nonzero = [difference for difference in differences if difference != 0]
    if not nonzero:
        return WilcoxonResult(nonzero=0, w_plus=0.0, w_minus=0.0, p_value=1.0)
    ranks = doubled_midranks([abs(difference) for difference in nonzero])
    observed = sum(rank for rank, difference in zip(ranks, nonzero, strict=True) if difference > 0)
    counts = [1]
    for rank in ranks:
        counts = [
            without + with_rank
            for without, with_rank in zip(counts + [0] * rank, [0] * rank + counts, strict=True)
        ]
    lower = sum(counts[: observed + 1])
    upper = sum(counts[observed:])
    p_value = min(Fraction(1), Fraction(2 * min(lower, upper), 2 ** len(nonzero)))
    return WilcoxonResult(
        nonzero=len(nonzero),
        w_plus=observed / 2,
        w_minus=(sum(ranks) - observed) / 2,
        p_value=float(p_value),
    )


def bootstrap_mean_ci(
    differences: Sequence[float], resamples: int, seed: int
) -> tuple[float, float]:
    """Ortalama farkın yüzdelik bootstrap güven aralığı (CONFIDENCE düzeyinde)."""
    if not differences:
        raise ValueError("bootstrap needs at least one difference")
    if resamples < 1:
        raise ValueError(f"resamples must be at least 1, got {resamples}")
    rng = random.Random(seed)
    count = len(differences)
    means = sorted(
        sum(differences[index] for index in rng.choices(range(count), k=count)) / count
        for _ in range(resamples)
    )
    tail = (1 - CONFIDENCE) / 2
    low_index = math.floor(tail * resamples)
    high_index = math.ceil((1 - tail) * resamples) - 1
    return means[low_index], means[high_index]


def compare(first: MethodReport, second: MethodReport, resamples: int, seed: int) -> PairComparison:
    """İki yöntemin eşleştirilmiş MRR@10 karşılaştırması."""
    pairs = paired_tasks(first, second)
    differences = [
        reciprocal_rank(a.rank, METRIC_DEPTH) - reciprocal_rank(b.rank, METRIC_DEPTH)
        for a, b in pairs
    ]
    low, high = bootstrap_mean_ci([float(value) for value in differences], resamples, seed)
    return PairComparison(
        pair=f"{METHOD_LABELS[first.method]}-{METHOD_LABELS[second.method]}",
        first=first.method,
        second=second.method,
        tasks=len(pairs),
        delta_mrr10=float(sum(differences, Fraction(0)) / len(differences)),
        ci_low=low,
        ci_high=high,
        wins=sum(1 for value in differences if value > 0),
        losses=sum(1 for value in differences if value < 0),
        ties=sum(1 for value in differences if value == 0),
        wilcoxon=wilcoxon_signed_rank(differences),
    )


def recomputed_mrr(method: MethodReport) -> float:
    """Görev sıralarından yeniden hesaplanan MRR@10; rapordakinden sapıyorsa hata."""
    if not method.tasks:
        raise ReportError(f"method '{method.method}' has no tasks")
    total = sum((reciprocal_rank(task.rank, METRIC_DEPTH) for task in method.tasks), Fraction(0))
    value = float(total / len(method.tasks))
    if abs(value - method.mrr_at_10) > MRR_TOLERANCE:
        raise ReportError(
            f"method '{method.method}': report MRR@10 {method.mrr_at_10} != recomputed {value}"
        )
    return value


def build_stats(report: EvalReport, resamples: int, seed: int) -> StatsReport:
    """Rapordaki tüm yöntemlerin MRR@10 değerleri ve mevcut çiftlerin karşılaştırmaları."""
    names = {method.method for method in report.methods}
    unknown = sorted(names - METHOD_LABELS.keys())
    if unknown:
        raise ReportError(f"{report.path}: unknown methods {unknown}")
    pairs = [
        compare(find_method(report, first), find_method(report, second), resamples, seed)
        for first, second in PAIRS
        if first in names and second in names
    ]
    if not pairs:
        raise ReportError(f"{report.path}: needs at least two methods to compare")
    return StatsReport(
        schema_version=SCHEMA_VERSION,
        report=report.path,
        report_sha256=report.sha256,
        commit=report.commit,
        depth=METRIC_DEPTH,
        resamples=resamples,
        seed=seed,
        confidence=float(CONFIDENCE),
        mrr10={METHOD_LABELS[m.method]: recomputed_mrr(m) for m in report.methods},
        pairs=pairs,
    )


def summary_lines(stats: StatsReport) -> list[str]:
    """Çiftlerin okunabilir özeti."""
    lines = [f"tasks {stats.pairs[0].tasks}, resamples {stats.resamples}, seed {stats.seed}"]
    lines.append("pair  delta_mrr10  ci95_low  ci95_high  wins  losses  ties  nonzero  p")
    lines.extend(
        f"{pair.pair:<5} {pair.delta_mrr10:+.4f}  {pair.ci_low:+.4f}  {pair.ci_high:+.4f}  "
        f"{pair.wins:>4}  {pair.losses:>6}  {pair.ties:>4}  {pair.wilcoxon.nonzero:>7}  "
        f"{pair.wilcoxon.p_value:.4f}"
        for pair in stats.pairs
    )
    return lines


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--report", type=Path, required=True)
    parser.add_argument("--resamples", type=int, required=True)
    parser.add_argument("--seed", type=int, required=True)
    parser.add_argument("--out", type=Path, required=True)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    stats = build_stats(load_report(args.report), args.resamples, args.seed)
    args.out.write_text(json.dumps(asdict(stats), indent=2) + "\n")
    print("\n".join(summary_lines(stats)))


if __name__ == "__main__":
    main()
