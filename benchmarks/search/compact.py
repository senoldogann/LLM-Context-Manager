"""Arama eşleşmelerini biçimler: CCM kompakt, tam ve yapısız taban.

Bütçe RTK'nın varsayılan sınırlarıyla aynıdır (dosya başına 25, toplam 200 konum taşıyan çıktı
satırı) ve RTK'nın yok saydığı dizin/dosyalar burada da yok sayılır. Bir kompakt satır aynı
kapsamdaki birçok konumu satır numarası olarak taşır.
"""

from __future__ import annotations

import fnmatch
import re
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path
from typing import Literal

Kind = Literal["def", "code", "import", "comment", "string", "text"]

IGNORED_DIRS: frozenset[str] = frozenset(
    {".git", "node_modules", "target", "__pycache__", ".venv", "vendor"}
)
IGNORED_FILES: tuple[str, ...] = ("*.lock", "*.min.js", "*.min.css")
PER_FILE_LINES: int = 25
TOTAL_LINES: int = 200
NUMBERS_PER_LINE: int = 40
TEXT_WIDTH: int = 100


@dataclass(frozen=True)
class Match:
    path: str
    line: int
    text: str


@dataclass(frozen=True)
class Structure:
    kind: Kind
    scope: str | None
    name: str | None


@dataclass(frozen=True)
class Rendered:
    text: str
    visible: int


Items = list[tuple[Match, Structure]]
Entries = list[tuple[str, int]]


def parse_null_output(output: bytes) -> list[Match]:
    """`yol\\0satır:metin` biçimli satırları ayrıştırır; uymayan satırları atlar."""
    matches: list[Match] = []
    for raw_line in output.split(b"\n"):
        if b"\0" not in raw_line:
            continue
        raw_path, rest = raw_line.split(b"\0", 1)
        found = re.match(rb"(\d+):(.*)", rest, re.S)
        if found is None:
            continue
        matches.append(
            Match(
                path=raw_path.decode("utf-8", errors="replace"),
                line=int(found.group(1)),
                text=found.group(2).decode("utf-8", errors="replace"),
            )
        )
    return matches


def is_ignored(path: str) -> bool:
    parts = Path(path).parts
    if any(part in IGNORED_DIRS for part in parts):
        return True
    name = parts[-1] if parts else path
    return any(fnmatch.fnmatch(name, pattern) for pattern in IGNORED_FILES)


def match_column(text: str, patterns: list[re.Pattern[str]]) -> int:
    """İlk desen eşleşmesinin bayt sütunu; bulunamazsa ilk boşluk olmayan karakter."""
    for pattern in patterns:
        found = pattern.search(text)
        if found is not None:
            return len(text[: found.start()].encode("utf-8"))
    indent = len(text) - len(text.lstrip())
    return len(text[:indent].encode("utf-8"))


def trim(text: str, width: int) -> str:
    clean = text.strip()
    return clean if len(clean) <= width else clean[: width - 1] + "…"


def _group_by_file(matches: list[Match], structures: list[Structure]) -> dict[str, Items]:
    files: dict[str, Items] = {}
    for match, structure in zip(matches, structures, strict=True):
        if is_ignored(match.path):
            continue
        files.setdefault(match.path, []).append((match, structure))
    return files


def _header(files: dict[str, Items]) -> str:
    total = sum(len(items) for items in files.values())
    counts: dict[str, int] = {}
    for items in files.values():
        for _, structure in items:
            key = "doc" if structure.kind in ("comment", "string") else structure.kind
            counts[key] = counts.get(key, 0) + 1
    order = ("def", "code", "import", "doc", "text")
    summary = " · ".join(f"{key} {counts[key]}" for key in order if key in counts)
    return f"{total} matches in {len(files)} files ({summary})"


def _definition_line(match: Match, structure: Structure) -> str:
    qualified = f"{structure.scope}.{structure.name}" if structure.scope else structure.name
    return f"  def {qualified} :{match.line} {trim(match.text, TEXT_WIDTH)}"


def _number_line(label: str, numbers: list[int]) -> tuple[str, int]:
    shown = numbers[:NUMBERS_PER_LINE]
    text = " ".join(str(number) for number in shown)
    if len(numbers) > len(shown):
        text += f" +{len(numbers) - len(shown)} more"
    return f"  {label}: {text}", len(shown)


