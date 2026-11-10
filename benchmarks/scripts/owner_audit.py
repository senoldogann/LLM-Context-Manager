"""Cevaplardaki sahip (sınıf, tip) adlarının denetimi; kayıtlara ve puanlara dokunmaz.

Kullanım:
    uv run python scripts/owner_audit.py <korpus dizini> <sonuç dizini>...

Preregistered puanlayıcı cevapları dosya ve ad düzeyinde puanlar: `Receiver.closed`,
`Sender.closed` yerine kabul edilir, aynı dosyadaki aynı adlı iki metot tek öğedir. Bu betik
her koşunun cevabında iki şeyi sayar:

- yanlış sahip: `yol:Sahip.ad` girdisinde `ad`, o dosyada `Sahip`'in içinde tanımlı değil
  (sahip; tanımın sınıf/impl/fonksiyon/mod zincirinde, dosya adında ya da dizinlerinde
  aranır: geniş kabul, yanlış alarmı önlemek içindir);
- eksik kopya: aynı dosyada aynı adı taşıyan birden çok altın caller'ı olan görevlerde
  (`OWNER_GOLD`), sahipli cevap bunların hepsini adlandırmıyor.

Python ve Rust dosyaları denetlenir; diğer diller "denetlenmedi" sayılır. Düzenleme
görevleri denetlenmez: cevapları ajanın eklediği, sabit korpusta olmayan koda atıf yapar.
"""

import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path

from agent.score import answer_entries
from agent.tasks import KNOWN_TASKS

# Aynı dosyada aynı adla birden çok altın caller'ı olan öğeler: (görev, yol, ad) → sahipler.
OWNER_GOLD: dict[tuple[str, str, str], frozenset[str]] = {
    ("django-trap-urlencode", "django/test/client.py", "generic"): frozenset(
        {"RequestFactory", "AsyncRequestFactory"}
    ),
    ("django-callers-constant-time-compare", "django/contrib/auth/hashers.py", "verify"): (
        frozenset(
            {
                "PBKDF2PasswordHasher",
                "BCryptSHA256PasswordHasher",
                "ScryptPasswordHasher",
                "MD5PasswordHasher",
            }
        )
    ),
    ("django-callers-iri-to-uri", "django/utils/feedgenerator.py", "__init__"): frozenset(
        {"SyndicationFeed", "Enclosure"}
    ),
}
SOURCE_PATH = re.compile(r"^(?P<path>[\w./-]+\.(?:py|rs)):(?P<rest>.+)$")
PY_DEF = re.compile(r"^(?P<indent>\s*)(?:async\s+)?(?P<kind>def|class)\s+(?P<name>\w+)")
RS_ITEM = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:(?:async|unsafe|const)\s+)*"
    r"(?:(?P<fn>fn)\s+(?P<fname>\w+)|(?P<mod>mod)\s+(?P<mname>\w+)\s*\{|"
    r"impl(?:<[^{]*?>)?\s+(?:[\w:]+(?:<[^{]*?>)?\s+for\s+)?(?P<iname>[\w:]+))"
)


@dataclass(frozen=True)
class Entry:
    """Cevap satırının yolu, sahibi (varsa) ve adı."""

    path: str
    owner: str | None
    name: str


@dataclass(frozen=True)
class RunAudit:
    """Bir koşunun denetim sonucu."""

    wrong_owner: tuple[str, ...]
    missing_copies: tuple[str, ...]
    unqualified_copies: int


def parse_entry(entry: str, workspace_name: str) -> Entry | None:
    """`yol:Sahip.ad` / `yol:Sahip::ad` / `yol:ad`; Python ve Rust dışı yollar için None."""
    text = entry.strip().strip("`")
    marker = f"/{workspace_name}/"
    if marker in text:
        text = text.split(marker, 1)[1]
    match = SOURCE_PATH.match(text.removeprefix("./"))
    if match is None:
        return None
    rest = re.sub(r"<[^>]*>|\(\)", "", match.group("rest")).strip()
    parts = [part for part in re.split(r"::|\.", rest) if part]
    if not parts:
        return None
    owner = parts[-2] if len(parts) > 1 else None
    return Entry(path=match.group("path"), owner=owner, name=parts[-1])


