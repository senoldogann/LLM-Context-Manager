"""Arama sarmalayıcısı pilotu: Codex aynı görevlerde A (düz rg) ve S (CCM kompakt rg) kollarında.

Ön kayıtlı L3 harness'ine dokunmaz; onun görevlerini, istemini, Codex komutunu ve ortamını, döküm
okuyucusunu ve puanlayıcısını kullanır. İki kolda da PATH'in başında, paketlenmiş aynı `rg`'yi
çağıran bir sarmalayıcı vardır: A'da çağrıyı kaydedip çıktıya dokunmaz, S'de `search.rg_shim`
çıktıyı kompakt biçime çevirir. Ölçüm yalnız commit'lenmiş kodla başlar ve
kota yalnız sahibin açık onayıyla harcanır. Sınır ya da kimlik hatası ölçümü durdurur; tamamlanan
kayıtlar kalır.

Kullanım:
  uv run python -m search.pilot run --agent-bin PATH --codex-home DIR --corpus-dir DIR \\
    --model gpt-6.1-sol --effort high --tasks id1,id2 --arms A,S --repetitions 1 \\
    --timeout-s 1800 --out-dir DIR
  uv run python -m search.pilot report --out-dir DIR
"""

from __future__ import annotations

import argparse
import json
import re
import shlex
import shutil
import statistics
import subprocess
import sys
import tempfile
from collections import Counter
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Literal

from agent import codex
from agent.run import (
    AuthError,
    QuotaError,
    RunError,
    codex_transcript,
    hit_auth_failure,
    hit_quota,
    prepare_workspace,
    run_agent,
    verify_codex,
)
from agent.score import marker_files, score
from agent.tasks import TASKS, Task, prompt_for
from freshness.jsonio import as_object, as_str

PilotArm = Literal["A", "S"]
BENCHMARKS_DIR: Path = Path(__file__).resolve().parents[1]
REPO_DIR: Path = BENCHMARKS_DIR.parent
# Kabuk komutundaki çağrılar; Codex komutları `/bin/zsh -lc '...'` biçimindedir.
RG_CALL = re.compile(r"(?:^|[\s;&|(/'\"])rg\s")
READ_CALL = re.compile(r"(?:^|[\s;&|(/'\"])(?:cat|sed|head|tail|nl|less|bat|awk)\s")


@dataclass(frozen=True)
class PilotConfig:
    agent_bin: Path
    codex_home: Path
    corpus_dir: Path
    model: str
    effort: str
    timeout_s: int
    out_dir: Path


@dataclass(frozen=True)
class PilotRecord:
    run_id: str
    task_id: str
    repo: str
    category: str
    arm: str
    rep: int
    rg_path: str | None
    wall_s: float | None
    input_tokens: int | None
    cached_input_tokens: int | None
    output_tokens: int | None
    tool_calls: dict[str, int]
    shell_calls: int
    rg_calls: int
    read_calls: int
    shim_modes: dict[str, int]
    shim_raw_bytes: int
    shim_output_bytes: int
    success: bool | None
    recall: float | None
    precision: float | None
    failure: str | None


def git(args: list[str]) -> str:
    completed = subprocess.run(
        ["git", *args], cwd=REPO_DIR, capture_output=True, text=True, check=False
    )
    if completed.returncode != 0:
        raise RunError(f"git {' '.join(args)} failed: {completed.stderr.strip()}")
    return completed.stdout.strip()


def require_committed() -> None:
    """Ölçüm, kaydedeceği commit'teki kodla yapılmalıdır."""
    dirty = git(["status", "--porcelain", "--", "benchmarks/search", "benchmarks/agent"])
    if dirty:
        raise RunError(
            f"commit benchmarks/search and benchmarks/agent before a measured run:\n{dirty}"
        )