def _compact_entries(items: Items) -> Entries:
    """Tanımlar imzasıyla; diğer konumlar kapsam altında yalnız satır numarası olarak."""
    entries: Entries = []
    scopes: dict[str, list[int]] = {}
    imports: list[int] = []
    docs: list[int] = []
    for match, structure in items:
        if structure.kind == "def":
            entries.append((_definition_line(match, structure), 1))
        elif structure.kind == "import":
            imports.append(match.line)
        elif structure.kind in ("comment", "string"):
            docs.append(match.line)
        else:
            scopes.setdefault(structure.scope or "<top>", []).append(match.line)
    groups = [*scopes.items(), ("import", imports), ("doc", docs)]
    entries.extend(_number_line(label, numbers) for label, numbers in groups if numbers)
    return entries


def _full_entries(items: Items) -> Entries:
    """Kapsam başlığı (konum taşımaz) altında metinli eşleşme satırları."""
    entries: Entries = []
    scopes: dict[str, list[Match]] = {}
    imports: list[Match] = []
    docs: list[int] = []
    for match, structure in items:
        if structure.kind == "def":
            entries.append((_definition_line(match, structure), 1))
        elif structure.kind == "import":
            imports.append(match)
        elif structure.kind in ("comment", "string"):
            docs.append(match.line)
        else:
            scopes.setdefault(structure.scope or "<top>", []).append(match)
    for scope, group in scopes.items():
        entries.append((f"  {scope}:", 0))
        entries.extend((f"    {m.line}: {trim(m.text, TEXT_WIDTH)}", 1) for m in group)
    entries.extend((f"  import {m.line}: {trim(m.text, TEXT_WIDTH)}", 1) for m in imports)
    if docs:
        entries.append(_number_line("doc", docs))
    return entries


def _text_entries(items: Items) -> Entries:
    """Dili tanınmayan dosya: dosya başına gruplanmış, kırpılmış metinli satırlar."""
    return [(f"  {match.line}: {trim(match.text, TEXT_WIDTH)}", 1) for match, _ in items]


def _render_with_budget(
    files: dict[str, Items], code_entries: Callable[[Items], Entries]
) -> Rendered:
    """RTK ile aynı bütçe: dosya başına 25, toplam 200 çıktı satırı."""
    lines: list[str] = [_header(files)]
    visible = 0
    remaining = TOTAL_LINES
    hidden_files = 0
    hidden_matches = 0
    for path, items in files.items():
        if remaining <= 0:
            hidden_files += 1
            hidden_matches += len(items)
            continue
        lines.append(path)
        plain_file = all(structure.kind == "text" for _, structure in items)
        entries = _text_entries(items) if plain_file else code_entries(items)
        file_lines = 0
        file_visible = 0
        for text, locations in entries:
            if file_lines >= PER_FILE_LINES or remaining <= 0:
                break
            lines.append(text)
            if locations > 0:
                file_lines += 1
                remaining -= 1
                file_visible += locations
        if file_visible < len(items):
            lines.append(f"  +{len(items) - file_visible} more")
        visible += file_visible
    if hidden_files > 0:
        lines.append(f"+{hidden_files} more files ({hidden_matches} matches)")
    return Rendered(text="\n".join(lines) + "\n", visible=visible)


def render_compact(matches: list[Match], structures: list[Structure]) -> Rendered:
    return _render_with_budget(_group_by_file(matches, structures), _compact_entries)


def render_full(matches: list[Match], structures: list[Structure]) -> Rendered:
    return _render_with_budget(_group_by_file(matches, structures), _full_entries)


def render_plain(matches: list[Match]) -> Rendered:
    """Yapısız taban: aynı gruplama, kırpma ve bütçe; tree-sitter kullanılmaz."""
    plain = [Structure(kind="text", scope=None, name=None) for _ in matches]
    return _render_with_budget(_group_by_file(matches, plain), _text_entries)


def rtk_visible(output: bytes) -> int:
    """RTK çıktısında gösterilen eşleşme satırı sayısı (başlık ve gizleme notları hariç)."""
    count = 0
    for line in output.decode("utf-8", errors="replace").splitlines():
        clean = line.strip()
        if not clean or clean.startswith("+") or "rtk recall" in clean:
            continue
        if re.match(r"^\d+ matches in \d+ files", clean):
            continue
        count += 1
    return count
