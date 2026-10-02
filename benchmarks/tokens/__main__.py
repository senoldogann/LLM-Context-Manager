"""Token kıyası CLI'ı: `run` bir sürümü ölçer, `report` sonuçları tabloya döker.

Ön-kayıt: `tokens/PREREGISTRATION.md`. Kullanım:
    python -m tokens run --version v1 --bin-dir … --corpus-dir corpus \
        --questions tokens/questions.json --skill ../SKILL.md --home … --out …
    python -m tokens report --results a.json --results b.json --out report.md
"""

from __future__ import annotations

import argparse
import json
import logging
import statistics
import subprocess
import sys
from datetime import UTC, datetime
from pathlib import Path

from freshness.ccm import PROTOCOL_VERSION
from freshness.jsonio import as_int, as_list, as_object, as_str
from freshness.logs import log_event
from freshness.mcp import McpClient, ServerSpec
from freshness.model import JsonValue
from tokens.model import (
    SCHEMA_VERSION,
    QuestionResult,
    RepoResult,
    RunResult,
    TokenBenchError,
    read_run,
    write_run,
)
from tokens.plans import (
    PlanSession,
    impact,
    overview,
    v1_callers,
    v1_explain,
    v2_targeted,
)

LOGGER = logging.getLogger("tokens")
PACKAGE_DIR = Path(__file__).resolve().parent
REQUEST_TIMEOUT_S = 60.0
VERSIONS = ("v1", "v2")


def harness_revision() -> str:
    """Harness commit'i; `benchmarks/tokens` kirliyse ölçüm başlamaz."""
    status = subprocess.run(
        ["git", "status", "--porcelain", "--", str(PACKAGE_DIR)],
        capture_output=True,
        text=True,
        check=False,
    )
    if status.returncode != 0 or status.stdout.strip():
        raise TokenBenchError(f"commit benchmarks/tokens before running:\n{status.stdout}")
    head = subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True, text=True, check=False)
    if head.returncode != 0:
        raise TokenBenchError(f"git rev-parse HEAD failed: {head.stderr.strip()}")
    return head.stdout.strip()


def load_questions(path: Path) -> tuple[int, dict[str, tuple[tuple[str, ...], tuple[str, ...]]]]:
    """Soru dosyası: en çok eşleşme sayısı ve repo → (semboller, dosyalar)."""
    loaded: JsonValue = json.loads(path.read_text(encoding="utf-8"))
    data = as_object(loaded, "questions", TokenBenchError)
    max_matches = as_int(data.get("max_matches"), "max_matches", TokenBenchError)
    repos: dict[str, tuple[tuple[str, ...], tuple[str, ...]]] = {}
    for name, value in as_object(data.get("repos"), "repos", TokenBenchError).items():
        repo = as_object(value, f"repos.{name}", TokenBenchError)
        symbols = tuple(
            as_str(item, "symbol", TokenBenchError)
            for item in as_list(repo.get("symbols"), "symbols", TokenBenchError)
        )
        files = tuple(
            as_str(item, "file", TokenBenchError)
            for item in as_list(repo.get("files"), "files", TokenBenchError)
        )
        repos[name] = (symbols, files)
    return max_matches, repos


def server_env(repo: Path, home: Path) -> dict[str, str]:
    """Sunucunun ortamı: yalnız PATH, yalıtılmış HOME ve CCM ayarları."""
    return {
        "PATH": "/usr/bin:/bin",
        "HOME": str(home),
        "CCM_DISABLE_EMBEDDER": "1",
        "CCM_PROJECT_ROOT": str(repo),
        "CCM_ALLOWED_ROOTS": str(repo),
        "CCM_AUTO_REFRESH": "0",
        "CCM_MCP_DEBUG": "0",
    }


