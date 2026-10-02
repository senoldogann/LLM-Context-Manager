"""Koşu kayıtlarından kol bazında özet ve A'ya karşı eşleştirilmiş karşılaştırma (Markdown)."""

from __future__ import annotations

import json
import math
import random
import statistics
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

from agent.prices import PRICES, estimated_cost
from freshness.jsonio import as_bool, as_float, as_int, as_list, as_object, as_str
from freshness.model import JsonValue

ARMS = ("A", "B", "C")
CCM_TOOL_PREFIX = "mcp__context-manager__"
BOOTSTRAP_ROUNDS = 10_000
BOOTSTRAP_SEED = 7
Z_95 = 1.96


class ReportError(ValueError):
    """Kayıt okunamadı ya da özetlenecek koşu yok."""


@dataclass(frozen=True)
class Outcome:
    """Raporun bir koşudan kullandığı ölçüler."""

    task_id: str
    category: str
    arm: str
    rep: int
    answered: bool
    success: bool
    recall: float
    precision: float
    stale: tuple[str, ...]
    edit_applied: bool | None
    missing: tuple[str, ...]
    extra: tuple[str, ...]
    failure: str | None
    cost_usd: float | None
    # Ajan maliyet bildirmediyse (Codex) `cost_usd` API fiyatıyla tahmindir.
    cost_estimated: bool
    input_tokens: int | None
    output_tokens: int | None
    turns: int | None
    wall_s: float | None
    index_s: float | None
    ccm_calls: int


Metric = Callable[[Outcome], float | None]


def optional_float(value: JsonValue, context: str) -> float | None:
    """Bildirilmeyen ölçü (Codex'te dolar maliyeti) `null`dır."""
    return None if value is None else as_float(value, context, ReportError)


def optional_int(value: JsonValue, context: str) -> int | None:
    """Bildirilmeyen ölçü (Codex'te tur sayısı) `null`dır."""
    return None if value is None else as_int(value, context, ReportError)


def strings(value: JsonValue, context: str) -> tuple[str, ...]:
    """JSON dizisini metin demetine daraltır."""
    return tuple(
        as_str(entry, context, ReportError) for entry in as_list(value, context, ReportError)
    )


def read_outcome(path: Path) -> Outcome:
    """Bir koşu kaydını okur; ürün/ajan hataları başarı paydasında kalır."""
    try:
        raw: JsonValue = json.loads(path.read_text())
    except json.JSONDecodeError as error:
        raise ReportError(f"{path} is not JSON") from error
    record = as_object(raw, str(path), ReportError)
    task_id = as_str(record.get("task_id"), f"{path}: task_id", ReportError)
    category = as_str(record.get("category"), f"{path}: category", ReportError)
    arm = as_str(record.get("arm"), f"{path}: arm", ReportError)
    rep = as_int(record.get("rep"), f"{path}: rep", ReportError)
    failure_value = record.get("failure")
    failure = (
        None if failure_value is None else as_str(failure_value, f"{path}: failure", ReportError)
    )
    wall_value = record.get("wall_s")
    wall_s = None if wall_value is None else as_float(wall_value, f"{path}: wall_s", ReportError)
    transcript_value = record.get("transcript")
    result_value = record.get("score")

    if transcript_value is None or result_value is None:
        if failure is None:
            raise ReportError(f"{path}: missing transcript/score without a failure reason")
        return Outcome(
            task_id=task_id,
            category=category,
            arm=arm,
            rep=rep,
            answered=False,
            success=False,
            recall=0.0,
            precision=0.0,
            stale=(),
            edit_applied=None,
            missing=(),
            extra=(),
            failure=failure,
            cost_usd=None,
            cost_estimated=False,
            input_tokens=None,
            output_tokens=None,
            turns=None,
            wall_s=wall_s,
            index_s=(
                None
                if record.get("index_s") is None
                else as_float(record.get("index_s"), f"{path}: index_s", ReportError)
            ),
            ccm_calls=0,
        )

    transcript = as_object(transcript_value, f"{path}: transcript", ReportError)
    usage = as_object(transcript.get("usage"), f"{path}: usage", ReportError)
    result = as_object(result_value, f"{path}: score", ReportError)
    calls = as_object(transcript.get("tool_calls"), f"{path}: tool_calls", ReportError)
    applied = result.get("edit_applied")
    input_fields = ("input_tokens", "cache_creation_input_tokens", "cache_read_input_tokens")
    score_success = as_bool(result.get("success"), f"{path}: success", ReportError)
    output_tokens = as_int(usage.get("output_tokens"), f"{path}: output_tokens", ReportError)
    reported_cost = optional_float(transcript.get("cost_usd"), f"{path}: cost_usd")
    estimate = (
        None
        if reported_cost is not None
        else estimated_cost(
            as_str(record.get("model"), f"{path}: model", ReportError),
            as_int(usage.get("input_tokens"), f"{path}: input_tokens", ReportError),
            as_int(usage.get("cache_read_input_tokens"), f"{path}: cache_read", ReportError),
            output_tokens,
        )
    )
    return Outcome(
        task_id=task_id,
        category=category,
        arm=arm,
        rep=rep,
        answered=as_bool(result.get("answered"), f"{path}: answered", ReportError),
        success=score_success and failure is None,
        recall=as_float(result.get("recall"), f"{path}: recall", ReportError),
        precision=as_float(result.get("precision"), f"{path}: precision", ReportError),
        stale=strings(result.get("stale"), f"{path}: stale"),
        edit_applied=None if applied is None else as_bool(applied, f"{path}: edit", ReportError),
        missing=strings(result.get("missing"), f"{path}: missing"),
        extra=strings(result.get("extra"), f"{path}: extra"),
        failure=failure,
        cost_usd=reported_cost if reported_cost is not None else estimate,
        cost_estimated=estimate is not None,
        input_tokens=sum(
            as_int(usage.get(field), f"{path}: {field}", ReportError) for field in input_fields
        ),
        output_tokens=output_tokens,
        turns=optional_int(transcript.get("num_turns"), f"{path}: num_turns"),
        wall_s=wall_s,
        index_s=(
            None
            if record.get("index_s") is None
            else as_float(record.get("index_s"), f"{path}: index_s", ReportError)
        ),
        ccm_calls=sum(
            as_int(count, f"{path}: tool_calls.{name}", ReportError)
            for name, count in calls.items()
            if name.startswith(CCM_TOOL_PREFIX)
        ),
    )


