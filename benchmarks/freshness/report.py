"""Ham sonuç dosyasından Markdown özet: ön-kayıtlı metrikler; saf fonksiyonlar."""

from __future__ import annotations

import math
import statistics
from collections import Counter
from dataclasses import dataclass
from itertools import pairwise

from freshness.model import LabelChannel, ProbeRecord, RunRecord, Verdict
from freshness.results import ResultsFile

ABBREVIATIONS: dict[Verdict, str] = {
    Verdict.CORRECT: "C",
    Verdict.STALE_SILENT: "SS",
    Verdict.STALE_LABELED: "SL",
    Verdict.STALE_STRUCTURED_ONLY: "SSO",
    Verdict.ERROR_EMPTY: "E",
    Verdict.PRESERVED_LABELED: "PL",
    Verdict.PRESERVED_SILENT: "PS",
    Verdict.LOST: "L",
}
# H1'de sessiz sayılan sınıflar: yalnız structuredContent'teki işaret de sessizdir.
SILENT: frozenset[Verdict] = frozenset({Verdict.STALE_SILENT, Verdict.STALE_STRUCTURED_ONLY})
STALE: frozenset[Verdict] = SILENT | {Verdict.STALE_LABELED}
CHANGE_VERDICTS: frozenset[Verdict] = STALE | {Verdict.CORRECT, Verdict.ERROR_EMPTY}
LEGEND = (
    "Legend: C correct · SS stale, silent · SL stale, labeled in content text · "
    "SSO stale, label only in structuredContent · E error, empty or partial update · "
    "PL / PS last good state preserved, labeled / silent · L last good state lost."
)


@dataclass(frozen=True)
class RowKey:
    """Özet satırı: senaryo × repo × aşama."""

    scenario: str
    repo: str
    phase: str


def row_keys(runs: tuple[RunRecord, ...]) -> tuple[RowKey, ...]:
    """Satırlar, koşularda ilk görüldükleri sırayla."""
    keys: dict[RowKey, None] = {}
    for run in runs:
        for probe in run.probes:
            keys.setdefault(RowKey(scenario=run.scenario, repo=run.repo, phase=probe.phase), None)
    return tuple(keys)


def runs_for(runs: tuple[RunRecord, ...], key: RowKey) -> tuple[RunRecord, ...]:
    """Satırın senaryo ve reposuna ait koşular (kurulumda düşenler dahil)."""
    return tuple(run for run in runs if run.scenario == key.scenario and run.repo == key.repo)


def phase_probes(run: RunRecord, phase: str) -> tuple[ProbeRecord, ...]:
    """Bir koşunun tek aşamasındaki problar, zaman sırasıyla."""
    return tuple(probe for probe in run.probes if probe.phase == phase)


def count_of(probes: tuple[ProbeRecord, ...], verdicts: frozenset[Verdict]) -> int:
    """Sınıfı `verdicts` içinde olan probların sayısı."""
    return sum(1 for probe in probes if probe.verdict in verdicts)


def known(values: tuple[float | None, ...]) -> tuple[float, ...]:
    """Ölçülmüş (None olmayan) değerler."""
    return tuple(value for value in values if value is not None)


def count_cell(probes: tuple[ProbeRecord, ...]) -> str:
    """Sınıf sayıları, ör. `C 2, SS 1`; kısmi güncellemeler ayrıca sayılır."""
    counts = Counter(probe.verdict for probe in probes)
    parts = [
        f"{ABBREVIATIONS[verdict]} {counts[verdict]}" for verdict in Verdict if counts[verdict]
    ]
    partial = sum(1 for probe in probes if probe.partial)
    if partial:
        parts.append(f"partial {partial}")
    return ", ".join(parts) if parts else "–"


def time_to_correct(probes: tuple[ProbeRecord, ...]) -> float:
    """Kendisinden sonraki her probun CORRECT olduğu ilk prob zamanı; yoksa sonsuz."""
    first = math.inf
    for probe in reversed(probes):
        if probe.verdict is not Verdict.CORRECT:
            break
        first = probe.t_target_s
    return first


def seconds_text(value: float) -> str:
    """Saniye; sonsuz 'never' olarak yazılır."""
    return "never" if math.isinf(value) else f"{value:.3g} s"


def share(part: int, whole: int) -> str:
    """`part/whole (yüzde)`."""
    return "n/a" if whole == 0 else f"{part}/{whole} ({100 * part / whole:.0f}%)"


def median_range(values: tuple[float, ...]) -> str:
    """Medyan (min–maks)."""
    if not values:
        return "–"
    ordered = sorted(values)
    middle = seconds_text(statistics.median(ordered))
    return f"{middle} ({seconds_text(ordered[0])}–{seconds_text(ordered[-1])})"