def index_repo(bin_dir: Path, repo: Path, home: Path) -> None:
    """`ccm-cli index` ile indeksi kurar; hata açıkça yükselir."""
    env = {"PATH": "/usr/bin:/bin", "HOME": str(home), "CCM_DISABLE_EMBEDDER": "1"}
    completed = subprocess.run(
        [str(bin_dir / "ccm-cli"), "index", "--path", str(repo)],
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    if completed.returncode != 0:
        raise TokenBenchError(
            f"ccm-cli index {repo} exited {completed.returncode}: {completed.stderr[-1500:]}"
        )


def measure_repo(
    version: str,
    name: str,
    repo: Path,
    symbols: tuple[str, ...],
    files: tuple[str, ...],
    max_matches: int,
    bin_dir: Path,
    home: Path,
    log_dir: Path,
) -> RepoResult:
    """Bir repoda ön-kayıtlı soruları sürümün planıyla sorar."""
    index_repo(bin_dir, repo, home)
    client = McpClient(
        ServerSpec(
            argv=(str(bin_dir / "ccm-mcp"),),
            cwd=repo,
            env=tuple(sorted(server_env(repo, home).items())),
            roots=(repo,),
            stderr_log=log_dir / f"{version}-{name}.stderr.log",
        )
    )
    try:
        client.initialize(PROTOCOL_VERSION, REQUEST_TIMEOUT_S)
        listed = client.list_tools(REQUEST_TIMEOUT_S)
        session = PlanSession(client, REQUEST_TIMEOUT_S)
        questions: list[QuestionResult] = []
        for symbol in symbols:
            if version == "v1":
                questions.append(v1_callers(session, symbol, max_matches))
                questions.append(v1_explain(session, symbol, max_matches))
            else:
                questions.append(
                    v2_targeted(session, "find_usages", "callers", symbol, max_matches)
                )
                questions.append(v2_targeted(session, "explain", "explain", symbol, max_matches))
            log_event(LOGGER, logging.INFO, "symbol_done", {"repo": name, "symbol": symbol})
        for file in files:
            questions.append(impact(session, file))
        if version == "v2":
            questions.append(overview(session))
        return RepoResult(name, len(listed.encode("utf-8")), tuple(questions))
    finally:
        client.close(10.0)


def command_run(namespace: argparse.Namespace) -> None:
    """Bir sürümü tüm repolarda ölçer ve sonucu yazar."""
    revision = harness_revision()
    max_matches, repos = load_questions(namespace.questions)
    started = datetime.now(UTC).isoformat(timespec="seconds")
    results: list[RepoResult] = []
    for name, (symbols, files) in repos.items():
        repo = (namespace.corpus_dir / name).resolve()
        if not repo.is_dir():
            raise TokenBenchError(f"corpus repo missing: {repo}")
        results.append(
            measure_repo(
                namespace.version,
                name,
                repo,
                symbols,
                files,
                max_matches,
                namespace.bin_dir.resolve(),
                namespace.home.resolve(),
                namespace.log_dir.resolve(),
            )
        )
    run = RunResult(
        schema_version=SCHEMA_VERSION,
        version=namespace.version,
        harness_revision=revision,
        started_at=started,
        skill_bytes=len(namespace.skill.read_bytes()),
        repos=tuple(results),
    )
    write_run(namespace.out, run)


def kind_totals(run: RunResult, repo: str, kind: str) -> tuple[int, int, int, float]:
    """(soru, çağrı, bayt, soru başına medyan bayt) toplamları."""
    questions = [
        question
        for result in run.repos
        if result.repo == repo
        for question in result.questions
        if question.kind == kind
    ]
    sizes = [sum(call.response_bytes for call in question.calls) for question in questions]
    calls = sum(len(question.calls) for question in questions)
    median = statistics.median(sizes) if sizes else 0.0
    return len(questions), calls, sum(sizes), median


def parity(baseline: RunResult, candidate: RunResult, repo: str) -> tuple[int, int]:
    """v1 çağıran konumlarından v2 cevaplarında da görülenler: (görülen, toplam)."""
    seen = 0
    total = 0
    for base in (
        q for r in baseline.repos if r.repo == repo for q in r.questions if q.kind == "callers"
    ):
        match = [
            q
            for r in candidate.repos
            if r.repo == repo
            for q in r.questions
            if q.kind == "callers" and q.subject == base.subject
        ]
        candidate_locations = set(match[0].locations) if match else set()
        expected = set(base.locations)
        total += len(expected)
        seen += len(expected & candidate_locations)
    return seen, total


def render_report(runs: tuple[RunResult, ...]) -> str:
    """Bir ya da iki ölçümün Markdown tablosu."""
    lines = ["# Token cost of answers", ""]
    for run in runs:
        lines.append(f"- {run.version}: harness `{run.harness_revision[:7]}`, {run.started_at}")
    lines += ["", "Estimated tokens = bytes / 4.", "", "## Fixed overhead per session", ""]
    lines += ["| Version | tools/list bytes | SKILL.md bytes |", "|---|---|---|"]
    for run in runs:
        tools = statistics.median([repo.tools_list_bytes for repo in run.repos])
        lines.append(f"| {run.version} | {tools:.0f} | {run.skill_bytes} |")
    lines += ["", "## Per question kind", ""]
    lines += [
        "| Repo | Kind | Version | Questions | Calls | Bytes | Est. tokens | Median bytes |",
        "|---|---|---|---|---|---|---|---|",
    ]
    repos = sorted({repo.repo for run in runs for repo in run.repos})
    for repo in repos:
        for kind in ("callers", "explain", "impact", "map"):
            for run in runs:
                count, calls, size, median = kind_totals(run, repo, kind)
                if count:
                    cells = (
                        repo,
                        kind,
                        run.version,
                        count,
                        calls,
                        size,
                        size // 4,
                        f"{median:.0f}",
                    )
                    lines.append("| " + " | ".join(str(cell) for cell in cells) + " |")
    if len(runs) == 2:
        lines += ["", "## Parity of callers (v1 locations found in v2 answers)", ""]
        lines += ["| Repo | Found | v1 total | Share |", "|---|---|---|---|"]
        for repo in repos:
            seen, total = parity(runs[0], runs[1], repo)
            share = f"{seen / total:.0%}" if total else "n/a"
            lines.append(f"| {repo} | {seen} | {total} | {share} |")
    return "\n".join(lines) + "\n"


def command_report(namespace: argparse.Namespace) -> None:
    """Sonuç dosyalarından Markdown rapor."""
    runs = tuple(read_run(path) for path in namespace.results)
    if len(runs) not in (1, 2):
        raise TokenBenchError(f"report takes one or two results files, got {len(runs)}")
    namespace.out.write_text(render_report(runs), encoding="utf-8")


def build_parser() -> argparse.ArgumentParser:
    """Tüm argümanlar zorunlu; varsayılan yok."""
    parser = argparse.ArgumentParser(prog="python -m tokens")
    commands = parser.add_subparsers(dest="command", required=True)
    run = commands.add_parser("run")
    run.add_argument("--version", choices=VERSIONS, required=True)
    run.add_argument("--bin-dir", type=Path, required=True)
    run.add_argument("--corpus-dir", type=Path, required=True)
    run.add_argument("--questions", type=Path, required=True)
    run.add_argument("--skill", type=Path, required=True)
    run.add_argument("--home", type=Path, required=True)
    run.add_argument("--log-dir", type=Path, required=True)
    run.add_argument("--out", type=Path, required=True)
    report = commands.add_parser("report")
    report.add_argument("--results", type=Path, action="append", required=True)
    report.add_argument("--out", type=Path, required=True)
    return parser


def main() -> int:
    """Giriş noktası."""
    logging.basicConfig(level=logging.INFO, stream=sys.stderr, format="%(message)s")
    namespace = build_parser().parse_args()
    if namespace.command == "run":
        namespace.log_dir.mkdir(parents=True, exist_ok=True)
        command_run(namespace)
    else:
        command_report(namespace)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