def python_owners(source: str, name: str) -> set[str]:
    """`name` tanımlarının kapsayan sınıf/fonksiyon zincirlerindeki adlar."""
    owners: set[str] = set()
    stack: list[tuple[int, str]] = []
    for line in source.splitlines():
        match = PY_DEF.match(line)
        if match is None:
            continue
        indent = len(match.group("indent"))
        stack = [(level, owner) for level, owner in stack if level < indent]
        if match.group("kind") == "def" and match.group("name") == name:
            owners.update(owner for _, owner in stack)
        stack.append((indent, match.group("name")))
    return owners


def balanced_generics(text: str) -> tuple[str, str]:
    """`<...>` ile başlayan metnin dengeli generic içeriği ve kalanı; `<` yoksa ("", metin)."""
    if not text.startswith("<"):
        return "", text
    depth = 0
    for index, character in enumerate(text):
        depth += {"<": 1, ">": -1}.get(character, 0)
        if depth == 0:
            return text[1:index], text[index + 1 :]
    return text[1:], ""


def impl_self_types(header: str) -> set[str]:
    """`impl` başlığındaki self tip adı ve generic argümanlarındaki tipler.

    `impl<T> Schedule for Arc<Handle> where T: Convert` → {Arc, Handle}: trait (`Schedule`),
    trait argümanları ve `where` kısıtları sayılmaz.
    """
    rest = header.strip().removeprefix("impl")
    _, rest = balanced_generics(rest.lstrip())
    rest = re.split(r"\bwhere\b|\{", rest, maxsplit=1)[0].strip()
    # `for` en üst düzeydeyse self tip ondan sonradır (trait argümanları atlanır).
    depth = 0
    for match in re.finditer(r"<|>|\bfor\b", rest):
        token = match.group(0)
        if token in "<>":
            depth += 1 if token == "<" else -1
        elif depth == 0:
            rest = rest[match.end() :].strip()
            break
    head = re.match(r"[\w:]+", rest)
    if head is None:
        return set()
    arguments, _ = balanced_generics(rest[head.end() :])
    return {head.group(0).rsplit("::", 1)[-1], *re.findall(r"\b[A-Z]\w*", arguments)}


def rust_owners(source: str, name: str) -> set[str]:
    """`fn name` tanımlarının kapsayan impl tipleri, fonksiyon ve mod adları.

    `impl Trait for Arc<Handle>` içindeki fonksiyonun sahipleri `Arc` ve `Handle`'dır.
    """
    owners: set[str] = set()
    stack: list[tuple[int, frozenset[str]]] = []
    # `{` sonraki satırda açılabilir (`where` bloğu): öğenin sahip adları o satıra kadar bekler.
    pending: frozenset[str] | None = None
    depth = 0
    for line in source.splitlines():
        match = RS_ITEM.match(line)
        if match is not None:
            if match.group("fn") and match.group("fname") == name:
                owners.update(owner for _, labels in stack for owner in labels)
            if match.group("iname"):
                pending = frozenset(impl_self_types(line))
            else:
                label = match.group("fname") or match.group("mname")
                pending = frozenset({label}) if label else None
        if pending is not None and "{" in line:
            stack.append((depth, pending))
            pending = None
        elif pending is not None and line.rstrip().endswith(";"):
            pending = None
        depth += line.count("{") - line.count("}")
        stack = [(level, labels) for level, labels in stack if level < depth]
    return owners


def owner_is_plausible(corpus: Path, repo: str, entry: Entry) -> bool | None:
    """Sahip bu dosyada adın kapsayanlarından biri mi; dosya yoksa ya da dil yoksa None."""
    path = corpus / repo / entry.path
    if entry.owner is None or not path.is_file():
        return None
    source = path.read_text(encoding="utf-8")
    owners = (
        python_owners(source, entry.name)
        if path.suffix == ".py"
        else rust_owners(source, entry.name)
    )
    structural = set(Path(entry.path).with_suffix("").parts)
    return entry.owner in owners or entry.owner in structural


