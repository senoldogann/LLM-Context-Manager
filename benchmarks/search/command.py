"""rg/grep komut satırlarını güvenli biçimde ayrıştırır; çıktı şeklini ve desenleri çıkarır.

Tekrar oynatma ve ajan sarmalayıcısı aynı kuralları kullanır.
"""

from __future__ import annotations

import re
import shlex
from dataclasses import dataclass
from typing import Literal

Program = Literal["rg", "grep"]

SHELL_OPERATOR_CHARS: frozenset[str] = frozenset("();<>|&")
# Harici program çalıştıran ya da sıkıştırılmış dosya açan seçenekler tekrar oynatılmaz.
FORBIDDEN_PREFIXES: tuple[str, ...] = ("--pre", "-z", "--search-zip", "--decompress")

# Çıktıyı satır-eşleşme biçiminden çıkaran seçenekler: liste, sayım, bağlam, ters eşleşme.
RG_SHAPE_LETTERS: frozenset[str] = frozenset("lcABCqv")
GREP_SHAPE_LETTERS: frozenset[str] = frozenset("lLcABCqv")
RG_SHAPE_LONG: frozenset[str] = frozenset(
    {
        "--files-with-matches", "--files-without-match", "--count", "--count-matches", "--files",
        "--json", "--after-context", "--before-context", "--context", "--quiet", "--type-list",
        "--stats", "--invert-match", "--passthru", "--passthrough", "--heading", "--vimgrep",
    }
)  # fmt: skip
GREP_SHAPE_LONG: frozenset[str] = frozenset(
    {
        "--files-with-matches", "--files-without-match", "--count", "--after-context",
        "--before-context", "--context", "--quiet", "--silent", "--invert-match", "--json",
        "--csv", "--xml", "--format", "--tree",
    }
)  # fmt: skip
# Değer alan seçenekler: değerleri desen ya da yol sanılmasın.
RG_VALUE_LETTERS: frozenset[str] = frozenset("efgtTmABCdMjEr")
GREP_VALUE_LETTERS: frozenset[str] = frozenset("efABCmdDgtKOM")
RG_VALUE_LONG: frozenset[str] = frozenset(
    {
        "--regexp", "--file", "--glob", "--iglob", "--type", "--type-not", "--max-count",
        "--after-context", "--before-context", "--context", "--max-depth", "--max-columns",
        "--sort", "--sortr", "--threads", "--type-add", "--type-clear", "--encoding",
        "--ignore-file", "--replace", "--context-separator", "--field-match-separator",
        "--field-context-separator", "--path-separator", "--max-filesize", "--pre", "--pre-glob",
        "--color", "--colors", "--engine", "--dfa-size-limit", "--regex-size-limit",
        "--hostname-bin", "--hyperlink-format",
    }
)  # fmt: skip
GREP_VALUE_LONG: frozenset[str] = frozenset(
    {
        "--regexp", "--file", "--after-context", "--before-context", "--context", "--max-count",
        "--include", "--exclude", "--exclude-dir", "--include-dir", "--color", "--colour",
        "--directories", "--devices", "--label", "--binary-files", "--glob", "--iglob",
        "--file-type", "--file-extension", "--file-magic", "--max-depth", "--format",
        "--group-separator", "--range", "--tabs", "--width",
    }
)  # fmt: skip
# Yapı bilgisi için aynı arama: satır numarası ve NUL ayraçlı dosya adı.
RG_STRUCTURED_FLAGS: tuple[str, ...] = (
    "--line-number", "--with-filename", "--no-heading", "--color", "never", "--null",
)  # fmt: skip
GREP_STRUCTURED_FLAGS: tuple[str, ...] = ("-n", "-H", "--color=never", "--null")


@dataclass(frozen=True)
class Command:
    argv: tuple[str, ...]
    program: Program


@dataclass(frozen=True)
class Operands:
    patterns: tuple[str, ...]
    paths: tuple[str, ...]
    flags: frozenset[str]


@dataclass(frozen=True)
class OptionSets:
    shape_letters: frozenset[str]
    shape_long: frozenset[str]
    value_letters: frozenset[str]
    value_long: frozenset[str]


def parse_simple(text: str) -> Command | None:
    """Yalnızca boru, yönlendirme ya da alt kabuk içermeyen tek bir rg/grep çağrısını kabul eder."""
    if "$(" in text or "`" in text or "\n" in text:
        return None
    try:
        lexer = shlex.shlex(text, posix=True, punctuation_chars=True)
        lexer.whitespace_split = True
        parts = list(lexer)
    except ValueError:
        return None
    if not parts:
        return None
    if any(part and set(part) <= SHELL_OPERATOR_CHARS for part in parts):
        return None
    if any(part.startswith(FORBIDDEN_PREFIXES) for part in parts[1:]):
        return None
    if parts[0] == "rg":
        return Command(argv=tuple(parts), program="rg")
    if parts[0] == "grep":
        return Command(argv=tuple(parts), program="grep")
    return None


