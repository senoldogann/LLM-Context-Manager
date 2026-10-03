"""Gerçek arama geçmişini tekrar oynatır: ham, RTK, yapısız taban ve CCM (tam, kompakt) çıktıları.

RTK'nın `history.db` kaydındaki düz rg/grep komutları, kaydedildikleri proje dizininde yerelde ve
yalnız okuma yaparak yeniden çalıştırılır. Gizlilik: komut metni, yol ya da çıktı içeriği yazılmaz
ve kaydedilmez; yalnız sayılar. Token sayısı `o200k_base` ile ölçülür; kodlama dosyası önceden
indirilmiş olmalıdır. RTK, kullanıcının takip ve telemetri kaydını değiştirmesin diye takip ve
telemetrisi kapalı ayrı bir HOME ile çalışır.

Kullanım:
  uv run python -m search.replay --rtk-db PATH --rtk-home DIR --tiktoken-cache DIR \\
    --sample 1500 --seed 20261003 --timeout-s 15 --workers 4 --out DIR
"""

from __future__ import annotations

import argparse
import json
import os
import random
import sqlite3
import statistics
import subprocess
from collections import Counter
from collections.abc import Callable
from concurrent.futures import ThreadPoolExecutor
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Literal

import tiktoken

from search.command import (
    Command,
    is_line_shaped,
    parse_simple,
    pattern_regexes,
    structured_argv,
)
from search.compact import (
    parse_null_output,
    render_compact,
    render_full,
    render_plain,
    rtk_visible,
)
from search.outline import OutlineError, classify

Shape = Literal["line", "other"]
MAX_RAW_BYTES: int = 5_000_000


@dataclass(frozen=True)
class Candidate:
    text: str
    project: str
    weight: int


@dataclass(frozen=True)
class Measurement:
    shape: Shape
    weight: int
    rtk_unchanged: bool
    raw_bytes: int
    rtk_bytes: int
    plain_bytes: int
    full_bytes: int
    compact_bytes: int
    passthrough_bytes: int
    raw_tokens: int
    rtk_tokens: int
    plain_tokens: int
    full_tokens: int
    compact_tokens: int
    passthrough_tokens: int
    total: int
    rtk_visible: int
    plain_visible: int
    full_visible: int
    compact_visible: int
    kinds: dict[str, int]


@dataclass(frozen=True)
class Context:
    rtk_env: dict[str, str]
    timeout_s: int
    count: Callable[[bytes], int]


def load_candidates(db: Path) -> tuple[int, list[Candidate]]:
    """Arama komutları ve kaç kez çalıştırıldıkları (aynı proje dizininde)."""
    connection = sqlite3.connect(f"file:{db}?mode=ro", uri=True)
    try:
        rows = connection.execute(
            "SELECT original_cmd, project_path FROM commands "
            "WHERE original_cmd LIKE 'rg %' OR original_cmd LIKE 'grep %'"
        ).fetchall()
    finally:
        connection.close()
    counts: Counter[tuple[str, str]] = Counter((str(text), str(project)) for text, project in rows)
    return len(rows), [Candidate(t, p, n) for (t, p), n in counts.items()]


def run(
    argv: list[str], cwd: Path, env: dict[str, str], timeout_s: int
) -> subprocess.CompletedProcess[bytes]:
    return subprocess.run(
        argv,
        cwd=cwd,
        env=env,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        timeout=timeout_s,
        check=False,
    )