def audit_run(corpus: Path, record: dict[str, object]) -> RunAudit | None:
    """Koşunun cevabını denetler; cevap yoksa None."""
    transcript = record.get("transcript")
    task_id = str(record.get("task_id"))
    if not isinstance(transcript, dict) or not isinstance(transcript.get("result_text"), str):
        return None
    entries_text = answer_entries(transcript["result_text"])
    if entries_text is None:
        return None
    repo = str(record.get("repo"))
    workspace = f"ws-{record.get('run_id')}"
    entries = [entry for text in entries_text if (entry := parse_entry(text, workspace))]
    wrong = tuple(
        f"{entry.path}:{entry.owner}.{entry.name}"
        for entry in entries
        if owner_is_plausible(corpus, repo, entry) is False
    )
    missing: list[str] = []
    unqualified = 0
    for (gold_task, gold_path, gold_name), gold_owners in OWNER_GOLD.items():
        if gold_task != task_id:
            continue
        named = [entry for entry in entries if (entry.path, entry.name) == (gold_path, gold_name)]
        if any(entry.owner is None for entry in named):
            unqualified += 1
            continue
        missing.extend(
            f"{gold_path}:{owner}.{gold_name}"
            for owner in sorted(gold_owners - {entry.owner for entry in named if entry.owner})
        )
    return RunAudit(
        wrong_owner=wrong, missing_copies=tuple(missing), unqualified_copies=unqualified
    )


def measured_commit(agent_dir: Path) -> str:
    """Kümenin ölçüldüğü commit (`settings.json` → `harness_revision`), kısa biçim."""
    settings = json.loads((agent_dir / "settings.json").read_text())
    revision = settings.get("harness_revision")
    if not isinstance(revision, str):
        raise ValueError(f"{agent_dir}/settings.json has no harness_revision")
    return revision[:7]


def main() -> None:
    """Sonuç dizinlerindeki tüm koşuları denetler ve görev başına özetler."""
    corpus = Path(sys.argv[1])
    # Düzenleme görevlerinin cevapları ajanın eklediği koda atıf yapar; korpusta yoktur.
    known = {task.id for task in KNOWN_TASKS if task.edit is None}
    print(
        "| Results | Agent | Commit | Task | Runs | Wrong owner | Missing same-name copy "
        "| Unqualified copy |"
    )
    print("|---|---|---|---|---|---|---|---|")
    audited: list[str] = []
    examples: list[str] = []
    for results in (Path(argument) for argument in sys.argv[2:]):
        # Ajan başına alt dizin (`<set>/claude/runs`) ya da tek ajanlı küme (`<set>/runs`).
        agent_dirs = (
            [results]
            if (results / "runs").is_dir()
            else sorted(path for path in results.iterdir() if (path / "runs").is_dir())
        )
        for agent_dir in agent_dirs:
            commit = measured_commit(agent_dir)
            audited.append(f"{results.name}/{agent_dir.name} at `{commit}`")
            by_task: dict[str, list[RunAudit]] = {}
            for path in sorted((agent_dir / "runs").glob("*.json")):
                record = json.loads(path.read_text())
                if record.get("task_id") not in known:
                    continue
                audit = audit_run(corpus, record)
                if audit is not None:
                    by_task.setdefault(str(record["task_id"]), []).append(audit)
                    if audit.wrong_owner or audit.missing_copies:
                        examples.append(
                            f"- {results.name}/{agent_dir.name} {record['run_id']}: wrong "
                            f"{list(audit.wrong_owner)}, missing {list(audit.missing_copies)}"
                        )
            for task, audits in sorted(by_task.items()):
                wrong = sum(1 for audit in audits if audit.wrong_owner)
                missing = sum(1 for audit in audits if audit.missing_copies)
                unqualified = sum(1 for audit in audits if audit.unqualified_copies)
                if wrong or missing or unqualified:
                    print(
                        f"| {results.name} | {agent_dir.name} | `{commit}` | `{task}` | "
                        f"{len(audits)} | {wrong} | {missing} | {unqualified} |"
                    )
    print()
    print("\n".join(examples))
    print()
    print("Audited result sets and their measured commits: " + "; ".join(audited) + ".")


if __name__ == "__main__":
    main()