def option_sets(command: Command) -> OptionSets:
    if command.program == "rg":
        return OptionSets(RG_SHAPE_LETTERS, RG_SHAPE_LONG, RG_VALUE_LETTERS, RG_VALUE_LONG)
    return OptionSets(GREP_SHAPE_LETTERS, GREP_SHAPE_LONG, GREP_VALUE_LETTERS, GREP_VALUE_LONG)


def is_line_shaped(command: Command) -> bool:
    """Çıktı `yol:satır:metin` eşleşme satırlarından mı oluşuyor (yeniden biçimlenebilir mi)?"""
    sets = option_sets(command)
    argv = command.argv
    index = 1
    while index < len(argv):
        part = argv[index]
        if part == "--":
            break
        if part.startswith("--"):
            name, equals, _ = part.partition("=")
            if name in sets.shape_long:
                return False
            if name in sets.value_long and not equals:
                index += 1
        elif part.startswith("-") and len(part) > 1:
            letters = part[1:]
            for position, letter in enumerate(letters):
                if letter in sets.shape_letters:
                    return False
                if letter in sets.value_letters:
                    if position == len(letters) - 1:
                        index += 1
                    break
        index += 1
    return True


def structured_argv(command: Command) -> list[str]:
    """Aynı arama, yapı bilgisi için satır numarası ve NUL ayraçlı dosya adıyla."""
    flags = RG_STRUCTURED_FLAGS if command.program == "rg" else GREP_STRUCTURED_FLAGS
    return [command.argv[0], *flags, *command.argv[1:]]


def _long_flag(name: str) -> str | None:
    return {
        "--ignore-case": "i",
        "--smart-case": "S",
        "--fixed-strings": "F",
        "--word-regexp": "w",
        "--extended-regexp": "E",
        "--perl-regexp": "P",
    }.get(name)


def _short_flag(letter: str, program: Program) -> str | None:
    if letter in ("i", "F", "w", "P"):
        return letter
    if letter == "S" and program == "rg":
        return "S"
    if letter == "E" and program == "grep":
        return "E"
    return None


def operands(command: Command) -> Operands:
    """Desenler (-e ya da ilk konumsal), yollar ve eşleşmeyi etkileyen bayraklar."""
    sets = option_sets(command)
    argv = command.argv
    explicit: list[str] = []
    positional: list[str] = []
    flags: set[str] = set()
    options_ended = False
    index = 1
    while index < len(argv):
        part = argv[index]
        if options_ended or not part.startswith("-") or part == "-":
            positional.append(part)
        elif part == "--":
            options_ended = True
        elif part.startswith("--"):
            name, equals, value = part.partition("=")
            if name == "--regexp":
                if equals:
                    explicit.append(value)
                elif index + 1 < len(argv):
                    explicit.append(argv[index + 1])
                    index += 1
            elif (flag := _long_flag(name)) is not None:
                flags.add(flag)
            elif name in sets.value_long and not equals:
                index += 1
        else:
            letters = part[1:]
            for position, letter in enumerate(letters):
                if letter in sets.value_letters:
                    value = letters[position + 1 :]
                    if not value and index + 1 < len(argv):
                        value = argv[index + 1]
                        index += 1
                    if letter == "e":
                        explicit.append(value)
                    break
                if (flag := _short_flag(letter, command.program)) is not None:
                    flags.add(flag)
        index += 1
    if explicit:
        return Operands(tuple(explicit), tuple(positional), frozenset(flags))
    return Operands(tuple(positional[:1]), tuple(positional[1:]), frozenset(flags))


def _bre_to_ere(pattern: str) -> str:
    """GNU BRE kaçışlarını ERE karşılıklarına çevirir (yaklaşık)."""
    for escaped, extended in (
        ("\\|", "|"), ("\\(", "("), ("\\)", ")"), ("\\+", "+"), ("\\?", "?"), ("\\{", "{"),
        ("\\}", "}"),
    ):  # fmt: skip
        pattern = pattern.replace(escaped, extended)
    return pattern


def pattern_regexes(command: Command) -> list[re.Pattern[str]]:
    """Eşleşme sütununu bulmak için desenlerin Python karşılıkları; derlenemeyenler atlanır."""
    found = operands(command)
    regexes: list[re.Pattern[str]] = []
    for pattern in found.patterns:
        candidates = [re.escape(pattern)] if "F" in found.flags else [pattern, _bre_to_ere(pattern)]
        insensitive = "i" in found.flags or ("S" in found.flags and pattern == pattern.lower())
        for candidate in candidates:
            body = rf"\b(?:{candidate})\b" if "w" in found.flags else candidate
            try:
                regexes.append(re.compile(body, re.IGNORECASE if insensitive else 0))
                break
            except re.error:
                continue
    return regexes