def measure(candidate: Candidate, command: Command, context: Context) -> Measurement | str:
    """Tek komutu ölçer; ölçülemezse nedenini (sayım için kısa bir etiket) döndürür."""
    cwd = Path(candidate.project)
    env = dict(os.environ)
    count = context.count
    try:
        raw = run(list(command.argv), cwd, env, context.timeout_s)
        if raw.returncode != 0 or not raw.stdout.strip():
            return "no-match-or-error"
        if len(raw.stdout) > MAX_RAW_BYTES:
            return "too-large"
        rtk = run(["rtk", *command.argv], cwd, context.rtk_env, context.timeout_s)
        rtk_tokens = count(rtk.stdout)
        unchanged = rtk.stdout == raw.stdout
        if not is_line_shaped(command):
            seen = rtk_visible(rtk.stdout)
            return Measurement(
                shape="other", weight=candidate.weight, rtk_unchanged=unchanged,
                raw_bytes=len(raw.stdout), rtk_bytes=len(rtk.stdout), plain_bytes=len(rtk.stdout),
                full_bytes=len(rtk.stdout), compact_bytes=len(rtk.stdout),
                passthrough_bytes=len(rtk.stdout), raw_tokens=count(raw.stdout),
                rtk_tokens=rtk_tokens, plain_tokens=rtk_tokens, full_tokens=rtk_tokens,
                compact_tokens=rtk_tokens, passthrough_tokens=rtk_tokens, total=seen,
                rtk_visible=seen, plain_visible=seen, full_visible=seen, compact_visible=seen,
                kinds={},
            )  # fmt: skip
        structured = run(structured_argv(command), cwd, env, context.timeout_s)
        matches = parse_null_output(structured.stdout)
        if not matches:
            return "structured-output-empty"
        structures = classify(cwd, matches, pattern_regexes(command))
    except subprocess.TimeoutExpired:
        return "timeout"
    except OutlineError:
        return "outline-error"
    except OSError:
        return "os-error"
    plain = render_plain(matches)
    full = render_full(matches, structures)
    compact = render_compact(matches, structures)
    compact_text = compact.text.encode("utf-8")
    raw_tokens = count(raw.stdout)
    compact_tokens = count(compact_text)
    keep_raw = compact_tokens >= raw_tokens
    kinds = Counter("doc" if s.kind in ("comment", "string") else s.kind for s in structures)
    return Measurement(
        shape="line", weight=candidate.weight, rtk_unchanged=unchanged,
        raw_bytes=len(raw.stdout), rtk_bytes=len(rtk.stdout),
        plain_bytes=len(plain.text.encode("utf-8")), full_bytes=len(full.text.encode("utf-8")),
        compact_bytes=len(compact_text),
        passthrough_bytes=len(raw.stdout) if keep_raw else len(compact_text),
        raw_tokens=raw_tokens, rtk_tokens=rtk_tokens, plain_tokens=count(plain.text.encode()),
        full_tokens=count(full.text.encode()), compact_tokens=compact_tokens,
        passthrough_tokens=raw_tokens if keep_raw else compact_tokens, total=len(matches),
        rtk_visible=rtk_visible(rtk.stdout), plain_visible=plain.visible,
        full_visible=full.visible, compact_visible=compact.visible, kinds=dict(kinds),
    )  # fmt: skip


def percentile(values: list[float], fraction: float) -> float:
    ordered = sorted(values)
    return ordered[min(len(ordered) - 1, int(fraction * (len(ordered) - 1)))]


def bucket(tokens: int) -> str:
    if tokens < 250:
        return "a) <250 tokens"
    if tokens < 2_500:
        return "b) 250-2,500"
    if tokens < 25_000:
        return "c) 2,500-25,000"
    return "d) >=25,000"


TOTAL_FIELDS: tuple[str, ...] = (
    "raw", "rtk", "plain", "full", "compact", "passthrough",
)  # fmt: skip


def totals(group: list[Measurement], weighted: bool, unit: str) -> dict[str, int]:
    return {
        name: sum(int(getattr(m, f"{name}_{unit}")) * (m.weight if weighted else 1) for m in group)
        for name in TOTAL_FIELDS
    }


def ratio_line(label: str, values: list[float]) -> str:
    fractions = (0.1, 0.25, 0.5, 0.75, 0.9)
    return f"  {label}: " + " ".join(f"{percentile(values, f):.2f}" for f in fractions)


