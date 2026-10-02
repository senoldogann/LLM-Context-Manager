"""Ajan cevabının ayrıştırılması ve beklenen cevapla karşılaştırılması (saf fonksiyonlar)."""

from __future__ import annotations

import json
import re
from dataclasses import dataclass

from agent.tasks import AnswerKind, Edit, Item, Task, label
from freshness.model import JsonValue

ANSWER_BLOCK = re.compile(r"```json[^\n]*\n(.*?)```", re.DOTALL)
SOURCE_PATH = re.compile(r"^(?P<path>[^\s:]+?\.(?:py|js|rs))(?::(?P<rest>.*))?$")
LINE_PREFIX = re.compile(r"^\d+(?:-\d+)?[:\s]*")
NAME_END = re.compile(r"[\s(<\[]")
NAME_SEPARATOR = re.compile(r"[.#]")
# Başarı için tüm beklenen öğeler bulunmalı ve listelenenlerin en az bu kadarı doğru olmalı.
MIN_PRECISION = 0.75

Key = tuple[str, str]


@dataclass(frozen=True)
class Score:
    """Bir cevabın puanı; `stale` yalnız düzenleme görevlerinde dolabilir."""

    answered: bool
    recall: float
    precision: float
    missing: tuple[str, ...]
    extra: tuple[str, ...]
    edit_applied: bool | None
    stale: tuple[str, ...]
    success: bool


def answer_entries(text: str) -> tuple[str, ...] | None:
    """Son ```json bloğundaki `answer` dizisi; blok yoksa ya da biçimi bozuksa None.

    Biçim hatası ölçümün bir sonucudur (ajan istenen biçime uymadı), harness hatası değildir:
    kayıtta `answered=false` olarak görünür.
    """
    blocks = ANSWER_BLOCK.findall(text)
    if not blocks:
        return None
    try:
        payload: JsonValue = json.loads(blocks[-1])
    except json.JSONDecodeError:
        return None
    if not isinstance(payload, dict):
        return None
    entries = payload.get("answer")
    if not isinstance(entries, list):
        return None
    strings = tuple(entry for entry in entries if isinstance(entry, str))
    return strings if len(strings) == len(entries) else None


def symbol_name(text: str) -> str:
    """`Flask.run()`, `Container::from_ast`, `ContentDeserializer<'de, E>` gibi
    yazımların son adı.
    """
    head = NAME_END.split(text.replace("::", ".").strip(), maxsplit=1)[0]
    parts = [part for part in NAME_SEPARATOR.split(head) if part]
    return parts[-1] if parts else ""


def entry_key(entry: str, workspace_name: str, kind: AnswerKind) -> Key:
    """Cevap satırını (yol, ad) anahtarına indirger.

    Mutlak yollar çalışma kopyasının adından sonrasına kısaltılır; satır numaraları, sınıf ve
    modül önekleri, `()` ve tür parametreleri atılır. Dosya görevlerinde ad boştur.
    """
    text = entry.strip().strip("`").strip()
    marker = f"/{workspace_name}/"
    if marker in text:
        text = text.split(marker, 1)[1]
    text = text.removeprefix("./")
    match = SOURCE_PATH.match(text)
    if match is None:
        return (text, "")
    if kind == "files":
        return (match.group("path"), "")
    rest = LINE_PREFIX.sub("", (match.group("rest") or "").strip())
    return (match.group("path"), symbol_name(rest))


def accepts(item: Item, key: Key) -> bool:
    """Anahtar bu öğeyi mi gösteriyor: aynı yol, aynı ad ya da kabul edilen diğer ad."""
    path, name = key
    return path == item.path and (name == item.name or name in item.aliases)


def unmatched(
    expected: tuple[Item, ...], keys: tuple[Key, ...]
) -> tuple[tuple[Item, ...], tuple[Key, ...]]:
    """Bire bir eşleştirme: eşleşmeyen beklenen öğeler ve fazladan anahtarlar."""
    remaining = list(expected)
    extra: list[Key] = []
    for key in keys:
        hit = next((item for item in remaining if accepts(item, key)), None)
        if hit is None:
            extra.append(key)
        else:
            remaining.remove(hit)
    return tuple(remaining), tuple(extra)


def squash(text: str) -> str:
    """Boşlukları atar; işaretler biçimlendirme farkından etkilenmesin."""
    return "".join(text.split())


def marker_files(edit: Edit) -> tuple[str, ...]:
    """Düzenleme denetimi için okunacak dosyalar."""
    return tuple(sorted({marker.file for marker in (*edit.present, *edit.absent)}))


def edit_applied(edit: Edit, contents: dict[str, str]) -> bool:
    """Değişiklik yapıldı mı: tüm `present` işaretleri var, hiçbir `absent` işareti yok."""
    squashed = {path: squash(text) for path, text in contents.items()}
    present = all(squash(marker.text) in squashed[marker.file] for marker in edit.present)
    absent = not any(squash(marker.text) in squashed[marker.file] for marker in edit.absent)
    return present and absent


def stale_items(edit: Edit, keys: tuple[Key, ...]) -> tuple[str, ...]:
    """Düzenleme öncesine ait cevap izleri: listelenen `removed` ve atlanan `added` öğeleri."""
    listed = [label(item) for item in edit.removed if any(accepts(item, key) for key in keys)]
    skipped = [label(item) for item in edit.added if not any(accepts(item, key) for key in keys)]
    return tuple(listed + skipped)


def score(task: Task, text: str, workspace_name: str, contents: dict[str, str]) -> Score:
    """Cevabı puanlar; `contents` düzenleme görevlerinde işaret dosyalarının son hâlidir."""
    applied = None if task.edit is None else edit_applied(task.edit, contents)
    entries = answer_entries(text)
    if entries is None:
        return Score(
            answered=False,
            recall=0.0,
            precision=0.0,
            missing=tuple(label(item) for item in task.expected),
            extra=(),
            edit_applied=applied,
            stale=(),
            success=False,
        )
    keys = tuple(
        dict.fromkeys(entry_key(entry, workspace_name, task.answer_kind) for entry in entries)
    )
    missing, extra = unmatched(task.expected, keys)
    matched = len(task.expected) - len(missing)
    precision = matched / len(keys) if keys else 0.0
    return Score(
        answered=True,
        recall=matched / len(task.expected),
        precision=precision,
        missing=tuple(label(item) for item in missing),
        extra=tuple(f"{path}:{name}" if name else path for path, name in extra),
        edit_applied=applied,
        stale=() if task.edit is None else stale_items(task.edit, keys),
        success=not missing and precision >= MIN_PRECISION and applied is not False,
    )
