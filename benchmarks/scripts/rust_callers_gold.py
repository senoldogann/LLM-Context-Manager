"""Rust kütüphane crate'lerinde serbest ya da ilişkili fonksiyonların çağıranları (CCM'siz).

Kullanım (`benchmarks/` içinde):
    uv run python scripts/rust_callers_gold.py <depo kökü> <crate,...> <ad>
    uv run python scripts/rust_callers_gold.py <depo kökü> <crate,...> --select <tohum> <dışlanan>

Kaynak düz metin olarak taranır, hiçbir CCM paketi okunmaz:

- taranan dosyalar `<crate>/src/**.rs`; `tests/`, `benches/`, `examples/` dizinleri, `tests.rs`
  dosyaları ve `test` koşullu (`#[cfg(test)]`, `#[cfg(all(test, …))]`; `not(test)` değil)
  `mod` blokları sayılmaz;
- çağrı yalnız yol biçimidir: `ad(`, `Tip::ad(`, `modül::ad(` ve turbofish; `.ad(` metot çağrısı
  ve `fn ad(` tanımı sayılmaz; `//` yorum satırları atlanır;
- ad çözülür: nitelikli çağrı (`X::ad(`) yalnız `X` tanımın `impl` tipi, tanım dosyasının
  yolundaki bir modül (`sync/mpsc/unbounded.rs` → `sync`, `mpsc`, `unbounded`; yeniden dışa
  aktarma için), `self`/`super`/`crate` ya da kapsayan `impl` tanımın sahibiyken `Self` ise;
  çıplak çağrı (`ad(`) yalnız tanımın dosyasında ya da adı tanımın modül yolundan (veya
  modülünü `*` ile) `use` eden dosyada sayılır;
- çağrı, kapsayan en içteki `fn`'ye atanır (`impl` tipi ön ek olarak yazılır).

`--select` görev adaylarını Python seçicisiyle aynı kurallarla ve sabit tohumla seçer: adı
kütüphane crate'lerinde tek tanımlı, `self` almayan fonksiyon; çağıran görevi 6–20 çağrı ve 3–6
dosya (iki tane), etki görevi 6–20 çağrı ve en az 7 dosya (bir tane), tuzak 4–20 çağrı, en az 3
dosya ve aynı ad başka bir şeyi de adlandırır: ya en az iki `.ad(` metot çağrısı vardır ya da ad
iki ayrı dosyada `self` almayan iki tanımdır ve diğerinin en az bir çağrısı vardır (bir tane).
Sorgu `<tanım dosyası>:<ad>` biçimindedir; iki tanımlı adda `callers` o dosyadaki tanımı alır.
"""

import random
import re
import sys
from collections import Counter, defaultdict
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path