def read_outcomes(out_dir: Path) -> list[Outcome]:
    """Sonuç dizinindeki tüm tamamlanmış koşular."""
    runs = out_dir / "runs"
    return [read_outcome(path) for path in sorted(runs.glob("*.json"))] if runs.is_dir() else []


def wilson(successes: int, total: int) -> tuple[float, float]:
    """Başarı oranı için Wilson %95 aralığı."""
    rate = successes / total
    denominator = 1 + Z_95**2 / total
    centre = (rate + Z_95**2 / (2 * total)) / denominator
    half = Z_95 * math.sqrt(rate * (1 - rate) / total + Z_95**2 / (4 * total**2)) / denominator
    return (centre - half, centre + half)


def bootstrap_median(values: list[float]) -> tuple[float, float]:
    """Medyanın, görevler yeniden örneklenerek bulunan %95 aralığı (sabit tohum)."""
    generator = random.Random(BOOTSTRAP_SEED)
    medians = sorted(
        statistics.median(generator.choices(values, k=len(values))) for _ in range(BOOTSTRAP_ROUNDS)
    )
    return (medians[int(0.025 * BOOTSTRAP_ROUNDS)], medians[int(0.975 * BOOTSTRAP_ROUNDS) - 1])


def task_means(outcomes: list[Outcome], arm: str, metric: Metric) -> dict[str, float]:
    """Kolda her görevin tekrar ortalaması; bilinmeyen metriği olan görev dışarıda kalır."""
    by_task: dict[str, list[float | None]] = {}
    for outcome in outcomes:
        if outcome.arm == arm:
            by_task.setdefault(outcome.task_id, []).append(metric(outcome))
    means: dict[str, float] = {}
    for task, values in by_task.items():
        known = [value for value in values if value is not None]
        if len(known) == len(values):
            means[task] = statistics.fmean(known)
    return means


def paired_ratios(outcomes: list[Outcome], arm: str, metric: Metric) -> list[float]:
    """Görev başına kol / A oranları; iki kolda da eksiksiz metriği olan görevler."""
    base = task_means(outcomes, "A", metric)
    other = task_means(outcomes, arm, metric)
    return [other[task] / base[task] for task in sorted(base) if task in other and base[task] > 0]


def as_metric(value: int | float | None) -> float | None:
    """İsteğe bağlı sayıyı oran metriğine çevirir."""
    return None if value is None else float(value)


RATIO_METRICS: tuple[tuple[str, Metric], ...] = (
    ("Cost", lambda outcome: outcome.cost_usd),
    ("Input tokens (incl. cache)", lambda outcome: as_metric(outcome.input_tokens)),
    ("Output tokens", lambda outcome: as_metric(outcome.output_tokens)),
    ("Turns", lambda outcome: as_metric(outcome.turns)),
    ("Wall time", lambda outcome: outcome.wall_s),
)


def known_numbers(values: list[int | float | None]) -> list[float]:
    """Bilinmeyen ölçüleri atıp sayıları float olarak döner."""
    return [float(value) for value in values if value is not None]


