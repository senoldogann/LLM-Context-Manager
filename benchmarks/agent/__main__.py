"""`python -m agent check|run|report`: L3 ajan ölçümünün komut satırı girişi."""

from __future__ import annotations

import argparse
import fcntl
import json
import os
import signal
import subprocess
import sys
import time
from collections.abc import Iterator
from contextlib import contextmanager
from pathlib import Path
from types import FrameType

from agent import codex
from agent.check import check_tasks
from agent.context_preflight import command_preflight
from agent.report import ReportError, read_outcomes, render
from agent.run import (
    AgentName,
    Arm,
    Auth,
    AuthError,
    Embedding,
    IsolationError,
    ProductProfile,
    QuotaError,
    Secrets,
    Settings,
    SetupError,
    file_sha256,
    record_path,
    run_one,
    write_atomic,
)
from agent.tasks import KNOWN_TASKS, TASKS, Task
from freshness.model import JsonValue
from freshness.repos import run_git

PACKAGE_DIR = Path(__file__).resolve().parent
DEFAULT_ARMS = "A,B,C"
KNOWN_ARMS: tuple[Arm, ...] = ("A", "B", "C", "D", "E", "F", "H")
# Ön kayıttaki dengeli sıranın kolları; D gibi sonradan eklenen kollar her zaman bunlardan sonra
# koşar, böylece A/B/C'nin kayıtlı sırası değişmez (ek 14).
PREREGISTERED_ROTATION: tuple[Arm, ...] = ("A", "B", "C")
HARNESS_PATHS = ("benchmarks/agent", "benchmarks/pyproject.toml", "benchmarks/corpus.json")
CCM_SOURCE_PATHS = ("core", "mcp", "cli", "npm", "Cargo.toml", "Cargo.lock", "SKILL.md")
SETUP_SCHEMA_VERSION = 1
# `run` çıkış kodları; `--help` metni ve README ile aynı tutulur.
EXIT_BUDGET = 3
EXIT_QUOTA = 4
# Kota duruşunda sonuç dizinine yazılan dosya (bkz. `write_quota_stop`).
QUOTA_STOP_FILE = "quota-stop.json"
EXIT_ISOLATION = 5
EXIT_AUTH = 6
EXIT_SETUP = 7
EXIT_TERMINATED = 128 + signal.SIGTERM
RUN_EXIT_CODES = f"""\
exit codes:
  0    every planned run has a record
  {EXIT_BUDGET}    stopped before a run that could push the total over --max-total-usd
  {EXIT_QUOTA}    usage or rate limit; rerun the same command after it resets
       (scripts/run_until_done.py waits for the reset in quota-stop.json and reruns it)
  {EXIT_ISOLATION}    isolation check failed; no record written
  {EXIT_AUTH}    authentication failed; no record written
  {EXIT_SETUP}    setup or infrastructure failure (workspace, index cache, credentials, agent
       start); no record written, rerunning the same command retries that run
  {EXIT_TERMINATED}  SIGTERM; the running agent was stopped, no record written
  1    harness error, e.g. another process holds <out-dir>/.lock
"""


class HarnessError(RuntimeError):
    """Ölçüm kurulamadı ya da kayıtlı koşuyla uyumsuz ayar istendi."""


class HarnessTerminated(BaseException):
    """Harness SIGTERM aldı; `finally` blokları ajanı durdurup geçici dizinleri silebilsin.

    KeyboardInterrupt gibi `BaseException`dır: koşu hatası yakalayan bloklar onu tutmaz.
    """


def raise_terminated(signum: int, frame: FrameType | None) -> None:
    """SIGTERM işleyicisi: sinyali istisnaya çevirir."""
    raise HarnessTerminated(f"received signal {signum}")


@contextmanager
def sigterm_raises() -> Iterator[None]:
    """Blok süresince SIGTERM'i `HarnessTerminated` istisnasına çevirir, sonra eskisini kurar."""
    previous = signal.signal(signal.SIGTERM, raise_terminated)
    try:
        yield
    finally:
        signal.signal(signal.SIGTERM, previous)


@contextmanager
def exclusive_out_dir(out_dir: Path) -> Iterator[None]:
    """Sonuç dizinini blok süresince tek sürece ayırır (`<out_dir>/.lock`, bloklamayan flock).

    Aynı dizinde ikinci bir harness aynı koşuyu yeniden başlatıp kayıtları ezebilirdi.
    """
    out_dir.mkdir(parents=True, exist_ok=True)
    lock_path = out_dir / ".lock"
    with lock_path.open("w") as lock:
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as error:
            raise HarnessError(
                f"another harness process holds {lock_path}; wait for it to finish or use "
                "another --out-dir"
            ) from error
        yield


