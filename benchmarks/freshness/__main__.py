"""`python -m freshness run|report`: L1 tazelik ölçümünün komut satırı girişi."""

from __future__ import annotations

import argparse
import dataclasses
import logging
import os
import platform
import subprocess
import sys
from collections.abc import Callable
from datetime import UTC, datetime
from pathlib import Path

from freshness import repos
from freshness.ccm import CcmAdapter, CcmConfig
from freshness.logs import JsonLineFormatter, log_event
from freshness.model import Adapter, RunRecord
from freshness.report import render_report
from freshness.results import (
    SCHEMA_VERSION,
    EnvironmentInfo,
    ResultsFile,
    Settings,
    read_results,
    write_results,
)
from freshness.runner import (
    READY_POLL_S,
    SETTLE_S,
    T_GRID,
    build_plans,
    execute_run,
    run_name,
)
from freshness.scenarios import all_scenarios, select_scenarios

LOGGER = logging.getLogger("freshness")
PACKAGE_DIR = Path(__file__).resolve().parent
PREREGISTRATION = "benchmarks/freshness/PREREGISTRATION.md"
# Ölçümü belirleyen dosyalar: bunlarda commit'lenmemiş değişiklik varken ölçüm koşulmaz.
HARNESS_PATHS = ("benchmarks/freshness", "benchmarks/pyproject.toml", "benchmarks/uv.lock")
COMMAND_ATTEMPTS = 2


class HarnessStateError(RuntimeError):
    """Ölçüm ön koşulu sağlanmadı."""


@dataclasses.dataclass(frozen=True)
class RunOptions:
    """`run` komutunun argümanları; hepsi zorunlu."""

    bin_dir: Path
    corpus_dir: Path
    work_dir: Path
    log_dir: Path
    out: Path
    repetitions: int
    scenarios: str
    request_timeout_s: float
    ready_timeout_s: float
    command_timeout_s: float


def now_iso() -> str:
    """UTC zaman damgası."""
    return datetime.now(UTC).isoformat(timespec="seconds")


def harness_revision() -> str:
    """Harness commit'i; ölçüm yöntemi commit'lenmemişse koşmayı reddeder."""
    root = Path(repos.run_git(PACKAGE_DIR, ("rev-parse", "--show-toplevel")).strip())
    dirty = repos.run_git(
        root, ("status", "--porcelain", "--untracked-files=all", "--", *HARNESS_PATHS)
    ).strip()
    if dirty:
        raise HarnessStateError(
            "uncommitted harness changes; commit the methodology before a measured run:\n" + dirty
        )
    return repos.run_git(root, ("rev-parse", "HEAD")).strip()


def sysctl(name: str) -> str:
    """macOS `sysctl -n` değeri."""
    completed = subprocess.run(
        ["/usr/sbin/sysctl", "-n", name], capture_output=True, text=True, check=False
    )
    if completed.returncode != 0:
        raise HarnessStateError(
            f"sysctl -n {name} exited {completed.returncode}: {completed.stderr.strip()}"
        )
    return completed.stdout.strip()


def environment_info() -> EnvironmentInfo:
    """Ölçüm makinesinin kimliği (yol ya da kullanıcı adı içermez)."""
    cpus = os.cpu_count()
    if cpus is None:
        raise HarnessStateError("os.cpu_count() returned None")
    return EnvironmentInfo(
        platform=platform.platform(),
        machine=platform.machine(),
        cpu=sysctl("machdep.cpu.brand_string"),
        logical_cpus=cpus,
        memory_bytes=int(sysctl("hw.memsize")),
        python=platform.python_version(),
    )


def configure_logging(log_dir: Path) -> None:
    """JSON satırı logları hem stderr'e hem `harness.jsonl` dosyasına."""
    formatter = JsonLineFormatter()
    handlers: tuple[logging.Handler, ...] = (
        logging.StreamHandler(sys.stderr),
        logging.FileHandler(log_dir / "harness.jsonl", encoding="utf-8"),
    )
    root = logging.getLogger()
    root.setLevel(logging.INFO)
    for handler in handlers:
        handler.setFormatter(formatter)
        root.addHandler(handler)


def ccm_factory(options: RunOptions) -> Callable[[Path], Adapter]:
    """Her koşu için kendi HOME'u olan bir CCM adapter'ı üretir."""

    def make(home: Path) -> Adapter:
        return CcmAdapter(
            CcmConfig(
                bin_dir=options.bin_dir,
                home=home,
                log_dir=options.log_dir,
                request_timeout_s=options.request_timeout_s,
                command_timeout_s=options.command_timeout_s,
                command_attempts=COMMAND_ATTEMPTS,
            )
        )

    return make


def prepare_work_dir(work_dir: Path) -> Path:
    """Boş çalışma dizini; içi doluysa silmek yerine açık hata verir."""
    if work_dir.exists() and any(work_dir.iterdir()):
        raise HarnessStateError(f"work dir {work_dir} is not empty; inspect and remove it first")
    work_dir.mkdir(parents=True, exist_ok=True)
    return work_dir.resolve()


