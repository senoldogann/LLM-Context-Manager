"""Ön-kayıtlı senaryoları bir sisteme karşı koşturan ve probları kaydeden yürütücü."""

from __future__ import annotations

import logging
import time
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

from freshness.classify import (
    CheckResult,
    classify_change,
    classify_preserve,
    judge_callers,
    judge_exists,
    judge_node,
)
from freshness.logs import log_event
from freshness.mcp import McpError
from freshness.model import (
    Adapter,
    AdapterError,
    ProbeOutcome,
    ProbeRecord,
    RunRecord,
    Session,
)
from freshness.scenarios import CallersCheck, Check, ExistsCheck, Phase, PhaseKind, Scenario

LOGGER = logging.getLogger("freshness.runner")

# Ön-kayıtlı prob zamanları: düzenlemenin son yazımı döndükten sonra geçen saniye.
T_GRID: tuple[float, ...] = (0.0, 0.25, 0.5, 1.0, 2.0, 5.0, 30.0)
SETTLE_S = 1.0
READY_POLL_S = 0.1
CLASSIFIERS: dict[PhaseKind, Callable[[tuple[CheckResult, ...]], ProbeOutcome]] = {
    PhaseKind.CHANGE: classify_change,
    PhaseKind.PRESERVE: classify_preserve,
}


class RunDirectoryError(RuntimeError):
    """Koşu dizini yarım kalmış önceki bir koşudan kalmış."""


@dataclass(frozen=True)
class RunPlan:
    """Tek bir senaryo × tekrar koşusu."""

    scenario: Scenario
    repetition: int
    run_dir: Path
    ready_timeout_s: float


def run_name(scenario: Scenario, repetition: int) -> str:
    """Koşunun log ve dizin adı."""
    return f"{scenario.scenario_id}-{scenario.repo_name}-r{repetition}"


def build_plans(
    scenarios: tuple[Scenario, ...], repetitions: int, work_dir: Path, ready_timeout_s: float
) -> tuple[RunPlan, ...]:
    """Tekrar-öncelikli sıra: her tekrar, tüm senaryoları ön-kayıttaki sırayla bir kez koşar."""
    return tuple(
        RunPlan(
            scenario=scenario,
            repetition=repetition,
            run_dir=work_dir / run_name(scenario, repetition),
            ready_timeout_s=ready_timeout_s,
        )
        for repetition in range(1, repetitions + 1)
        for scenario in scenarios
    )


def ask(check: Check, session: Session) -> CheckResult:
    """Soruyu açık oturuma sorar ve cevabı değerlendirir."""
    if isinstance(check, CallersCheck):
        return judge_callers(check.post, check.pre, session.callers(check.symbol))
    if isinstance(check, ExistsCheck):
        return judge_exists(check.post, check.pre, session.exists(check.symbol))
    return judge_node(check.post, check.pre, session.node_at(check.file, check.line))


def ask_all(checks: tuple[Check, ...], session: Session) -> tuple[CheckResult, ...]:
    """Bir probun bütün soruları, sırayla."""
    return tuple(ask(check, session) for check in checks)


def sleep_until(deadline: float) -> None:
    """Monotonik saat `deadline`a ulaşana kadar bekler; geçmişse hemen döner."""
    remaining = deadline - time.monotonic()
    if remaining > 0:
        time.sleep(remaining)


def wait_ready(session: Session, symbol: str, timeout_s: float, started: float) -> float | None:
    """Sembol görünene kadar yoklar; `started`tan beri geçen süreyi, olmazsa None döndürür."""
    deadline = started + timeout_s
    while time.monotonic() < deadline:
        if session.exists(symbol).found:
            return time.monotonic() - started
        if not session.alive():
            return None
        time.sleep(READY_POLL_S)
    return None