def write_quota_stop(out_dir: Path, name: str, error: QuotaError) -> None:
    """Kota duruşunu `quota-stop.json`'a yazar: hangi koşu, ne zaman sıfırlanacağı.

    `scripts/run_until_done.py` bu dosyadan sıfırlanma anını okuyup aynı komutu sürdürür.
    """
    stop: dict[str, JsonValue] = {
        "run": name,
        "resets_at": error.resets_at,
        "message": str(error),
    }
    write_atomic(out_dir / QUOTA_STOP_FILE, json.dumps(stop, indent=2, ensure_ascii=False))
    if error.resets_at is not None:
        reset = time.strftime("%Y-%m-%d %H:%M %Z", time.localtime(error.resets_at))
        print(f"the limit resets at {reset}", flush=True)


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
    arms: tuple[Arm, ...],
) -> dict[str, JsonValue]:
    """Sonuç dizinine yazılan, tekrar koşuda bire bir eşleşmesi gereken ayarlar."""
    return {
        "model": settings.model,
        "effort": settings.effort,
        "embedding": settings.embedding,
        "auth": settings.auth,
        "profile": settings.profile,
        "agent": settings.agent,
        "max_budget_usd": settings.max_budget_usd,
        "timeout_s": settings.timeout_s,
        "repetitions": repetitions,
        "arms": list(arms),
        "tasks": [task.id for task in tasks],
    }


def environment_json(settings: Settings, revision: str) -> dict[str, JsonValue]:
    """Ajan ve CCM sürümleri, CCM ikili dosyalarının özetleri ve kaynak revizyonu."""
    root = repository_root()
    cli = settings.ccm_bin_dir / "ccm-cli"
    mcp = settings.ccm_bin_dir / "ccm-mcp"
    return {
        "agent_version": command_text((str(settings.agent_bin), "--version")),
        "agent_login": codex_login(settings) if settings.agent == "codex" else "environment",
        "ccm_version": command_text((str(cli), "--version")),
        "ccm_commit": revision,
        "ccm_dirty": ccm_dirty(root),
        "ccm_cli_sha256": file_sha256(cli),
        "ccm_mcp_sha256": file_sha256(mcp),
    }