def write_baseline_wrapper(path: Path, real_rg: Path, log: Path) -> None:
    """A kolunun `rg`'si: çağrıyı kaydeder ve aynı gerçek `rg`'yi aynen çalıştırır.

    İki kolda da PATH'in başında bir sarmalayıcı bulunur; kollar yalnız çıktı biçiminde ayrışır ve
    kayıt sarmalayıcının gerçekten çağrıldığını kanıtlar.
    """
    entry = json.dumps({"mode": "baseline", "raw_bytes": 0, "output_bytes": 0})
    script = (
        "#!/bin/sh\n"
        f"printf '%s\\n' {shlex.quote(entry)} >> {shlex.quote(str(log))}\n"
        f'exec {shlex.quote(str(real_rg))} "$@"\n'
    )
    path.write_text(script)
    path.chmod(0o755)


def write_shim(path: Path, real_rg: Path, log: Path) -> None:
    """S kolunun `rg`'si: tek satırlık kabuk betiği, sarmalayıcı modülü aynı süreçte çalıştırır."""
    script = (
        "#!/bin/sh\n"
        f"exec /usr/bin/env PYTHONPATH={shlex.quote(str(BENCHMARKS_DIR))} "
        f"CCM_REAL_RG={shlex.quote(str(real_rg))} CCM_SHIM_LOG={shlex.quote(str(log))} "
        f'{shlex.quote(sys.executable)} -m search.rg_shim "$@"\n'
    )
    path.write_text(script)
    path.chmod(0o755)


def shell_commands(lines: list[str]) -> list[str]:
    """Dökümdeki tamamlanmış kabuk komutları."""
    commands: list[str] = []
    for number, line in enumerate(lines, 1):
        if not line.strip():
            continue
        event = as_object(json.loads(line), f"transcript line {number}", RunError)
        if event.get("type") != "item.completed":
            continue
        item = as_object(event.get("item"), "item", RunError)
        if item.get("type") == "command_execution":
            commands.append(as_str(item.get("command"), "command", RunError))
    return commands


def shim_summary(log: Path) -> tuple[dict[str, int], int, int]:
    """Sarmalayıcı kaydı: kip sayıları, kompakt çağrılarda ham ve gösterilen bayt toplamı."""
    if not log.is_file():
        return {}, 0, 0
    modes: Counter[str] = Counter()
    raw_bytes = 0
    output_bytes = 0
    for number, line in enumerate(log.read_text().splitlines(), 1):
        event = as_object(json.loads(line), f"shim log line {number}", RunError)
        mode = as_str(event.get("mode"), "mode", RunError)
        modes[mode] += 1
        if mode == "compact":
            raw_value = event.get("raw_bytes")
            output_value = event.get("output_bytes")
            if not isinstance(raw_value, int) or not isinstance(output_value, int):
                raise RunError(f"shim log line {number} has non-integer sizes")
            raw_bytes += raw_value
            output_bytes += output_value
    return dict(modes), raw_bytes, output_bytes


