"""Görev alt kümeleri için kol/A oranları, başarı ve sınır analizi (kayıtlara dokunmaz).

Kullanım (`benchmarks/` içinde):
    PYTHONPATH=. uv run python scripts/subset_report.py <sonuç dizini> <kol> <başlık>=<görevler>...

`<görevler>` virgülle ayrılmış görev kimlikleridir. Her alt küme için bir Markdown tablosu
yazılır: görev başına A ve kol ortalama giriş token'ı (önbellek dahil), oran, başarı, CCM
çağrısı ve pilotlardaki görev kuralı (kolun ortalaması A'nınkinden düşük ve kolun her koşusu A
ortalamasının altında). Altında medyan oran ve sabit tohumlu bootstrap %95 aralığı, iki kolun
başarısı (Wilson %95) ile kategori ve depo kırılımı yer alır.
"""

import statistics
import sys
from dataclasses import dataclass
from pathlib import Path

from agent.report import Outcome, as_metric, bootstrap_median, read_outcomes, wilson


@dataclass(frozen=True)
class TaskRow:
    """Bir görevin iki koldaki özeti."""

    task_id: str
    repo: str
    category: str
    base_mean: float
    arm_mean: float
    base_success: int
    arm_success: int
    runs: int
    arm_ccm_runs: int
    pilot_rule: bool


class SubsetError(ValueError):
    """Alt küme raporlanamıyor (eksik görev ya da eksik token sayısı)."""


def tokens(outcome: Outcome) -> float:
    """Koşunun giriş token'ı; bilinmiyorsa açık hata."""
    value = as_metric(outcome.input_tokens)
    if value is None:
        raise SubsetError(f"{outcome.task_id} {outcome.arm}#{outcome.rep}: no input tokens")
    return value


def task_row(outcomes: list[Outcome], task_id: str, arm: str) -> TaskRow:
    """Görevin A ve kol koşularından satır."""
    base = [outcome for outcome in outcomes if outcome.task_id == task_id and outcome.arm == "A"]
    other = [outcome for outcome in outcomes if outcome.task_id == task_id and outcome.arm == arm]
    if not base or len(base) != len(other):
        raise SubsetError(f"{task_id}: {len(base)} A runs and {len(other)} {arm} runs")
    base_mean = statistics.fmean(tokens(outcome) for outcome in base)
    arm_tokens = [tokens(outcome) for outcome in other]
    arm_mean = statistics.fmean(arm_tokens)
    return TaskRow(
        task_id=task_id,
        repo=base[0].repo,
        category=base[0].category,
        base_mean=base_mean,
        arm_mean=arm_mean,
        base_success=sum(outcome.success for outcome in base),
        arm_success=sum(outcome.success for outcome in other),
        runs=len(base),
        arm_ccm_runs=sum(outcome.ccm_calls > 0 for outcome in other),
        pilot_rule=arm_mean < base_mean and all(value < base_mean for value in arm_tokens),
    )


def median_with_interval(rows: list[TaskRow]) -> str:
    """Oranların medyanı ve bootstrap aralığı."""
    ratios = [row.arm_mean / row.base_mean for row in rows]
    low, high = bootstrap_median(ratios) if len(ratios) > 1 else (ratios[0], ratios[0])
    return f"{statistics.median(ratios):.2f} ({low:.2f}–{high:.2f}, {len(ratios)} tasks)"


def success_cell(successes: int, total: int) -> str:
    """Başarı sayısı ve Wilson aralığı."""
    low, high = wilson(successes, total)
    return f"{successes}/{total} ({low:.0%}–{high:.0%})"


def subset_table(title: str, rows: list[TaskRow], arm: str) -> list[str]:
    """Alt kümenin Markdown tablosu ve özeti."""
    lines = [
        f"### {title}",
        "",
        f"| Task | A tokens | {arm} tokens | {arm}/A | A ok | {arm} ok | {arm} runs calling CCM "
        "| Pilot rule |",
        "|---|---:|---:|---:|---:|---:|---:|---|",
    ]
    lines.extend(
        f"| `{row.task_id}` | {row.base_mean:,.0f} | {row.arm_mean:,.0f} | "
        f"{row.arm_mean / row.base_mean:.2f} | {row.base_success}/{row.runs} | "
        f"{row.arm_success}/{row.runs} | {row.arm_ccm_runs}/{row.runs} | "
        f"{'saving' if row.pilot_rule else '–'} |"
        for row in rows
    )
    total = sum(row.runs for row in rows)
    lines += [
        "",
        f"- {arm}/A median: {median_with_interval(rows)}",
        f"- Success: A {success_cell(sum(row.base_success for row in rows), total)}, "
        f"{arm} {success_cell(sum(row.arm_success for row in rows), total)}",
        f"- Pilot rule met on {sum(row.pilot_rule for row in rows)} of {len(rows)} tasks",
    ]
    for label, key in (("category", "category"), ("repository", "repo")):
        groups = sorted({getattr(row, key) for row in rows})
        cells = [
            f"{group} {median_with_interval([row for row in rows if getattr(row, key) == group])}"
            for group in groups
        ]
        lines.append(f"- By {label}: " + "; ".join(cells))
    return [*lines, ""]


def main() -> None:
    """Komut satırındaki her alt küme için tablo yazar."""
    out_dir, arm = Path(sys.argv[1]), sys.argv[2]
    outcomes = read_outcomes(out_dir)
    for spec in sys.argv[3:]:
        title, _, task_list = spec.partition("=")
        rows = [task_row(outcomes, task_id, arm) for task_id in task_list.split(",")]
        print("\n".join(subset_table(title, rows, arm)))


if __name__ == "__main__":
    main()