def setup_json(
    settings: Settings,
    repetitions: int,
    tasks: tuple[Task, ...],
    arms: tuple[Arm, ...],
    revision: str,
) -> dict[str, JsonValue]:
    """Rapor ve resume için sabit deney tanımı."""
    return {
        "schema_version": SETUP_SCHEMA_VERSION,
        "harness_revision": revision,
        "preregistration": "benchmarks/agent/PREREGISTRATION.md",
        "settings": settings_json(settings, repetitions, tasks, arms),
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
    write_atomic(path, json.dumps(setup, indent=2, ensure_ascii=False) + "\n")


def selected_tasks(raw: str) -> tuple[Task, ...]:
    """`all` ya da virgülle ayrılmış görev kimliklerini veri sırasıyla seçer."""
    if raw == "all":
        return TASKS
    requested = {item.strip() for item in raw.split(",") if item.strip()}
    known = {task.id for task in KNOWN_TASKS}
    unknown = sorted(requested - known)
    if unknown:
        raise HarnessError("unknown task ids: " + ", ".join(unknown))
    tasks = tuple(task for task in KNOWN_TASKS if task.id in requested)
    if not tasks:
        raise HarnessError("no tasks selected")
    return tasks


def validate_paths(settings: Settings) -> None:
    """Pahalı ilk çağrıdan önce tüm yerel girdileri doğrular."""
    for binary in (
        settings.agent_bin,
        settings.ccm_bin_dir / "ccm-cli",
        settings.ccm_bin_dir / "ccm-mcp",
    ):
        if not binary.is_file() or not os.access(binary, os.X_OK):
            raise HarnessError(f"executable not found: {binary}")
    if settings.agent == "codex" and (
        settings.codex_home is None or not settings.codex_home.is_dir()
    ):
        raise HarnessError(
            "--agent codex needs --codex-home, a directory logged in with codex login"
        )
    if not settings.corpus_dir.is_dir():
        raise HarnessError(f"corpus directory not found: {settings.corpus_dir}")
    if settings.embedding == "local" and not settings.ccm_model_dir.is_dir():
        raise HarnessError(f"local model directory not found: {settings.ccm_model_dir}")


def required_env(name: str, reason: str) -> str:
    """Zorunlu ortam değişkeni; yoksa ne yapılacağını söyleyen açık hata."""
    value = os.environ.get(name)
    if not value:
        raise HarnessError(f"{name} is required: {reason}")
    return value


def codex_login(settings: Settings) -> str:
    """Deneye özel CODEX_HOME'un ChatGPT ile giriş yaptığını model çağırmadan doğrular."""
    if settings.auth != "subscription":
        raise HarnessError(
            "--agent codex runs on the ChatGPT subscription; use --auth subscription"
        )
    if settings.codex_home is None:
        raise HarnessError("--agent codex needs --codex-home")
    status = codex.login_status(settings.agent_bin, settings.codex_home)
    if "ChatGPT" not in status:
        raise HarnessError(
            f"{settings.codex_home} is not logged in with ChatGPT ({status!r}); "
            f"run: CODEX_HOME={settings.codex_home} codex login"
        )
    return status


def secrets_from_env(agent: AgentName, embedding: Embedding, auth: Auth) -> Secrets:
    """Yalnız seçilen ajan ve kipin kimlik bilgilerini ortamdan alır; hiçbir dosyaya yazmaz.

    Codex kimliğini deneye özel CODEX_HOME'dan kendisi okur; harness ona anahtar geçirmez.
    """
    claude = agent == "claude"
    return Secrets(
        claude_oauth_token=(
            required_env("CLAUDE_CODE_OAUTH_TOKEN", "create one with `claude setup-token`")
            if claude and auth == "subscription"
            else None
        ),
        anthropic_api_key=(
            required_env("ANTHROPIC_API_KEY", "--auth api-key bills the API key")
            if claude and auth == "api-key"
            else None
        ),
        openai_api_key=(
            required_env("OPENAI_API_KEY", "--embedding openai embeds through the OpenAI API")
            if embedding == "openai"
            else None
        ),
    )


def arm_order(arms: tuple[Arm, ...], task_index: int, repetition: int) -> tuple[Arm, ...]:
    """Tekrarlar boyunca her görev için ilk kolu dengeli döndürür."""
    shift = (task_index + repetition - 1) % len(arms)
    return (*arms[shift:], *arms[:shift])


def run_order(
    profile: ProductProfile, arms: tuple[Arm, ...], task_index: int, repetition: int
) -> tuple[Arm, ...]:
    """Koşu sırası: preregistered profilde yalnız A/B/C döner ve diğer kollar onlardan sonra
    gelir; pilot profillerinde tüm kollar döner (önceki davranış)."""
    if profile != "preregistered":
        return arm_order(arms, task_index, repetition)
    rotated = tuple(arm for arm in arms if arm in PREREGISTERED_ROTATION)
    extra = tuple(arm for arm in arms if arm not in PREREGISTRATION_ROTATION_SET)
    return (*arm_order(rotated, task_index, repetition), *extra)


PREREGISTRATION_ROTATION_SET = frozenset(PREREGISTERED_ROTATION)


def selected_arms(raw: str) -> tuple[Arm, ...]:
    """Virgülle ayrılmış kolları bilinen sırayla seçer; A her zaman karşılaştırma tabanıdır."""
    requested = [item.strip() for item in raw.split(",") if item.strip()]
    unknown = sorted(set(requested) - set(KNOWN_ARMS))
    if unknown:
        raise HarnessError("unknown arms: " + ", ".join(unknown))
    arms = tuple(arm for arm in KNOWN_ARMS if arm in requested)
    if "A" not in arms:
        raise HarnessError("arm A (built-ins) is required as the comparison baseline")
    return arms


def command_check(corpus_dir: Path) -> int:
    """24 görevi ve puanlayıcı işaretlerini sabit korpusa karşı doğrular."""
    problems = check_tasks(KNOWN_TASKS, corpus_dir.resolve())
    if problems:
        for problem in problems:
            print(problem, file=sys.stderr)
        return 1
    print(f"{len(KNOWN_TASKS)} tasks checked: ok")
    return 0


def spent_usd(out_dir: Path) -> float:
    """Sonuç dizinindeki tamamlanmış koşuların bilinen toplam maliyeti."""
    return sum(
        outcome.cost_usd for outcome in read_outcomes(out_dir) if outcome.cost_usd is not None
    )


def command_run(
    settings: Settings,
    repetitions: int,
    raw_tasks: str,
    arms: tuple[Arm, ...],
    max_total_usd: float,
) -> int:
    """Eksik koşuları yürütür; tamamlanmış kayıtları atlar, toplam bütçeyi aşmadan durur.

    Bir koşu en çok `--max-budget-usd` harcayabildiği için, harcanan tutara bu eklendiğinde
    `--max-total-usd` aşılacaksa yeni koşu başlatılmaz (çıkış kodu 3); aynı komut daha yüksek
    bir sınırla yeniden çalıştırılırsa kaldığı yerden sürer. Komut boyunca sonuç dizini
    `<out_dir>/.lock` ile bu sürece ayrılır; SIGTERM ajanı durdurur ve kayıt yazılmaz.
    """
    if repetitions < 1:
        raise HarnessError("--repetitions must be at least 1")
    if max_total_usd < settings.max_budget_usd:
        raise HarnessError("--max-total-usd must be at least --max-budget-usd")
    if ("F" in arms or "H" in arms) and settings.profile != "context-first-pilot":
        raise HarnessError("arms F and H exist only in the context-first-pilot profile")
    if ("D" in arms or "E" in arms) and settings.profile != "preregistered":
        raise HarnessError("arms D and E exist only in the preregistered profile")
    if ("E" in arms or "H" in arms) and settings.agent != "claude":
        raise HarnessError("arms E and H (Claude Code hooks) are only defined for --agent claude")
    with exclusive_out_dir(settings.out_dir), sigterm_raises():
        try:
            return run_missing(settings, repetitions, raw_tasks, arms, max_total_usd)
        except HarnessTerminated as error:
            print(f"stopping: {error}; the running agent was stopped, no record written.")
            return EXIT_TERMINATED


def run_missing(
    settings: Settings,
    repetitions: int,
    raw_tasks: str,
    arms: tuple[Arm, ...],
    max_total_usd: float,
) -> int:
    """Kilit alınmış sonuç dizininde ayarları doğrular ve eksik koşuları sırayla yürütür."""
    tasks = selected_tasks(raw_tasks)
    validate_paths(settings)
    problems = check_tasks(tasks, settings.corpus_dir)
    if problems:
        raise HarnessError("task validation failed:\n" + "\n".join(problems))
    revision = harness_revision()
    setup = setup_json(settings, repetitions, tasks, arms, revision)
    write_or_verify_setup(settings.out_dir / "settings.json", setup)
    secrets = secrets_from_env(settings.agent, settings.embedding, settings.auth)

    total = len(tasks) * len(arms) * repetitions
    completed = 0
    spent = spent_usd(settings.out_dir)
    for repetition in range(1, repetitions + 1):
        for task_index, task in enumerate(tasks):
            for arm in run_order(settings.profile, arms, task_index, repetition):
                name = f"{task.id}__{arm}__{repetition}"
                if record_path(settings.out_dir, name).is_file():
                    completed += 1
                    print(f"[{completed}/{total}] skip {name}")
                    continue
                if spent + settings.max_budget_usd > max_total_usd:
                    print(
                        f"stopping before {name}: spent ${spent:.2f}; one more run could "
                        f"exceed --max-total-usd {max_total_usd:.2f}",
                        flush=True,
                    )
                    return EXIT_BUDGET
                print(f"[{completed + 1}/{total}] run {name} (spent ${spent:.2f})", flush=True)
                try:
                    record = run_one(settings, task, arm, repetition, secrets)
                except QuotaError as error:
                    print(f"stopping at {name}: usage or rate limit ({error}).", flush=True)
                    write_quota_stop(settings.out_dir, name, error)
                    print("Rerun the same command after the limit resets.", flush=True)
                    return EXIT_QUOTA
                except IsolationError as error:
                    print(f"stopping at {name}: isolation check failed: {error}", flush=True)
                    return EXIT_ISOLATION
                except AuthError as error:
                    print(f"stopping at {name}: authentication failed ({error}).", flush=True)
                    print("Create a new token with `claude setup-token` (or log in again).")
                    return EXIT_AUTH
                except SetupError as error:
                    print(f"stopping at {name}: setup failed ({error}).", flush=True)
                    print("No record was written; fix the cause and rerun the same command.")
                    return EXIT_SETUP
                completed += 1
                known_cost = None if record.transcript is None else record.transcript.cost_usd
                if known_cost is not None:
                    spent += known_cost
                success = (
                    record.failure is None and record.score is not None and record.score.success
                )
                cost = "unknown" if known_cost is None else f"{known_cost:.4f}"
                known_turns = None if record.transcript is None else record.transcript.num_turns
                turns = "unknown" if known_turns is None else str(known_turns)
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

    preflight = commands.add_parser(
        "preflight-context",
        help="verify the three context-first pilot packets without calling an agent or API",
    )
    preflight.add_argument("--ccm-bin-dir", type=Path, required=True)
    preflight.add_argument("--corpus-dir", type=Path, required=True)
    preflight.add_argument(
        "--live-edits",
        action="store_true",
        help="index edit tasks before editing, then check the first live post-edit context packet",
    )

    run = commands.add_parser(
        "run",
        help="run or resume the three-arm agent benchmark",
        epilog=RUN_EXIT_CODES,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    run.add_argument("--model", required=True)
    run.add_argument("--effort", required=True)
    run.add_argument("--embedding", choices=("local", "openai"), required=True)
    run.add_argument(
        "--profile",
        choices=("preregistered", "context-first-pilot"),
        default="preregistered",
        help="preregistered keeps the final A/B/C design; context-first-pilot compares the new "
        "default context surface against the legacy full surface",
    )
    run.add_argument(
        "--auth",
        choices=("subscription", "api-key"),
        required=True,
        help="subscription: CLAUDE_CODE_OAUTH_TOKEN from `claude setup-token`; "
        "api-key: ANTHROPIC_API_KEY",
    )
    run.add_argument("--max-budget-usd", type=float, required=True, help="cap per agent run")
    run.add_argument(
        "--max-total-usd",
        type=float,
        required=True,
        help="no new run starts if it could push the total over this amount",
    )
    run.add_argument("--timeout-s", type=int, required=True)
    run.add_argument("--repetitions", type=int, required=True)
    run.add_argument("--tasks", required=True, help="'all' or comma-separated task ids")
    run.add_argument(
        "--arms",
        default=DEFAULT_ARMS,
        help="comma-separated arms; F (forced context-first) and H (Claude Code Grep hook) exist "
        "only in context-first-pilot",
    )
    run.add_argument("--agent", choices=("claude", "codex"), required=True)
    run.add_argument(
        "--agent-bin",
        type=Path,
        required=True,
        help="claude executable, or the native codex binary (vendor/<triple>/bin/codex)",
    )
    run.add_argument(
        "--codex-home",
        type=Path,
        help="dedicated CODEX_HOME logged in once with `CODEX_HOME=<dir> codex login`",
    )
    run.add_argument("--ccm-bin-dir", type=Path, required=True)
    run.add_argument("--ccm-model-dir", type=Path, required=True)
    run.add_argument("--corpus-dir", type=Path, required=True)
    run.add_argument("--out-dir", type=Path, required=True)
    run.add_argument(
        "--index-cache-dir",
        type=Path,
        required=True,
        help="where each repository's index is built once (outside the repository); an "
        "entry is keyed by repository, corpus HEAD, embedding source and model, and the "
        "ccm-cli hash, and is only reused when its entry.json matches",
    )

    report = commands.add_parser("report", help="render a Markdown report from completed runs")
    report.add_argument("--out-dir", type=Path, required=True)
    report.add_argument("--out", type=Path, required=True)
    return parser


def main() -> int:
    """Komutu çalıştırır; yöntem veya kurulum hataları açıkça yükselir."""
    namespace = build_parser().parse_args()
    if namespace.command == "check":
        return command_check(namespace.corpus_dir)
    if namespace.command == "preflight-context":
        return command_preflight(namespace.ccm_bin_dir, namespace.corpus_dir, namespace.live_edits)
    if namespace.command == "report":
        return command_report(namespace.out_dir, namespace.out)
    embedding: Embedding = namespace.embedding
    auth: Auth = namespace.auth
    profile: ProductProfile = namespace.profile
    agent: AgentName = namespace.agent
    codex_home: Path | None = namespace.codex_home
    settings = Settings(
        model=namespace.model,
        effort=namespace.effort,
        embedding=embedding,
        auth=auth,
        profile=profile,
        max_budget_usd=namespace.max_budget_usd,
        timeout_s=namespace.timeout_s,
        agent=agent,
        agent_bin=namespace.agent_bin.expanduser().resolve(),
        codex_home=None if codex_home is None else codex_home.expanduser().resolve(),
        ccm_bin_dir=namespace.ccm_bin_dir.resolve(),
        ccm_model_dir=namespace.ccm_model_dir.resolve(),
        index_cache_dir=namespace.index_cache_dir.expanduser().resolve(),
        corpus_dir=namespace.corpus_dir.resolve(),
        out_dir=namespace.out_dir.resolve(),
    )
    return command_run(
        settings,
        namespace.repetitions,
        namespace.tasks,
        selected_arms(namespace.arms),
        namespace.max_total_usd,
    )


if __name__ == "__main__":
    raise SystemExit(main())