def run_one(config: PilotConfig, task: Task, arm: PilotArm, rep: int) -> PilotRecord:
    """Bir koşu; ajan ya da döküm hataları başarısız kayıt olarak saklanır."""
    name = f"{task.id}__{arm}__{rep}"
    work = Path(tempfile.mkdtemp(prefix="ccm-search-pilot-"))
    home = work / "home"
    home.mkdir()
    workspace = work / f"ws-{name}"
    transcript_path = config.out_dir / "transcripts" / f"{name}.jsonl"
    # Codex kum havuzu .git'e yazdırmaz: kayıt kökte gizli dosyadır ve git'ten dışlanır.
    shim_log = workspace / ".ccm-shim.jsonl"
    wall_s: float | None = None
    rg_path: str | None = None
    result: codex.CodexResult | None = None
    commands: list[str] = []
    modes: dict[str, int] = {}
    raw_bytes = 0
    output_bytes = 0
    success: bool | None = None
    recall: float | None = None
    precision: float | None = None
    failure: str | None = None
    try:
        prepare_workspace(config.corpus_dir / task.repo, workspace)
        with (workspace / ".git" / "info" / "exclude").open("a") as exclude:
            exclude.write(f"/{shim_log.name}\n")
        base = codex.command(
            config.agent_bin, config.model, config.effort, workspace, prompt_for(task), None
        )
        # Kabuk anlık görüntüsü PATH'i kendisi kurar ve sarmalayıcıyı atlar; iki kolda da kapalı.
        command = [*base[:-1], "--disable", "shell_snapshot", base[-1]]
        real_rg = codex.bundled_path(config.agent_bin) / "rg"
        rg_path = str(real_rg)
        wrapper_dir = work / "wrapper"
        wrapper_dir.mkdir()
        if arm == "S":
            write_shim(wrapper_dir / "rg", real_rg, shim_log)
        else:
            write_baseline_wrapper(wrapper_dir / "rg", real_rg, shim_log)
        env = codex.environment(config.agent_bin, home, config.codex_home)
        # Codex kabuk anlık görüntüsü ortam PATH'ini kullanır; giriş kabuğu ise /etc/zprofile'dan
        # sonra HOME/.zprofile'ı okur. Sarmalayıcı iki durumda da öndedir.
        env = {**env, "PATH": f"{wrapper_dir}:{env['PATH']}"}
        (home / ".zprofile").write_text(f"export PATH={shlex.quote(str(wrapper_dir))}:$PATH\n")
        wall_s = run_agent(
            command,
            workspace,
            env,
            config.timeout_s,
            transcript_path,
            transcript_path.with_suffix(".stderr"),
        )
        lines = transcript_path.read_text().splitlines()
        result = codex.parse(lines)
        transcript = codex_transcript(result, wall_s)
        verify_codex(transcript, "preregistered", "A")
        if hit_auth_failure(transcript):
            raise AuthError(f"{name}: {transcript.result_text[:300]}")
        if hit_quota(transcript):
            raise QuotaError(f"{name}: {transcript.result_text[:300]}")
        if result.failure is not None:
            failure = f"codex failure: {result.failure}"
        contents = (
            {}
            if task.edit is None
            else {path: (workspace / path).read_text() for path in marker_files(task.edit)}
        )
        task_score = score(task, transcript.result_text, workspace.name, contents)
        success = task_score.success
        recall = task_score.recall
        precision = task_score.precision
        commands = shell_commands(lines)
        modes, raw_bytes, output_bytes = shim_summary(shim_log)
        if any(RG_CALL.search(c) for c in commands) and not modes:
            missing = "the rg wrapper was not invoked although the agent ran rg"
            failure = missing if failure is None else f"{failure}; {missing}"
    except (RunError, codex.CodexOutputError) as error:
        failure = str(error)
    finally:
        shutil.rmtree(work)
    record = PilotRecord(
        run_id=name, task_id=task.id, repo=task.repo, category=task.category, arm=arm, rep=rep,
        rg_path=rg_path, wall_s=wall_s,
        input_tokens=None if result is None else result.input_tokens,
        cached_input_tokens=None if result is None else result.cached_input_tokens,
        output_tokens=None if result is None else result.output_tokens,
        tool_calls={} if result is None else result.tool_calls,
        shell_calls=len(commands), rg_calls=sum(bool(RG_CALL.search(c)) for c in commands),
        read_calls=sum(bool(READ_CALL.search(c)) for c in commands), shim_modes=modes,
        shim_raw_bytes=raw_bytes, shim_output_bytes=output_bytes, success=success, recall=recall,
        precision=precision, failure=failure,
    )  # fmt: skip
    path = config.out_dir / "runs" / f"{name}.json"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(asdict(record), indent=2) + "\n")
    return record


def parse_arms(text: str) -> list[PilotArm]:
    arms: list[PilotArm] = []
    for part in text.split(","):
        if part == "A":
            arms.append("A")
        elif part == "S":
            arms.append("S")
        else:
            raise SystemExit(f"unknown arm {part!r}; use A and/or S")
    return arms