FN_DEF = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:(?:const|async|unsafe|extern\s+\"[^\"]*\")\s+)*fn\s+(?P<name>\w+)"
)
IMPL = re.compile(r"^\s*(?:unsafe\s+)?impl\b(?P<header>[^{]*)")
TEST_MOD = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{")
NON_LIBRARY_DIRECTORIES = {"tests", "benches", "examples"}


@dataclass(frozen=True)
class Site:
    """Bir çağrı yeri ve kapsayan fonksiyon."""

    path: str
    line: int
    caller: str


@dataclass(frozen=True)
class Definition:
    """Bir `fn` tanımı, sahibi (`impl` tipi ya da modül) ve `self` alıp almadığı."""

    path: str
    line: int
    name: str
    owner: str
    takes_self: bool


def module_name(path: str) -> str:
    """Dosyanın modül adı: `a/b/c.rs` → `c`, `a/b/mod.rs` → `b`."""
    parts = Path(path).with_suffix("").parts
    return parts[-2] if parts[-1] in ("mod", "lib") and len(parts) > 1 else parts[-1]


def library_files(root: Path, crates: list[str]) -> list[Path]:
    """Taranan kaynak dosyaları."""
    files = []
    for crate in crates:
        for path in sorted((root / crate / "src").rglob("*.rs")):
            parts = set(path.relative_to(root).parts)
            if parts & NON_LIBRARY_DIRECTORIES or path.name == "tests.rs":
                continue
            files.append(path)
    return files


def cfg_mentions_test(attribute: str) -> bool:
    """`#[cfg(...)]` koşulu `test`'i olumlu anıyor mu (`not(test)` sayılmaz)."""
    if not attribute.startswith("#[cfg("):
        return False
    positive = re.sub(r"not\([^()]*\)", "", attribute)
    return re.search(r"\btest\b", positive) is not None


def strip_line(line: str) -> str:
    """Satırın kod kısmı: `//` yorumu ve dize içerikleri atılır (süslü parantez sayımı için)."""
    code = re.sub(r'"(?:[^"\\]|\\.)*"', '""', line)
    code = re.sub(r"'(?:[^'\\]|\\.)'", "''", code)
    return code.split("//", 1)[0]


def impl_owner(header: str) -> str:
    """`impl<T> Trait for Tip<T>` → `Tip`; `impl Tip` → `Tip`."""
    rest = header.strip()
    if rest.startswith("<"):
        depth = 0
        for index, character in enumerate(rest):
            depth += {"<": 1, ">": -1}.get(character, 0)
            if depth == 0:
                rest = rest[index + 1 :].strip()
                break
    if " for " in f" {rest} ":
        rest = rest.split(" for ", 1)[1]
    rest = re.split(r"\bwhere\b", rest, maxsplit=1)[0]
    head = re.match(r"[\w:]+", rest.strip())
    return head.group(0).rsplit("::", 1)[-1] if head else ""


@dataclass
class Frame:
    """Açık bir blok: derinliği, türü (fn/impl/test) ve adı."""

    depth: int
    kind: str
    name: str


def scan_file(
    root: Path,
    path: Path,
    call: re.Pattern[str] | None,
    accepts: Callable[[str | None, str | None], bool],
) -> tuple[list[Definition], list[Site]]:
    """Dosyadaki tanımlar ve (desen verildiyse) çağrı yerleri, kapsayan fonksiyonla."""
    relative = str(path.relative_to(root))
    definitions: list[Definition] = []
    sites: list[Site] = []
    stack: list[Frame] = []
    pending: Frame | None = None
    cfg_test = False
    depth = 0
    lines = path.read_text(encoding="utf-8").splitlines()
    for number, raw in enumerate(lines, 1):
        code = strip_line(raw)
        stripped = code.strip()
        in_test = any(frame.kind == "test" for frame in stack)
        if cfg_mentions_test(stripped):
            cfg_test = True
        fn_match = FN_DEF.match(code)
        impl_match = IMPL.match(code)
        if fn_match:
            signature = " ".join(strip_line(line) for line in lines[number - 1 : number + 4])
            params = signature.split("(", 1)[1] if "(" in signature else ""
            takes_self = bool(
                re.match(r"\s*(?:&\s*(?:'\w+\s+)?(?:mut\s+)?)?(?:mut\s+)?self\b", params)
            )
            impls = [frame.name for frame in stack if frame.kind == "impl"]
            owner = impls[-1] if impls else module_name(relative)
            if not in_test:
                definitions.append(
                    Definition(relative, number, fn_match["name"], owner, takes_self)
                )
            pending = Frame(0, "fn", fn_match["name"])
        elif impl_match:
            pending = Frame(0, "impl", impl_owner(impl_match["header"]))
        elif cfg_test and TEST_MOD.match(code):
            pending = Frame(0, "test", "")
        if call is not None and not in_test and not stripped.startswith("//"):
            for match in call.finditer(code):
                before = code[: match.start()]
                if re.search(r"\bfn\s+$", before) or before.endswith("."):
                    continue
                impls = [frame.name for frame in stack if frame.kind == "impl"]
                if not accepts(match["qual"], impls[-1] if impls else None):
                    continue
                functions = [frame for frame in stack if frame.kind == "fn"]
                if not functions:
                    caller = "<module>"
                else:
                    inner = functions[-1]
                    owners = [
                        frame.name for frame in stack[: stack.index(inner)] if frame.kind == "impl"
                    ]
                    caller = ".".join([*owners[-1:], inner.name])
                sites.append(Site(relative, number, caller))
        opened = code.count("{")
        closed = code.count("}")
        if pending is not None and opened:
            pending.depth = depth
            stack.append(pending)
            pending = None
            if stack[-1].kind == "test":
                cfg_test = False
        elif pending is not None and stripped.endswith(";"):
            pending = None
        # `#[cfg(test)]` yalnız hemen ardından gelen öğeye uygulanır.
        if stripped and not stripped.startswith("#") and not TEST_MOD.match(code):
            cfg_test = False
        depth += opened - closed
        stack = [frame for frame in stack if frame.depth < depth]
    return definitions, sites


def call_pattern(name: str) -> re.Pattern[str]:
    """`ad(`, `X::ad(`, `ad::<T>(` biçimleri (`qual` = `X`); `.ad(` ayrıca elenir."""
    return re.compile(
        rf"(?:(?<![\w.])(?P<qual>\w+)\s*::\s*)?(?<![\w.]){re.escape(name)}\s*(?:::<[^()]*>)?\s*\("
    )


def imports_name(text: str, name: str, owner: str, modules: set[str]) -> bool:
    """Dosya adı tanımın modül yolundan `use` ediyor mu (ya da modülünü `*` ile).

    `use` ifadesi adı ve tanım yolundaki modüllerden birini birlikte anmalıdır: aynı adı
    başka bir modülden alan dosya (`use crate::task::unconstrained`) sayılmaz.
    """
    for statement in re.findall(r"\buse\s[^;]*;", text, re.DOTALL):
        names_it = re.search(rf"\b{re.escape(name)}\b", statement) is not None
        from_module = any(re.search(rf"\b{re.escape(part)}\b", statement) for part in modules)
        if (names_it and from_module) or re.search(rf"\b{re.escape(owner)}::\*", statement):
            return True
    return False


def all_definitions(root: Path, crates: list[str]) -> dict[str, list[Definition]]:
    """Kütüphane crate'lerindeki `fn` tanımları, ada göre."""
    definitions: dict[str, list[Definition]] = defaultdict(list)
    for path in library_files(root, crates):
        for definition in scan_file(root, path, None, lambda _qual, _impl: True)[0]:
            definitions[definition.name].append(definition)
    return definitions


def acceptance(
    owner: str, qualifiers: set[str], bare: bool
) -> Callable[[str | None, str | None], bool]:
    """Bir dosyanın çağrı kabul kuralı: `Self` yalnız sahibin `impl`'inde, nitelikli çağrı
    yalnız bilinen niteleyiciyle, çıplak çağrı yalnız dosya adı görüyorsa (`bare`)."""

    def accepts(qualifier: str | None, impl: str | None) -> bool:
        if qualifier == "Self":
            return impl == owner
        return qualifier in qualifiers if qualifier is not None else bare

    return accepts


def callers(root: Path, crates: list[str], definition: Definition) -> list[Site]:
    """Tanımın yol biçimindeki, ad çözümlemesinden geçen çağrıları."""
    pattern = call_pattern(definition.name)
    modules = set(Path(definition.path).with_suffix("").parts[1:]) - {"src", "mod", "lib"}
    qualifiers = {definition.owner, "self", "super", "crate", *modules}
    sites: list[Site] = []
    for path in library_files(root, crates):
        relative = str(path.relative_to(root))
        bare = relative == definition.path or imports_name(
            path.read_text(encoding="utf-8"), definition.name, definition.owner, modules
        )

        sites.extend(
            scan_file(root, path, pattern, acceptance(definition.owner, qualifiers, bare))[1]
        )
    return sites


def method_calls(root: Path, crates: list[str]) -> Counter[str]:
    """`.ad(` metot çağrılarının adları."""
    counts: Counter[str] = Counter()
    for path in library_files(root, crates):
        for raw in path.read_text(encoding="utf-8").splitlines():
            counts.update(re.findall(r"\.(\w+)\s*(?:::<[^()]*>)?\s*\(", strip_line(raw)))
    return counts


def select(root: Path, crates: list[str], seed: int, excluded: set[str]) -> None:
    """Aday havuzlarını kurar ve sabit tohumla seçer."""
    files = library_files(root, crates)
    definitions = all_definitions(root, crates)
    text = "\n".join(path.read_text(encoding="utf-8") for path in files)
    methods = method_calls(root, crates)
    pools: dict[str, list[tuple[str, int, int]]] = {"callers": [], "impact": [], "trap": []}
    for name, found in sorted(definitions.items()):
        if len(name) < 6 or name in excluded or any(item.takes_self for item in found):
            continue
        if len(found) == 2 and found[0].path != found[1].path:
            if not 4 <= len(re.findall(rf"\b{re.escape(name)}\b", text)) <= 80:
                continue
            pair = [callers(root, crates, item) for item in found]
            for chosen, sites, other in (
                (found[0], pair[0], pair[1]),
                (found[1], pair[1], pair[0]),
            ):
                spread = len({site.path for site in sites})
                if 4 <= len(sites) <= 20 and spread >= 3 and other:
                    pools["trap"].append((f"{chosen.path}:{name}", len(sites), spread))
            continue
        if len(found) != 1:
            continue
        if not 4 <= len(re.findall(rf"\b{re.escape(name)}\b", text)) <= 80:
            continue
        sites = callers(root, crates, found[0])
        count, spread = len(sites), len({site.path for site in sites})
        entry = (f"{found[0].path}:{name}", count, spread)
        if 6 <= count <= 20 and 3 <= spread <= 6:
            pools["callers"].append(entry)
        if 6 <= count <= 20 and spread >= 7:
            pools["impact"].append(entry)
        if 4 <= count <= 20 and spread >= 3 and methods[name] >= 2:
            pools["trap"].append(entry)
    generator = random.Random(seed)
    for label, count in (("callers", 2), ("impact", 1), ("trap", 1)):
        pool = pools[label]
        print(f"{label}: pool {len(pool)}")
        for item in generator.sample(pool, count) if len(pool) >= count else []:
            print(f"  {item[0]}\t{item[1]} sites\t{item[2]} files")


def main() -> None:
    """Komut satırı."""
    root, crates = Path(sys.argv[1]), sys.argv[2].split(",")
    if sys.argv[3] == "--select":
        excluded = set(sys.argv[5].split(",")) if len(sys.argv) > 5 else set()
        select(root, crates, int(sys.argv[4]), excluded)
        return
    path, _, name = sys.argv[3].rpartition(":")
    found = [item for item in all_definitions(root, crates)[name] if not path or item.path == path]
    if len(found) != 1:
        raise SystemExit(f"{sys.argv[3]} has {len(found)} definitions: {found}")
    sites = callers(root, crates, found[0])
    for site in sites:
        print(f"{site.path}\t{site.line}\t{site.caller}")
    print(
        f"{len(sites)} sites in {len({(s.path, s.caller) for s in sites})} functions",
        file=sys.stderr,
    )


if __name__ == "__main__":
    main()
