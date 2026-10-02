"""`python -m agent check|run|report`: L3 ajan ölçümünün komut satırı girişi."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import sys
from pathlib import Path

from agent.check import check_tasks
from agent.report import ReportError, read_outcomes, render
from agent.run import Arm, Embedding, Secrets, Settings, record_path, run_one
from agent.tasks import TASKS, Task
from freshness.model import JsonValue
from freshness.repos import run_git

PACKAGE_DIR = Path(__file__).resolve().parent
ARMS: tuple[Arm, ...] = ("A", "B", "C")
HARNESS_PATHS = ("benchmarks/agent", "benchmarks/pyproject.toml", "benchmarks/corpus.json")
CCM_SOURCE_PATHS = ("core", "mcp", "cli", "npm", "Cargo.toml", "Cargo.lock", "SKILL.md")
SETUP_SCHEMA_VERSION = 1


class HarnessError(RuntimeError):
    """Ölçüm kurulamadı ya da kayıtlı koşuyla uyumsuz ayar istendi."""


def repository_root() -> Path:
    """Benchmark paketinin bağlı olduğu git deposu."""
    return Path(run_git(PACKAGE_DIR, ("rev-parse", "--show-toplevel")).strip())


def harness_revision() -> str:
    """Ölçüm yönteminin commit'li olduğunu doğrular ve commit kimliğini döner."""
    root = repository_root()
    dirty = run_git(
        root,
        ("status", "--porcelain", "--untracked-files=all", "--", *HARNESS_PATHS),
    ).strip()
    if dirty:
        raise HarnessError(
            "uncommitted L3 methodology; commit the harness before a measured run:\n" + dirty
        )
    return run_git(root, ("rev-parse", "HEAD")).strip()