def run_set(args: argparse.Namespace) -> None:
    require_committed()
    out_dir: Path = args.out_dir
    if out_dir.exists() and any(out_dir.iterdir()):
        raise SystemExit(f"{out_dir} is not empty; use a new directory")
    by_id = {task.id: task for task in TASKS}
    unknown = [task_id for task_id in args.tasks.split(",") if task_id not in by_id]
    if unknown:
        raise SystemExit(f"unknown task ids: {unknown}")
    tasks = [by_id[task_id] for task_id in args.tasks.split(",")]
    arms = parse_arms(args.arms)
    status = codex.login_status(args.agent_bin, args.codex_home)
    if "logged in" not in status.lower():
        raise SystemExit(f"codex is not logged in for {args.codex_home}: {status}")
    version = subprocess.run(
        [str(args.agent_bin), "--version"], capture_output=True, text=True, check=True
    )
    config = PilotConfig(
        agent_bin=args.agent_bin, codex_home=args.codex_home, corpus_dir=args.corpus_dir,
        model=args.model, effort=args.effort, timeout_s=args.timeout_s, out_dir=out_dir,
    )  # fmt: skip
    out_dir.mkdir(parents=True, exist_ok=True)
    settings = {
        "product_commit": git(["rev-parse", "HEAD"]),
        "agent_version": version.stdout.strip(),
        "model": args.model,
        "effort": args.effort,
        "tasks": [task.id for task in tasks],
        "arms": list(arms),
        "repetitions": args.repetitions,
        "timeout_s": args.timeout_s,
        "shim_python": sys.executable,
    }
    (out_dir / "settings.json").write_text(json.dumps(settings, indent=2) + "\n")
    for rep in range(1, args.repetitions + 1):
        for position, task in enumerate(tasks):
            # Kol sırası görevden göreve değişir: zaman ve önbellek etkisi tek kola yığılmasın.
            order = arms if (position + rep) % 2 == 0 else list(reversed(arms))
            for arm in order:
                record = run_one(config, task, arm, rep)
                tokens = (
                    None
                    if record.input_tokens is None or record.output_tokens is None
                    else (record.input_tokens + record.output_tokens)
                )
                print(
                    f"{record.run_id}: success={record.success} tokens={tokens} "
                    f"rg={record.rg_calls} reads={record.read_calls} failure={record.failure}",
                    flush=True,
                )


def total_tokens(record: dict[str, object]) -> int | None:
    value_in = record.get("input_tokens")
    value_out = record.get("output_tokens")
    if isinstance(value_in, int) and isinstance(value_out, int):
        return value_in + value_out
    return None


def number(record: dict[str, object], key: str) -> int:
    value = record.get(key)
    if not isinstance(value, int):
        raise RunError(f"{record.get('run_id')}: {key} is not an integer")
    return value