def median_cell(
    values: list[int | float | None],
    spec: str,
    prefix: str = "",
    suffix: str = "",
) -> str:
    """Bilinen ölçülerin medyanını yazar; hiç ölçü yoksa em dash döner."""
    known = known_numbers(values)
    if not known:
        return "—"
    return f"{prefix}{format(statistics.median(known), spec)}{suffix}"


def arm_rows(outcomes: list[Outcome]) -> list[str]:
    """Kol bazında başarı, cevap kalitesi ve maliyet."""
    rows = [
        "## By arm",
        "",
        "| Arm | Success | 95% CI | Recall | Precision | Median cost | Total known cost "
        "| Median input tokens | Median output tokens | Median turns | Median agent wall "
        "| Median pre-index | Runs using CCM | Stale (edit runs) | Run failures |",
        "|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|",
    ]
    for arm in ARMS:
        runs = [outcome for outcome in outcomes if outcome.arm == arm]
        if not runs:
            continue
        wins = sum(outcome.success for outcome in runs)
        low, high = wilson(wins, len(runs))
        edits = [outcome for outcome in runs if outcome.category == "edit"]
        stale = sum(bool(outcome.stale) for outcome in edits)
        used = sum(outcome.ccm_calls > 0 for outcome in runs)
        failures = sum(outcome.failure is not None for outcome in runs)
        costs = known_numbers([outcome.cost_usd for outcome in runs])
        unknown_costs = len(runs) - len(costs)
        total_cost = f"— ({unknown_costs} unknown)" if not costs else f"${sum(costs):.2f}"
        if unknown_costs and costs:
            total_cost += f" + {unknown_costs} unknown"
        rows.append(
            f"| {arm} | {wins}/{len(runs)} ({wins / len(runs):.0%}) | {low:.0%}–{high:.0%} "
            f"| {statistics.fmean(o.recall for o in runs):.2f} "
            f"| {statistics.fmean(o.precision for o in runs):.2f} "
            f"| {median_cell([o.cost_usd for o in runs], '.3f', prefix='$')} "
            f"| {total_cost} "
            f"| {median_cell([o.input_tokens for o in runs], ',.0f')} "
            f"| {median_cell([o.output_tokens for o in runs], ',.0f')} "
            f"| {median_cell([o.turns for o in runs], '.0f')} "
            f"| {median_cell([o.wall_s for o in runs], '.0f', suffix=' s')} "
            f"| {median_cell([o.index_s for o in runs], '.1f', suffix=' s')} "
            f"| {used}/{len(runs)} | {stale}/{len(edits)} | {failures}/{len(runs)} |"
        )
    return rows


def ratio_rows(outcomes: list[Outcome]) -> list[str]:
    """B ve C'nin A'ya göre görev başına oranlarının medyanı ve bootstrap aralığı."""
    rows = [
        "## Paired against A",
        "",
        "Per task, the mean over repetitions in each arm, then the ratio to arm A; the table "
        "gives the median of those ratios and its 95% bootstrap interval over tasks. Below 1 "
        "means less than A. A task is omitted from a metric ratio if any recorded repetition "
        "in either compared arm has that metric unknown.",
        "",
        "| Metric | B / A | C / A |",
        "|---|---|---|",
    ]
    for name, metric in RATIO_METRICS:
        cells: list[str] = []
        for arm in ("B", "C"):
            ratios = paired_ratios(outcomes, arm, metric)
            if not ratios:
                cells.append("—")
                continue
            low, high = bootstrap_median(ratios)
            cells.append(
                f"{statistics.median(ratios):.2f} ({low:.2f}–{high:.2f}, {len(ratios)} tasks)"
            )
        rows.append(f"| {name} | {cells[0]} | {cells[1]} |")
    return rows


def category_rows(outcomes: list[Outcome]) -> list[str]:
    """Görev türüne göre başarılar."""
    rows = [
        "## Success by task category",
        "",
        "| Category | " + " | ".join(ARMS) + " |",
        "|---|" + "---|" * len(ARMS),
    ]
    for category in sorted({outcome.category for outcome in outcomes}):
        cells: list[str] = []
        for arm in ARMS:
            runs = [o for o in outcomes if o.category == category and o.arm == arm]
            cells.append(f"{sum(o.success for o in runs)}/{len(runs)}" if runs else "—")
        rows.append(f"| {category} | " + " | ".join(cells) + " |")
    return rows