def probe_phase(
    session: Session, phase: Phase, edit_done_at: float, name: str
) -> tuple[ProbeRecord, ...]:
    """Düzenlemeden sonra ön-kayıtlı zamanlarda probları koşar ve sınıflandırır."""
    classify = CLASSIFIERS[phase.kind]
    records: list[ProbeRecord] = []
    for t_target in T_GRID:
        sleep_until(edit_done_at + t_target)
        sent = time.monotonic()
        outcome = classify(ask_all(phase.checks, session))
        record = ProbeRecord(
            phase=phase.name,
            t_target_s=t_target,
            t_actual_s=round(sent - edit_done_at, 4),
            duration_s=round(time.monotonic() - sent, 4),
            verdict=outcome.verdict,
            label_channel=outcome.label.channel,
            label_text=outcome.label.text,
            partial=outcome.partial,
            reflected_count=outcome.reflected_count,
            detail=outcome.detail,
        )
        log_event(
            LOGGER,
            logging.INFO,
            "probe",
            {
                "run": name,
                "phase": phase.name,
                "t_target_s": t_target,
                "t_actual_s": record.t_actual_s,
                "verdict": record.verdict.value,
                "label_channel": record.label_channel.value,
                "label_text": record.label_text,
            },
        )
        records.append(record)
    return tuple(records)


def failed_run(plan: RunPlan, index_seconds: float | None, error: str) -> RunRecord:
    """Kurulum sırasında sistem hatasıyla biten koşunun kaydı."""
    return RunRecord(
        scenario=plan.scenario.scenario_id,
        repo=plan.scenario.repo_name,
        repetition=plan.repetition,
        index_seconds=index_seconds,
        ready_seconds=None,
        baseline_ok=False,
        baseline_detail="",
        probes=(),
        crashed=False,
        error=error,
    )


def measure(adapter: Adapter, plan: RunPlan, repo: Path, index_seconds: float) -> RunRecord:
    """Oturumu açar; hazır olmayı ve ön-durumu kaydeder, düzenler ve problar."""
    scenario = plan.scenario
    name = run_name(scenario, plan.repetition)
    opened_at = time.monotonic()
    session = adapter.open_session(repo, scenario.auto_refresh, name)
    try:
        ready_seconds = wait_ready(session, scenario.ready_symbol, plan.ready_timeout_s, opened_at)
        baseline = ask_all(scenario.phases[0].checks, session)
        baseline_ok = all(result.pre_holds for result in baseline)
        baseline_detail = "; ".join(result.observed for result in baseline)
        log_event(
            LOGGER,
            logging.INFO,
            "baseline",
            {
                "run": name,
                "ready_seconds": ready_seconds,
                "baseline_ok": baseline_ok,
                "observed": baseline_detail,
            },
        )
        time.sleep(SETTLE_S)
        probes: list[ProbeRecord] = []
        for phase in scenario.phases:
            phase.edit(repo)
            if scenario.reindex_after_edit:
                adapter.reindex(repo)
            probes.extend(probe_phase(session, phase, time.monotonic(), name))
        return RunRecord(
            scenario=scenario.scenario_id,
            repo=scenario.repo_name,
            repetition=plan.repetition,
            index_seconds=index_seconds,
            ready_seconds=ready_seconds,
            baseline_ok=baseline_ok,
            baseline_detail=baseline_detail,
            probes=tuple(probes),
            crashed=not session.alive(),
            error="",
        )
    finally:
        session.close()


def execute_run(make_adapter: Callable[[Path], Adapter], plan: RunPlan) -> RunRecord:
    """Taze HOME ve çalışma kopyası kurar, indeksler ve ölçer.

    Sistemin kendi hataları (indeks, oturum) kayda geçer; harness hataları yükselir.
    """
    if plan.run_dir.exists():
        raise RunDirectoryError(f"{plan.run_dir} exists; remove the leftovers of an aborted run")
    home = plan.run_dir / "home"
    repo = plan.run_dir / "repo"
    home.mkdir(parents=True)
    plan.scenario.build(repo)
    adapter = make_adapter(home)
    try:
        index_seconds = adapter.build_index(repo)
    except AdapterError as error:
        return failed_run(plan, None, f"index: {error}")
    try:
        return measure(adapter, plan, repo, index_seconds)
    except (AdapterError, McpError) as error:
        return failed_run(plan, index_seconds, f"session: {type(error).__name__}: {error}")