def command_text(argv: tuple[str, ...], cwd: Path | None = None) -> str:
    """Kısa bir sürüm komutunu çalıştırır; başarısızlık sessizce yutulmaz."""
    completed = subprocess.run(
        argv,
        cwd=cwd,
        capture_output=True,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        raise HarnessError(
            f"{' '.join(argv)} exited {completed.returncode}: {completed.stderr.strip()}"
        )
    return completed.stdout.strip()


def sha256(path: Path) -> str:
    """Bir çalıştırılabilir dosyanın ölçüm kimliği."""
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for block in iter(lambda: handle.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def ccm_dirty(root: Path) -> bool:
    """Benchmark dosyalarından bağımsız, CCM ürün kaynaklarının kirli olup olmadığı."""
    status = run_git(
        root,
        ("status", "--porcelain", "--untracked-files=all", "--", *CCM_SOURCE_PATHS),
    )
    return bool(status.strip())


def settings_json(
    settings: Settings,
    repetitions: int,
    tasks: tuple[Task, ...],
) -> dict[str, JsonValue]:
    """Sonuç dizinine yazılan, tekrar koşuda bire bir eşleşmesi gereken ayarlar."""
    return {
        "model": settings.model,
        "effort": settings.effort,
        "embedding": settings.embedding,
        "max_budget_usd": settings.max_budget_usd,
        "timeout_s": settings.timeout_s,
        "repetitions": repetitions,
        "arms": list(ARMS),
        "tasks": [task.id for task in tasks],
    }


def environment_json(settings: Settings, revision: str) -> dict[str, JsonValue]:
    """Ölçümde kullanılan iki çalıştırılabilir dosya ve kaynak revizyonu."""
    root = repository_root()
    cli = settings.ccm_bin_dir / "ccm-cli"
    mcp = settings.ccm_bin_dir / "ccm-mcp"
    return {
        "claude_version": command_text((str(settings.claude_bin), "--version")),
        "ccm_version": command_text((str(cli), "--version")),
        "ccm_commit": revision,
        "ccm_dirty": ccm_dirty(root),
        "ccm_cli_sha256": sha256(cli),
        "ccm_mcp_sha256": sha256(mcp),
    }


def setup_json(
    settings: Settings,
    repetitions: int,
    tasks: tuple[Task, ...],
    revision: str,
) -> dict[str, JsonValue]:
    """Rapor ve resume için sabit deney tanımı."""
    return {
        "schema_version": SETUP_SCHEMA_VERSION,
        "harness_revision": revision,
        "preregistration": "benchmarks/agent/PREREGISTRATION.md",
        "settings": settings_json(settings, repetitions, tasks),
        "environment": environment_json(settings, revision),
    }


def write_or_verify_setup(path: Path, setup: dict[str, JsonValue]) -> None:
    """İlk koşuda ayarları yazar; resume sırasında değişmiş ayarı reddeder."""
    path.parent.mkdir(parents=True, exist_ok=True)
    if path.exists():
        try:
            existing: JsonValue = json.loads(path.read_text())
        except json.JSONDecodeError as error:
            raise HarnessError(f"{path} is not valid JSON") from error
        if existing != setup:
            raise HarnessError(
                f"{path} does not match this run configuration; use a new output directory"
            )
        return
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(setup, indent=2, ensure_ascii=False) + "\n")
    temporary.replace(path)


def selected_tasks(raw: str) -> tuple[Task, ...]:
    """`all` ya da virgülle ayrılmış görev kimliklerini veri sırasıyla seçer."""
    if raw == "all":
        return TASKS
    requested = {item.strip() for item in raw.split(",") if item.strip()}
    known = {task.id for task in TASKS}
    unknown = sorted(requested - known)
    if unknown:
        raise HarnessError("unknown task ids: " + ", ".join(unknown))
    tasks = tuple(task for task in TASKS if task.id in requested)
    if not tasks:
        raise HarnessError("no tasks selected")
    return tasks


def validate_paths(settings: Settings) -> None:
    """Pahalı ilk çağrıdan önce tüm yerel girdileri doğrular."""
    for binary in (
        settings.claude_bin,
        settings.ccm_bin_dir / "ccm-cli",
        settings.ccm_bin_dir / "ccm-mcp",
    ):
        if not binary.is_file() or not os.access(binary, os.X_OK):
            raise HarnessError(f"executable not found: {binary}")
    if not settings.corpus_dir.is_dir():
        raise HarnessError(f"corpus directory not found: {settings.corpus_dir}")
    if settings.embedding == "local" and not settings.ccm_model_dir.is_dir():
        raise HarnessError(f"local model directory not found: {settings.ccm_model_dir}")


def secrets_from_env(embedding: Embedding) -> Secrets:
    """Anahtarları yalnız ortamdan alır; hiçbir sonuç dosyasına yazmaz."""
    anthropic = os.environ.get("ANTHROPIC_API_KEY")
    if not anthropic:
        raise HarnessError("ANTHROPIC_API_KEY is required for agent runs")
    openai = os.environ.get("OPENAI_API_KEY")
    if embedding == "openai" and not openai:
        raise HarnessError("--embedding openai requires OPENAI_API_KEY")
    return Secrets(anthropic_api_key=anthropic, openai_api_key=openai)


def arm_order(task_index: int, repetition: int) -> tuple[Arm, ...]:
    """Üç tekrarda her görev için ilk kolu dengeli döndürür."""
    shift = (task_index + repetition - 1) % len(ARMS)
    return (*ARMS[shift:], *ARMS[:shift])


def command_check(corpus_dir: Path) -> int:
    """24 görevi ve puanlayıcı işaretlerini sabit korpusa karşı doğrular."""
    problems = check_tasks(TASKS, corpus_dir.resolve())
    if problems:
        for problem in problems:
            print(problem, file=sys.stderr)
        return 1
    print(f"{len(TASKS)} tasks checked: ok")
    return 0


def command_run(
    settings: Settings,
    repetitions: int,
    raw_tasks: str,
) -> int:
    """Eksik koşuları yürütür; tamamlanmış kayıtları resume sırasında atlar."""
    if repetitions < 1:
        raise HarnessError("--repetitions must be at least 1")
    tasks = selected_tasks(raw_tasks)
    validate_paths(settings)
    problems = check_tasks(tasks, settings.corpus_dir)
    if problems:
        raise HarnessError("task validation failed:\n" + "\n".join(problems))
    revision = harness_revision()
    setup = setup_json(settings, repetitions, tasks, revision)
    write_or_verify_setup(settings.out_dir / "settings.json", setup)
    secrets = secrets_from_env(settings.embedding)

    total = len(tasks) * len(ARMS) * repetitions
    completed = 0
    for repetition in range(1, repetitions + 1):
        for task_index, task in enumerate(tasks):
            for arm in arm_order(task_index, repetition):
                name = f"{task.id}__{arm}__{repetition}"
                if record_path(settings.out_dir, name).is_file():
                    completed += 1
                    print(f"[{completed}/{total}] skip {name}")
                    continue
                print(f"[{completed + 1}/{total}] run {name}", flush=True)
                record = run_one(settings, task, arm, repetition, secrets)
                completed += 1
                success = (
                    record.failure is None
                    and record.score is not None
                    and record.score.success
                )
                cost = (
                    "unknown"
                    if record.transcript is None
                    else f"{record.transcript.cost_usd:.4f}"
                )
                turns = (
                    "unknown"
                    if record.transcript is None
                    else str(record.transcript.num_turns)
                )
                wall = "unknown" if record.wall_s is None else f"{record.wall_s:.1f}s"
                print(
                    f"  success={success} cost_usd={cost} turns={turns} wall={wall}",
                    flush=True,
                )
                if record.failure is not None:
                    print(f"  failure={record.failure}", flush=True)
    return 0


def command_report(out_dir: Path, destination: Path) -> int:
    """Tamamlanmış koşuları mevcut ayar dosyasıyla Markdown'a çevirir."""
    setup_path = out_dir / "settings.json"
    if not setup_path.is_file():
        raise ReportError(f"missing {setup_path}")
    try:
        setup: JsonValue = json.loads(setup_path.read_text())
    except json.JSONDecodeError as error:
        raise ReportError(f"{setup_path} is not valid JSON") from error
    outcomes = read_outcomes(out_dir)
    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_text(render(outcomes, setup))
    print(f"wrote {destination} from {len(outcomes)} completed runs")
    return 0


def build_parser() -> argparse.ArgumentParser:
    """L3 kontrol, koşu ve rapor komutları."""
    parser = argparse.ArgumentParser(
        prog="python -m agent",
        description="CCM-Bench L3 real-agent benchmark",
    )
    commands = parser.add_subparsers(dest="command", required=True)

    check = commands.add_parser("check", help="validate all task ground truth against the corpus")
    check.add_argument("--corpus-dir", type=Path, required=True)

    run = commands.add_parser("run", help="run or resume the three-arm agent benchmark")
    run.add_argument("--model", required=True)
    run.add_argument("--effort", required=True)
    run.add_argument("--embedding", choices=("local", "openai"), required=True)
    run.add_argument("--max-budget-usd", type=float, required=True)
    run.add_argument("--timeout-s", type=int, required=True)
    run.add_argument("--repetitions", type=int, required=True)
    run.add_argument("--tasks", required=True, help="'all' or comma-separated task ids")
    run.add_argument("--claude-bin", type=Path, required=True)
    run.add_argument("--ccm-bin-dir", type=Path, required=True)
    run.add_argument("--ccm-model-dir", type=Path, required=True)
    run.add_argument("--corpus-dir", type=Path, required=True)
    run.add_argument("--out-dir", type=Path, required=True)

    report = commands.add_parser("report", help="render a Markdown report from completed runs")
    report.add_argument("--out-dir", type=Path, required=True)
    report.add_argument("--out", type=Path, required=True)
    return parser


def main() -> int:
    """Komutu çalıştırır; yöntem veya kurulum hataları açıkça yükselir."""
    namespace = build_parser().parse_args()
    if namespace.command == "check":
        return command_check(namespace.corpus_dir)
    if namespace.command == "report":
        return command_report(namespace.out_dir, namespace.out)
    embedding: Embedding = namespace.embedding
    settings = Settings(
        model=namespace.model,
        effort=namespace.effort,
        embedding=embedding,
        max_budget_usd=namespace.max_budget_usd,
        timeout_s=namespace.timeout_s,
        claude_bin=namespace.claude_bin.resolve(),
        ccm_bin_dir=namespace.ccm_bin_dir.resolve(),
        ccm_model_dir=namespace.ccm_model_dir.resolve(),
        corpus_dir=namespace.corpus_dir.resolve(),
        out_dir=namespace.out_dir.resolve(),
    )
    return command_run(settings, namespace.repetitions, namespace.tasks)


if __name__ == "__main__":
    raise SystemExit(main())