def report(out_dir: Path) -> str:
    """Kol toplamları, görev başına S/A oranı, komut sayıları ve sarmalayıcı kipleri."""
    records: list[dict[str, object]] = []
    for path in sorted((out_dir / "runs").glob("*.json")):
        loaded = json.loads(path.read_text())
        if not isinstance(loaded, dict):
            raise RunError(f"{path} is not a JSON object")
        records.append(loaded)
    lines = [f"# Search wrapper pilot: {out_dir.name}", ""]
    settings = json.loads((out_dir / "settings.json").read_text())
    lines.append(f"Settings: `{json.dumps(settings)}`")
    lines.append("")
    lines.append(
        "| Arm | Runs | Success | Total tokens | Uncached input | Cached input | Output "
        "| Shell | rg | Reads |"
    )
    lines.append("|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|")
    for arm in ("A", "S"):
        group = [r for r in records if r.get("arm") == arm]
        if not group:
            continue
        measured = [r for r in group if total_tokens(r) is not None]
        total = sum(total_tokens(r) or 0 for r in measured)
        cached = sum(number(r, "cached_input_tokens") for r in measured)
        uncached = sum(number(r, "input_tokens") for r in measured) - cached
        output = sum(number(r, "output_tokens") for r in measured)
        successes = sum(r.get("success") is True for r in group)
        shell = sum(number(r, "shell_calls") for r in group)
        searches = sum(number(r, "rg_calls") for r in group)
        reads = sum(number(r, "read_calls") for r in group)
        lines.append(
            f"| {arm} | {len(group)} | {successes}/{len(group)} | {total:,} | {uncached:,} | "
            f"{cached:,} | {output:,} | {shell} | {searches} | {reads} |"
        )
    lines.append("")
    lines.append("| Task | A tokens | S tokens | S/A | A success | S success |")
    lines.append("|---|---:|---:|---:|---:|---:|")
    ratios: list[float] = []
    for task_id in sorted({str(r.get("task_id")) for r in records}):
        means: dict[str, float] = {}
        wins: dict[str, str] = {}
        for arm in ("A", "S"):
            values = [
                total_tokens(r)
                for r in records
                if r.get("task_id") == task_id and r.get("arm") == arm
            ]
            known = [v for v in values if v is not None]
            if known:
                means[arm] = statistics.mean(known)
            runs = [r for r in records if r.get("task_id") == task_id and r.get("arm") == arm]
            wins[arm] = f"{sum(r.get('success') is True for r in runs)}/{len(runs)}"
        if "A" in means and "S" in means:
            ratio = means["S"] / means["A"]
            ratios.append(ratio)
            lines.append(
                f"| {task_id} | {means['A']:,.0f} | {means['S']:,.0f} | {ratio:.2f} | "
                f"{wins['A']} | {wins['S']} |"
            )
    if ratios:
        median_ratio = statistics.median(ratios)
        lines.append("")
        lines.append(
            f"Median per-task S/A total-token ratio: {median_ratio:.2f} ({len(ratios)} tasks)."
        )
    modes: Counter[str] = Counter()
    raw_bytes = 0
    output_bytes = 0
    for r in records:
        if r.get("arm") != "S":
            continue
        shim = r.get("shim_modes")
        if isinstance(shim, dict):
            for key, value in shim.items():
                if isinstance(key, str) and isinstance(value, int):
                    modes[key] += value
        raw_bytes += number(r, "shim_raw_bytes")
        output_bytes += number(r, "shim_output_bytes")
    lines.append("")
    lines.append(f"Wrapper modes in S: {dict(modes)}")
    if raw_bytes:
        share = output_bytes / raw_bytes
        lines.append(
            f"Compacted rg calls: {raw_bytes:,} raw bytes shown as {output_bytes:,} ({share:.2f})."
        )
    failures = [f"- {r.get('run_id')}: {r.get('failure')}" for r in records if r.get("failure")]
    lines.append("")
    lines.append("Failures:" if failures else "Failures: none.")
    lines.extend(failures)
    text = "\n".join(lines) + "\n"
    (out_dir / "summary.md").write_text(text)
    return text


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    commands = parser.add_subparsers(dest="action", required=True)
    run_parser = commands.add_parser("run")
    run_parser.add_argument("--agent-bin", type=Path, required=True)
    run_parser.add_argument("--codex-home", type=Path, required=True)
    run_parser.add_argument("--corpus-dir", type=Path, required=True)
    run_parser.add_argument("--model", required=True)
    run_parser.add_argument("--effort", required=True)
    run_parser.add_argument("--tasks", required=True)
    run_parser.add_argument("--arms", required=True)
    run_parser.add_argument("--repetitions", type=int, required=True)
    run_parser.add_argument("--timeout-s", type=int, required=True)
    run_parser.add_argument("--out-dir", type=Path, required=True)
    report_parser = commands.add_parser("report")
    report_parser.add_argument("--out-dir", type=Path, required=True)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    if args.action == "run":
        run_set(args)
    print(report(args.out_dir), end="")


if __name__ == "__main__":
    main()