def header(results: ResultsFile) -> list[str]:
    """Sistem, ikililer, ortam ve ayarlar."""
    system = results.system
    environment = results.environment
    settings = results.settings
    artifacts = ", ".join(f"`{item.name}` sha256 `{item.sha256}`" for item in system.artifacts)
    grid = ", ".join(f"{value:g}" for value in settings.t_grid_s)
    state = "complete" if results.complete else "INCOMPLETE (in progress or aborted)"
    memory = environment.memory_bytes / 2**30
    return [
        f"# L1 freshness: {system.name} {system.version}",
        "",
        f"- Pre-registration: `{results.preregistration}`, harness commit "
        f"`{results.harness_revision}`",
        f"- Artifacts: {artifacts}",
        f"- Environment: {environment.platform}, {environment.cpu}, "
        f"{environment.logical_cpus} logical CPUs, {memory:.0f} GiB RAM, "
        f"Python {environment.python}",
        f"- Settings: probes at t = {grid} s after the edit; settle {settings.settle_s:g} s; "
        f"ready poll {settings.ready_poll_s:g} s, timeout {settings.ready_timeout_s:g} s; "
        f"request timeout {settings.request_timeout_s:g} s; "
        f"{settings.repetitions} repetitions",
        f"- Runs: {len(results.runs)}, {results.started_at} to "
        f"{results.finished_at or '…'}, {state}",
        "",
        LEGEND,
    ]


def hypothesis(results: ResultsFile) -> list[str]:
    """H1 girdisi: S1'in sessiz bayat oranı. Tek sistemle H1 ölçülemez."""
    probes = tuple(
        probe for run in results.runs if run.scenario.startswith("S1-") for probe in run.probes
    )
    rate = share(count_of(probes, SILENT), len(probes))
    return [
        "## H1",
        "",
        "Not measurable from this file: only one system was run "
        "(competitor runs need approval gate G1).",
        "",
        f"Input for H1, {results.system.name} silent-stale rate on S1 "
        f"(all edits, repositories and probe times): {rate}.",
    ]


def summary_table(results: ResultsFile) -> list[str]:
    """Senaryo başına oranlar ve doğru cevaba kadar geçen süre."""
    lines = [
        "## Per scenario",
        "",
        "| Scenario | Repo | Phase | Runs | Baseline met | Silent stale | Labeled stale "
        "| Error/empty | Time to correct: median (min–max) |",
        "|---|---|---|---|---|---|---|---|---|",
    ]
    for key in row_keys(results.runs):
        runs = runs_for(results.runs, key)
        groups = tuple(group for group in (phase_probes(run, key.phase) for run in runs) if group)
        probes = tuple(
            probe for group in groups for probe in group if probe.verdict in CHANGE_VERDICTS
        )
        baseline = f"{sum(1 for run in runs if run.baseline_ok)}/{len(runs)}"
        if probes:
            cells = (
                share(count_of(probes, SILENT), len(probes)),
                share(count_of(probes, frozenset({Verdict.STALE_LABELED})), len(probes)),
                share(count_of(probes, frozenset({Verdict.ERROR_EMPTY})), len(probes)),
                median_range(tuple(time_to_correct(group) for group in groups)),
            )
        else:
            cells = ("–", "–", "–", "– (preservation phase: see class counts)")
        lines.append(
            f"| {key.scenario} | {key.repo} | {key.phase} | {len(runs)} | {baseline} | "
            + " | ".join(cells)
            + " |"
        )
    return lines


def class_table(results: ResultsFile) -> list[str]:
    """Her prob zamanında sınıf sayıları, tüm tekrarlar toplamı."""
    grid = results.settings.t_grid_s
    lines = [
        "## Class counts by probe time (all repetitions)",
        "",
        "| Scenario | Repo | Phase | " + " | ".join(f"{value:g} s" for value in grid) + " |",
        "|---|---|---|" + "---|" * len(grid),
    ]
    for key in row_keys(results.runs):
        probes = tuple(
            probe for run in runs_for(results.runs, key) for probe in phase_probes(run, key.phase)
        )
        cells = " | ".join(
            count_cell(tuple(probe for probe in probes if probe.t_target_s == value))
            for value in grid
        )
        lines.append(f"| {key.scenario} | {key.repo} | {key.phase} | {cells} |")
    return lines