def scenario_ids(raw: str, available: frozenset[str]) -> frozenset[str]:
    """`all` ya da virgülle ayrılmış senaryo kimlikleri."""
    if raw == "all":
        return available
    return frozenset(part.strip() for part in raw.split(",") if part.strip())


def command_run(options: RunOptions) -> None:
    """Tüm planı koşar; her koşudan sonra sonuç dosyasını yeniden yazar."""
    revision = harness_revision()
    options.log_dir.mkdir(parents=True, exist_ok=True)
    configure_logging(options.log_dir)
    work_dir = prepare_work_dir(options.work_dir)
    make_adapter = ccm_factory(options)
    describe_home = work_dir / "describe-home"
    describe_home.mkdir()
    system = make_adapter(describe_home).describe()
    repos.remove_path(describe_home)
    available = all_scenarios(options.corpus_dir.resolve())
    ids = scenario_ids(options.scenarios, frozenset(item.scenario_id for item in available))
    scenarios = select_scenarios(available, ids)
    plans = build_plans(scenarios, options.repetitions, work_dir, options.ready_timeout_s)
    base = ResultsFile(
        schema_version=SCHEMA_VERSION,
        harness_revision=revision,
        preregistration=PREREGISTRATION,
        system=system,
        environment=environment_info(),
        settings=Settings(
            t_grid_s=T_GRID,
            settle_s=SETTLE_S,
            ready_poll_s=READY_POLL_S,
            ready_timeout_s=options.ready_timeout_s,
            request_timeout_s=options.request_timeout_s,
            repetitions=options.repetitions,
            scenarios=tuple(dict.fromkeys(item.scenario_id for item in scenarios)),
        ),
        started_at=now_iso(),
        finished_at="",
        complete=False,
        runs=(),
    )
    log_event(
        LOGGER,
        logging.INFO,
        "benchmark_started",
        {"system": system.version, "revision": revision, "runs": len(plans)},
    )
    runs: list[RunRecord] = []
    for index, plan in enumerate(plans, start=1):
        name = run_name(plan.scenario, plan.repetition)
        log_event(LOGGER, logging.INFO, "run_started", {"run": name, "index": index})
        try:
            record = execute_run(make_adapter, plan)
        finally:
            repos.remove_path(plan.run_dir)
        runs.append(record)
        write_results(options.out, dataclasses.replace(base, runs=tuple(runs)))
        log_event(
            LOGGER,
            logging.INFO,
            "run_finished",
            {
                "run": name,
                "error": record.error,
                "baseline_ok": record.baseline_ok,
                "verdicts": " ".join(probe.verdict.value for probe in record.probes),
            },
        )
    write_results(
        options.out,
        dataclasses.replace(base, finished_at=now_iso(), complete=True, runs=tuple(runs)),
    )


def command_report(results_path: Path, out: Path) -> None:
    """Ham sonuç dosyasından Markdown özet yazar."""
    results = read_results(results_path)
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(render_report(results), encoding="utf-8")


def build_parser() -> argparse.ArgumentParser:
    """Tüm argümanları zorunlu komut satırı ayrıştırıcısı."""
    parser = argparse.ArgumentParser(
        prog="python -m freshness", description="CCM-Bench L1 freshness harness"
    )
    commands = parser.add_subparsers(dest="command", required=True)
    run = commands.add_parser("run", help="measure one system on the pre-registered scenarios")
    run.add_argument("--system", choices=("ccm",), required=True)
    run.add_argument("--ccm-bin-dir", type=Path, required=True)
    run.add_argument("--corpus-dir", type=Path, required=True)
    run.add_argument("--work-dir", type=Path, required=True)
    run.add_argument("--log-dir", type=Path, required=True)
    run.add_argument("--out", type=Path, required=True)
    run.add_argument("--repetitions", type=int, required=True)
    run.add_argument("--scenarios", required=True, help="'all' or ids such as S1-add,S5")
    run.add_argument("--request-timeout", type=float, required=True)
    run.add_argument("--ready-timeout", type=float, required=True)
    run.add_argument("--command-timeout", type=float, required=True)
    report = commands.add_parser("report", help="render the Markdown summary of a results file")
    report.add_argument("--results", type=Path, required=True)
    report.add_argument("--out", type=Path, required=True)
    return parser


def main() -> int:
    """Komutu çalıştırır; hatalar yakalanmadan yükselir (sessiz geri dönüş yok)."""
    namespace = build_parser().parse_args()
    if namespace.command == "run":
        command_run(
            RunOptions(
                bin_dir=namespace.ccm_bin_dir,
                corpus_dir=namespace.corpus_dir,
                work_dir=namespace.work_dir,
                log_dir=namespace.log_dir,
                out=namespace.out,
                repetitions=namespace.repetitions,
                scenarios=namespace.scenarios,
                request_timeout_s=namespace.request_timeout,
                ready_timeout_s=namespace.ready_timeout,
                command_timeout_s=namespace.command_timeout,
            )
        )
    else:
        command_report(namespace.results, namespace.out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
