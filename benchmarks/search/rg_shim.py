"""Ajan için `rg` sarmalayıcısı: satır biçimli aramaların çıktısını CCM kompakt biçimine çevirir.

Pilotun S kolunda PATH'in başına konan küçük bir betik bunu çalıştırır; gerçek `rg` yolu
`CCM_REAL_RG` ortam değişkeninden gelir. Liste, sayım ve bağlam gibi satır biçimli olmayan
çağrılar, stdin'den okuyan çağrılar ve bir boru hattının parçası olan çağrılar gerçek `rg`'ye
aynen devredilir: aşağı akıştaki `wc` ya da `head` gibi araçlar rg biçimini bekler. Kompakt çıktı
ham çıktıdan küçük değilse ham çıktı gösterilir. Biçimleme hatası stderr'de görünür bir notla
bildirilir ve ham çıktı gösterilir. `CCM_SHIM_LOG` verilirse her çağrının kipi ve bayt boyutları
JSON satırı olarak eklenir; içerik yazılmaz.
"""

from __future__ import annotations

import ctypes
import json
import os
import shlex
import stat
import subprocess
import sys
from pathlib import Path

from search.command import (
    RG_STRUCTURED_FLAGS,
    Command,
    is_line_shaped,
    operands,
    pattern_regexes,
)
from search.compact import parse_null_output, render_compact
from search.outline import OutlineError, classify

SHELLS: frozenset[str] = frozenset({"sh", "bash", "zsh", "dash"})
PIPE_OPERATORS: frozenset[str] = frozenset({"|", "|&"})
CTL_KERN: int = 1
KERN_PROCARGS2: int = 49


def reads_stdin(command: Command) -> bool:
    """Yol verilmemişse ve stdin bir boru ya da dosyaysa rg stdin'i arar."""
    if operands(command).paths:
        return False
    mode = os.fstat(0).st_mode
    return stat.S_ISFIFO(mode) or stat.S_ISREG(mode)


def parent_argv() -> list[str] | None:
    """Üst sürecin argv'si (macOS `sysctl KERN_PROCARGS2`); okunamazsa None.

    Codex kum havuzu setuid `/bin/ps`'i çalıştırmaya izin vermez; sysctl aynı kum havuzundaki üst
    süreç için izinlidir.
    """
    libc = ctypes.CDLL(None, use_errno=True)
    mib = (ctypes.c_int * 3)(CTL_KERN, KERN_PROCARGS2, os.getppid())
    size = ctypes.c_size_t(0)
    if libc.sysctl(mib, 3, None, ctypes.byref(size), None, 0) != 0:
        return None
    buffer = ctypes.create_string_buffer(size.value)
    if libc.sysctl(mib, 3, buffer, ctypes.byref(size), None, 0) != 0:
        return None
    raw = buffer.raw[: size.value]
    count = int.from_bytes(raw[:4], "little")
    _, _, rest = raw[4:].partition(b"\0")
    values = rest.lstrip(b"\0").split(b"\0")[:count]
    return [value.decode("utf-8", errors="replace") for value in values]


def same_stream(first: int, second: int) -> bool:
    left, right = os.fstat(first), os.fstat(second)
    return (left.st_dev, left.st_ino) == (right.st_dev, right.st_ino)


def in_pipeline() -> bool:
    """Bu çağrının çıktısı bir boru hattında başka bir araca gidiyor olabilir mi?

    Üst süreç kabuk değilse kabuk rg'ye `exec` etmiştir ve boru yoktur. Kabuğun `-c` betiğinde
    boru işleci (`|`, `|&`) yoksa boru yoktur. Boru varken stdout stderr ile aynı akışsa bu çağrı
    borunun dışındadır (karışık betikler). Üst süreç ya da betik okunamazsa çağrı devredilir.
    """
    argv = parent_argv()
    if argv is None:
        return True
    if not argv or Path(argv[0]).name.lstrip("-") not in SHELLS:
        return False
    script = next(
        (argv[i + 1] for i, part in enumerate(argv[:-1]) if part.startswith("-") and "c" in part),
        None,
    )
    if script is None:
        return True
    try:
        lexer = shlex.shlex(script, posix=True, punctuation_chars=True)
        lexer.whitespace_split = True
        parts = list(lexer)
    except ValueError:
        return True
    if not any(part in PIPE_OPERATORS for part in parts):
        return False
    return not same_stream(1, 2)


def delegation_reason(command: Command) -> str | None:
    if not is_line_shaped(command):
        return "delegate-shape"
    if reads_stdin(command):
        return "delegate-stdin"
    if in_pipeline():
        return "delegate-pipeline"
    return None


def log_event(log: str | None, mode: str, raw_bytes: int, output_bytes: int) -> None:
    if log is None:
        return
    with Path(log).open("a") as handle:
        handle.write(
            json.dumps({"mode": mode, "raw_bytes": raw_bytes, "output_bytes": output_bytes}) + "\n"
        )


def main() -> int:
    real_rg = os.environ.get("CCM_REAL_RG")
    if real_rg is None:
        sys.stderr.write("[ccm] CCM_REAL_RG is not set; the rg wrapper cannot run\n")
        return 2
    log = os.environ.get("CCM_SHIM_LOG")
    args = sys.argv[1:]
    command = Command(argv=("rg", *args), program="rg")
    reason = delegation_reason(command)
    if reason is not None:
        log_event(log, reason, 0, 0)
        os.execv(real_rg, [real_rg, *args])
    raw = subprocess.run([real_rg, *args], capture_output=True, check=False)
    if raw.returncode != 0 or not raw.stdout:
        log_event(log, "no-match", len(raw.stdout), len(raw.stdout))
        sys.stdout.buffer.write(raw.stdout)
        sys.stderr.buffer.write(raw.stderr)
        return raw.returncode
    structured = subprocess.run(
        [real_rg, *RG_STRUCTURED_FLAGS, *args], capture_output=True, check=False
    )
    matches = parse_null_output(structured.stdout)
    try:
        if not matches:
            raise OutlineError("no parsable matches in structured rg output")
        structures = classify(Path.cwd(), matches, pattern_regexes(command))
    except (OutlineError, subprocess.TimeoutExpired) as error:
        log_event(log, "error", len(raw.stdout), len(raw.stdout))
        sys.stdout.buffer.write(raw.stdout)
        sys.stderr.write(f"[ccm] structure unavailable, raw rg output shown: {error}\n")
        return 0
    compact = render_compact(matches, structures).text.encode("utf-8")
    smaller = len(compact) < len(raw.stdout)
    chosen = compact if smaller else raw.stdout
    log_event(log, "compact" if smaller else "raw-smaller", len(raw.stdout), len(chosen))
    sys.stdout.buffer.write(chosen)
    sys.stderr.buffer.write(raw.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