def s1_by_time(results: ResultsFile) -> list[str]:
    """S1 (gerçek repolar) için prob zamanına göre sınıf payları."""
    lines = [
        "## S1 by probe time (real repositories)",
        "",
        "| t | Repo | Probes | Correct | Silent stale | Labeled stale | Error/empty |",
        "|---|---|---|---|---|---|---|",
    ]
    s1_runs = tuple(run for run in results.runs if run.scenario.startswith("S1-"))
    for value in results.settings.t_grid_s:
        for repo in dict.fromkeys(run.repo for run in s1_runs):
            probes = tuple(
                probe
                for run in s1_runs
                if run.repo == repo
                for probe in run.probes
                if probe.t_target_s == value
            )
            total = len(probes)
            lines.append(
                f"| {value:g} s | {repo} | {total} | "
                f"{share(count_of(probes, frozenset({Verdict.CORRECT})), total)} | "
                f"{share(count_of(probes, SILENT), total)} | "
                f"{share(count_of(probes, frozenset({Verdict.STALE_LABELED})), total)} | "
                f"{share(count_of(probes, frozenset({Verdict.ERROR_EMPTY})), total)} |"
            )
    return lines


def reflected(results: ResultsFile) -> list[str]:
    """S6: her probda yansıyan çağıran sayısı ve hiç azalıp azalmadığı."""
    lines = ["## S6: callers reflected per probe", ""]
    for run in results.runs:
        if run.scenario != "S6":
            continue
        counts = tuple(probe.reflected_count for probe in run.probes)
        measured = [count for count in counts if count is not None]
        decreased = any(later < earlier for earlier, later in pairwise(measured))
        sequence = " → ".join("–" if count is None else str(count) for count in counts)
        lines.append(
            f"- repetition {run.repetition}: {sequence}; "
            f"ever decreased: {'yes' if decreased else 'no'}"
        )
    if len(lines) == 2:
        lines.append("No S6 runs in this file.")
    return lines


def labels(results: ResultsFile) -> list[str]:
    """S9: bayatlık işaretinin hangi kanalda göründüğü ve görülen durum satırları."""
    probes = tuple(probe for run in results.runs for probe in run.probes)
    stale = tuple(probe for probe in probes if probe.verdict in STALE)
    lines = [
        "## S9: where staleness labels appear",
        "",
        "| Channel | All probes | Stale probes |",
        "|---|---|---|",
    ]
    for channel in LabelChannel:
        in_all = sum(1 for probe in probes if probe.label_channel is channel)
        in_stale = sum(1 for probe in stale if probe.label_channel is channel)
        lines.append(
            f"| {channel.value} | {share(in_all, len(probes))} | {share(in_stale, len(stale))} |"
        )
    texts = Counter(probe.label_text for probe in probes)
    lines.extend(["", "Status lines seen, most frequent first:", ""])
    lines.extend(f"- `{text or '(none)'}`: {count}" for text, count in texts.most_common(10))
    return lines


def timing(results: ResultsFile) -> list[str]:
    """Betimleyici süreler: indeks, hazır olma ve sorgu süresi (ön-kayıtlı metrik değil)."""
    lines = [
        "## Index, ready and query time (descriptive, not a pre-registered metric)",
        "",
        "| Repo | Runs | Index build: median (min–max) | Ready after session start "
        "| One probe (all its tool calls) |",
        "|---|---|---|---|---|",
    ]
    for repo in dict.fromkeys(run.repo for run in results.runs):
        runs = tuple(run for run in results.runs if run.repo == repo)
        index = known(tuple(run.index_seconds for run in runs))
        ready = known(tuple(run.ready_seconds for run in runs))
        durations = tuple(probe.duration_s for run in runs for probe in run.probes)
        lines.append(
            f"| {repo} | {len(runs)} | {median_range(index)} | {median_range(ready)} | "
            f"{median_range(durations)} |"
        )
    return lines


def problems(results: ResultsFile) -> list[str]:
    """Kurulumda hata, hazır olmama, ön-durumu sağlamama ya da süreç çıkışı olan koşular."""
    lines = ["## Runs with setup problems", ""]
    for run in results.runs:
        issues: list[str] = []
        if run.error:
            issues.append(f"error: {run.error}")
        if run.ready_seconds is None:
            issues.append("never ready")
        if not run.baseline_ok:
            issues.append(f"baseline not met ({run.baseline_detail or 'no answer'})")
        if run.crashed:
            issues.append("server process exited")
        if issues:
            lines.append(f"- {run.scenario} / {run.repo} / r{run.repetition}: " + "; ".join(issues))
    if len(lines) == 2:
        lines.append("None.")
    return lines


def render_report(results: ResultsFile) -> str:
    """Tüm bölümleri tek Markdown belgesinde birleştirir."""
    sections = (
        header(results),
        hypothesis(results),
        summary_table(results),
        class_table(results),
        s1_by_time(results),
        reflected(results),
        labels(results),
        timing(results),
        problems(results),
    )
    return "\n\n".join("\n".join(section) for section in sections) + "\n"