def summarize(recorded: int, funnel: Counter[str], measured: list[Measurement]) -> str:
    """Toplu özet; yalnız sayılar."""
    lines = [f"search commands in RTK history: {recorded}"]
    lines += [
        f"  {key}: {funnel[key]}"
        for key in ("unique", "project-exists", "simple", "sampled", "measured")
    ]
    lines += [
        f"  skipped {key[7:]}: {n}"
        for key, n in sorted(funnel.items())
        if key.startswith("reason:")
    ]
    line = [m for m in measured if m.shape == "line"]
    lines.append(f"shape: line={len(line)} other={len(measured) - len(line)}")
    for label, group in (("line-shaped", line), ("all measured", measured)):
        for weighted in (False, True):
            t = totals(group, weighted, "tokens")
            b = totals(group, weighted, "bytes")
            if t["rtk"] == 0:
                continue
            lines.append(
                f"\n{label} ({'weighted by history count' if weighted else 'unique commands'})"
            )
            lines.append("  tokens " + " ".join(f"{k}={v:,}" for k, v in t.items()))
            lines.append("  bytes  " + " ".join(f"{k}={v:,}" for k, v in b.items()))
            lines.append(
                "  token ratio vs RTK: "
                + " ".join(f"{k}={t[k] / t['rtk']:.3f}" for k in TOTAL_FIELDS if k != "rtk")
            )
            seen = {
                "total": sum(m.total for m in group),
                "rtk": sum(m.rtk_visible for m in group),
                "plain": sum(m.plain_visible for m in group),
                "full": sum(m.full_visible for m in group),
                "compact": sum(m.compact_visible for m in group),
            }
            lines.append("  visible locations: " + " ".join(f"{k}={v:,}" for k, v in seen.items()))
    usable = [m for m in line if m.rtk_tokens > 0 and m.plain_tokens > 0]
    if usable:
        unchanged = sum(m.rtk_unchanged for m in line) / len(line)
        lines.append(f"\nRTK left the output unchanged: {unchanged:.1%} of line-shaped commands")
        lines.append("per-command token ratio, percentiles p10 p25 p50 p75 p90:")
        lines.append(ratio_line("compact/RTK", [m.compact_tokens / m.rtk_tokens for m in usable]))
        lines.append(
            ratio_line("passthrough/RTK", [m.passthrough_tokens / m.rtk_tokens for m in usable])
        )
        lines.append(ratio_line("plain/RTK", [m.plain_tokens / m.rtk_tokens for m in usable]))
        lines.append(ratio_line("full/RTK", [m.full_tokens / m.rtk_tokens for m in usable]))
        lines.append(
            ratio_line("compact/plain", [m.compact_tokens / m.plain_tokens for m in usable])
        )
        passthrough = [m.passthrough_tokens / m.rtk_tokens for m in usable]
        below = sum(r <= 0.7 for r in passthrough) / len(passthrough)
        above = sum(r > 1.0 for r in passthrough) / len(passthrough)
        fewer = sum(m.compact_visible < m.rtk_visible for m in line)
        lines.append(f"  passthrough at least 30% below RTK: {below:.1%}")
        lines.append(f"  passthrough above RTK: {above:.1%}")
        lines.append(f"  commands where compact shows fewer locations than RTK: {fewer}")
    kinds: Counter[str] = Counter()
    for m in line:
        kinds.update(m.kinds)
    if kinds:
        whole = sum(kinds.values())
        lines.append(
            "\nline-shaped matches by kind: "
            + ", ".join(f"{k} {v / whole:.1%}" for k, v in kinds.most_common())
        )
    lines.append("\nby raw size (line-shaped, tokens):")
    lines.append("  bucket: count, median passthrough/RTK, median plain/RTK, total passthrough/RTK")
    for name in sorted({bucket(m.raw_tokens) for m in usable}):
        group = [m for m in usable if bucket(m.raw_tokens) == name]
        median_passthrough = statistics.median(m.passthrough_tokens / m.rtk_tokens for m in group)
        median_plain = statistics.median(m.plain_tokens / m.rtk_tokens for m in group)
        share = sum(m.passthrough_tokens for m in group) / sum(m.rtk_tokens for m in group)
        lines.append(
            f"  {name}: {len(group)}, {median_passthrough:.2f}, {median_plain:.2f}, {share:.2f}"
        )
    return "\n".join(lines) + "\n"


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--rtk-db", type=Path, required=True)
    parser.add_argument("--rtk-home", type=Path, required=True)
    parser.add_argument("--tiktoken-cache", type=Path, required=True)
    parser.add_argument("--sample", type=int, required=True)
    parser.add_argument("--seed", type=int, required=True)
    parser.add_argument("--timeout-s", type=int, required=True)
    parser.add_argument("--workers", type=int, required=True)
    parser.add_argument("--out", type=Path, required=True)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    cache: Path = args.tiktoken_cache
    if not cache.is_dir() or not any(cache.iterdir()):
        raise SystemExit(f"tiktoken cache {cache} is empty; download o200k_base there first")
    os.environ["TIKTOKEN_CACHE_DIR"] = str(cache)
    encoding = tiktoken.get_encoding("o200k_base")
    rtk_config = args.rtk_home / "Library" / "Application Support" / "rtk" / "config.toml"
    if not rtk_config.is_file():
        raise SystemExit(
            f"{rtk_config} is missing; copy the RTK config with tracking and telemetry off"
        )

    def count(data: bytes) -> int:
        return len(encoding.encode(data.decode("utf-8", errors="replace"), disallowed_special=()))

    recorded, candidates = load_candidates(args.rtk_db)
    funnel: Counter[str] = Counter({"unique": len(candidates)})
    eligible: list[tuple[Candidate, Command]] = []
    for candidate in candidates:
        if not Path(candidate.project).is_dir():
            continue
        funnel["project-exists"] += 1
        command = parse_simple(candidate.text)
        if command is not None:
            eligible.append((candidate, command))
    funnel["simple"] = len(eligible)
    sample = random.Random(args.seed).sample(eligible, min(args.sample, len(eligible)))
    funnel["sampled"] = len(sample)
    context = Context(
        rtk_env={**os.environ, "HOME": str(args.rtk_home)}, timeout_s=args.timeout_s, count=count
    )
    measured: list[Measurement] = []
    with ThreadPoolExecutor(max_workers=args.workers) as pool:
        for outcome in pool.map(lambda pair: measure(pair[0], pair[1], context), sample):
            if isinstance(outcome, Measurement):
                measured.append(outcome)
            else:
                funnel[f"reason:{outcome}"] += 1
    funnel["measured"] = len(measured)
    args.out.mkdir(parents=True, exist_ok=True)
    (args.out / "measurements.json").write_text(json.dumps([asdict(m) for m in measured], indent=1))
    summary = summarize(recorded, funnel, measured)
    (args.out / "summary.txt").write_text(summary)
    print(summary, end="")


if __name__ == "__main__":
    main()