def task_rows(outcomes: list[Outcome]) -> list[str]:
    """Görev başına başarılar ve ortalama maliyet."""
    rows = [
        "## By task",
        "",
        "| Task | " + " | ".join(ARMS) + " | " + " | ".join(f"Cost {arm}" for arm in ARMS) + " |",
        "|---|" + "---|" * (2 * len(ARMS)),
    ]
    for task in sorted({outcome.task_id for outcome in outcomes}):
        wins: list[str] = []
        costs: list[str] = []
        for arm in ARMS:
            runs = [o for o in outcomes if o.task_id == task and o.arm == arm]
            wins.append(f"{sum(o.success for o in runs)}/{len(runs)}" if runs else "—")
            known_costs = known_numbers([outcome.cost_usd for outcome in runs])
            if not known_costs:
                costs.append("—")
            elif len(known_costs) == len(runs):
                costs.append(f"${statistics.fmean(known_costs):.3f}")
            else:
                costs.append(
                    f"${statistics.fmean(known_costs):.3f} ({len(known_costs)}/{len(runs)} known)"
                )
        rows.append(f"| `{task}` | " + " | ".join(wins) + " | " + " | ".join(costs) + " |")
    return rows


def failure_rows(outcomes: list[Outcome]) -> list[str]:
    """Başarısız koşuların listesi; nedenleri başarısızlık ledger'ında dökümden okunur."""
    rows = ["## Failed runs", ""]
    failed = sorted(
        (outcome for outcome in outcomes if not outcome.success),
        key=lambda outcome: (outcome.task_id, outcome.arm, outcome.rep),
    )
    for outcome in failed:
        reasons: list[str] = []
        if outcome.failure is not None:
            reasons.append(outcome.failure)
        if not outcome.answered and outcome.failure is None:
            reasons.append("no answer block")
        if outcome.edit_applied is False:
            reasons.append("edit not applied")
        if outcome.missing:
            reasons.append("missing " + ", ".join(outcome.missing))
        if outcome.extra:
            reasons.append("extra " + ", ".join(outcome.extra))
        if outcome.stale:
            reasons.append("stale: " + ", ".join(outcome.stale))
        rows.append(f"- `{outcome.task_id}` {outcome.arm}#{outcome.rep}: " + "; ".join(reasons))
    return rows if failed else [*rows, "None."]


def header(setup: JsonValue, outcomes: list[Outcome]) -> list[str]:
    """Raporun başlığı: yöntem kapsamı, model, ayarlar ve sürümler."""
    data = as_object(setup, "settings.json", ReportError)
    settings = as_object(data.get("settings"), "settings.json: settings", ReportError)
    environment = as_object(data.get("environment"), "settings.json: environment", ReportError)
    commit = as_str(environment.get("ccm_commit"), "settings.json: ccm_commit", ReportError)
    dirty = as_bool(environment.get("ccm_dirty"), "settings.json: ccm_dirty", ReportError)
    tasks = as_list(settings.get("tasks"), "settings.json: tasks", ReportError)
    repetitions = as_int(settings.get("repetitions"), "settings.json: repetitions", ReportError)
    planned = len(tasks) * len(ARMS) * repetitions
    design = (
        "preregistered final design" if len(tasks) == 24 and repetitions == 3 else "pilot design"
    )
    progress = (
        f"complete ({len(outcomes)}/{planned} runs)"
        if len(outcomes) == planned
        else f"partial ({len(outcomes)}/{planned} runs)"
    )
    return [
        "# L3 agent benchmark",
        "",
        f"Scope: {design}; {progress}.",
        "",
        f"Agent `{settings.get('agent')}` ({environment.get('agent_version')}, auth "
        f"`{settings.get('auth')}`), model `{settings.get('model')}` at effort "
        f"`{settings.get('effort')}`, embeddings `{settings.get('embedding')}`, per-run budget "
        f"${settings.get('max_budget_usd')}; {environment.get('ccm_version')} at "
        f"`{commit[:7]}`{' (uncommitted changes)' if dirty else ''}.",
        "",
        cost_note(as_str(settings.get("model"), "settings.json: model", ReportError), outcomes),
    ]


def cost_note(model: str, outcomes: list[Outcome]) -> str:
    """Maliyet sütunlarının neyi gösterdiği: ajanın bildirdiği tutar ya da API fiyatıyla tahmin."""
    if not any(outcome.cost_estimated for outcome in outcomes):
        return "Cost is the amount the agent reported."
    price = PRICES[model]
    return (
        f"Cost is an API-price estimate (the agent reports none): `{model}` at ${price.input:.2f} "
        f"input, ${price.cached_input:.2f} cached input and ${price.output:.2f} output per 1M "
        f"tokens, standard tier, from {price.source} ({price.retrieved}). On a subscription it "
        "shows what the same tokens would cost through the API; tokens are the quota measure."
    )


def render(outcomes: list[Outcome], setup: JsonValue) -> str:
    """Raporun tamamı."""
    if not outcomes:
        raise ReportError("there are no completed runs to report")
    sections = (
        header(setup, outcomes),
        arm_rows(outcomes),
        ratio_rows(outcomes),
        category_rows(outcomes),
        task_rows(outcomes),
        failure_rows(outcomes),
    )
    return "\n\n".join("\n".join(section) for section in sections) + "\n"
