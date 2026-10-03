"""`ccm-outline` istemcisi: eşleşmelerin türünü ve kapsayan tanımı CCM'in grammar'larıyla bulur."""

from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path

from freshness.jsonio import as_int, as_list, as_object, as_str
from freshness.model import JsonValue
from search.compact import Kind, Match, Structure, match_column

CRATE_DIR: Path = Path(__file__).parent / "ccm-outline"
OUTLINE_BIN: Path = CRATE_DIR / "target" / "release" / "ccm-outline"
OUTLINE_TIMEOUT_S: int = 30
KINDS: dict[str, Kind] = {
    "def": "def",
    "code": "code",
    "import": "import",
    "comment": "comment",
    "string": "string",
}


class OutlineError(RuntimeError):
    """Ayrıştırıcı eksik, başarısız ya da yanıtı geçersiz olduğunda yükseltilir."""


def _optional_str(value: JsonValue, where: str) -> str | None:
    return None if value is None else as_str(value, where, OutlineError)


def classify(cwd: Path, matches: list[Match], patterns: list[re.Pattern[str]]) -> list[Structure]:
    """Her eşleşme için tür ve kapsam; dili tanınmayan dosyalar `text` olur."""
    if not OUTLINE_BIN.is_file():
        raise OutlineError(
            f"{OUTLINE_BIN} is missing; run: cargo build --release --offline "
            f"--manifest-path {CRATE_DIR / 'Cargo.toml'}"
        )
    points: dict[str, list[list[int]]] = {}
    for match in matches:
        absolute = str((cwd / match.path).resolve())
        points.setdefault(absolute, []).append([match.line, match_column(match.text, patterns)])
    request = "\n".join(
        json.dumps({"path": path, "points": value}) for path, value in points.items()
    )
    completed = subprocess.run(
        [str(OUTLINE_BIN)],
        input=request,
        capture_output=True,
        text=True,
        timeout=OUTLINE_TIMEOUT_S,
        check=False,
    )
    if completed.returncode != 0:
        raise OutlineError(f"ccm-outline exited {completed.returncode}: {completed.stderr[:300]}")
    found: dict[tuple[str, int], Structure] = {}
    for number, line in enumerate(completed.stdout.splitlines(), 1):
        response = as_object(json.loads(line), f"ccm-outline line {number}", OutlineError)
        path = as_str(response.get("path"), "path", OutlineError)
        error = _optional_str(response.get("error"), "error")
        if error is not None:
            raise OutlineError(f"ccm-outline could not classify {path}: {error}")
        for item in as_list(response.get("results"), "results", OutlineError):
            result = as_object(item, "result", OutlineError)
            kind_text = as_str(result.get("kind"), "kind", OutlineError)
            if kind_text not in KINDS:
                raise OutlineError(f"ccm-outline returned unknown kind {kind_text!r} for {path}")
            key = (path, as_int(result.get("line"), "line", OutlineError))
            found[key] = Structure(
                kind=KINDS[kind_text],
                scope=_optional_str(result.get("scope"), "scope"),
                name=_optional_str(result.get("name"), "name"),
            )
    plain = Structure(kind="text", scope=None, name=None)
    return [found.get((str((cwd / m.path).resolve()), m.line), plain) for m in matches]
