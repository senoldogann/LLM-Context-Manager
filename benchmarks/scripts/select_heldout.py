"""Python depolarından held-out görev adaylarını sabit tohumla seçer (CCM'e bakmadan).

Kullanım (`benchmarks/` içinde):
    PYTHONPATH=. uv run python scripts/select_heldout.py <depo kökü> <paket> <tohum> \
        <çağıran görevi sayısı> <etki görevi sayısı> <dışlanan>

Yalnız kaynak taranır (`scripts/python_callers_gold.py`'nin `ast` kuralları); hiçbir CCM paketi
okunmaz. Adaylar üst düzey, alt çizgiyle başlamayan, en az 6 harfli fonksiyonlardır; sayım test
dışı dosyalardadır. Dışlanan adlar (virgülle) önceki görevlerin hedefleridir.

- çağıran görevi: adı pakette tek tanımlı, metin olarak 4–60 kez çağrılan (ön eleme), 6–20
  çağrı yeri, 3–6 dosya; komut satırında verilen sayıda.
- etki görevi (hangi dosyalar çağırıyor): aynı koşul, en az 7 dosya; komut satırında verilen
  sayıda.
- tuzak: seçilen tanımın 4–20 çağrı yeri ve en az 3 dosyası var ve aynı ad başka bir şeyi de
  adlandırır: (a) pakette ikinci bir üst düzey tanım (en az bir çağrı yeriyle), (b) bir sınıfın
  metodu, adın çağrılarının en az ikisi seçilen fonksiyona gitmeden, ya da (c) paket dışından
  import edilen bir modülün aynı adlı fonksiyonu (`np.ad(...)` gibi en az iki çağrı); bir tane.
  (b) ve (c) havuzlarında çağrı sayısı ön elemesi yoktur.

Her havuz ada göre sıralanır ve `random.Random(tohum).sample` ile seçilir.
"""

import ast
import random
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path

from scripts.python_callers_gold import ParsedModule, call_sites, is_test, parse_package


@dataclass(frozen=True)
class Candidate:
    """Bir adayın tanım modülü, adı ve test dışı çağrı sayıları."""

    module: str
    name: str
    sites: int
    files: int


def top_level_functions(modules: list[ParsedModule]) -> dict[str, list[str]]:
    """Test dışı modüllerin üst düzey fonksiyonları: ad → tanımlayan modüller."""
    defined: dict[str, list[str]] = defaultdict(list)
    for module in modules:
        if is_test(module.path):
            continue
        for node in module.tree.body:
            if isinstance(node, ast.FunctionDef | ast.AsyncFunctionDef):
                defined[node.name].append(module.name)
    return defined


def method_names(modules: list[ParsedModule]) -> set[str]:
    """Test dışı modüllerde bir sınıfın içinde tanımlı metot adları."""
    names: set[str] = set()
    for module in modules:
        if is_test(module.path):
            continue
        for node in ast.walk(module.tree):
            if isinstance(node, ast.ClassDef):
                names.update(
                    item.name
                    for item in node.body
                    if isinstance(item, ast.FunctionDef | ast.AsyncFunctionDef)
                )
    return names


def external_attribute_calls(modules: list[ParsedModule], package: str) -> Counter[str]:
    """Paket dışından import edilen modül üzerinden yapılan çağrılar (`np.ad(...)`): ad → sayı."""
    counts: Counter[str] = Counter()
    for module in modules:
        if is_test(module.path):
            continue
        external: set[str] = set()
        for node in ast.walk(module.tree):
            if isinstance(node, ast.Import):
                external.update(
                    alias.asname or alias.name.split(".")[0]
                    for alias in node.names
                    if alias.name.split(".")[0] != package
                )
            elif (
                isinstance(node, ast.ImportFrom)
                and node.level == 0
                and node.module
                and node.module.split(".")[0] != package
            ):
                external.update(alias.asname or alias.name for alias in node.names)
        for node in ast.walk(module.tree):
            if (
                isinstance(node, ast.Call)
                and isinstance(node.func, ast.Attribute)
                and isinstance(node.func.value, ast.Name)
                and node.func.value.id in external
            ):
                counts[node.func.attr] += 1
    return counts


def called_names(modules: list[ParsedModule]) -> Counter[str]:
    """Test dışı modüllerde çağrılan adlar (`ad(...)` ve `x.ad(...)`); hızlı ön eleme içindir."""
    counts: Counter[str] = Counter()
    for module in modules:
        if is_test(module.path):
            continue
        for node in ast.walk(module.tree):
            if isinstance(node, ast.Call):
                function = node.func
                if isinstance(function, ast.Name):
                    counts[function.id] += 1
                elif isinstance(function, ast.Attribute):
                    counts[function.attr] += 1
    return counts


def candidate(root: Path, modules: list[ParsedModule], module: str, name: str) -> Candidate:
    """Tanımın çağrı sayıları."""
    sites = call_sites(root, modules, module, name)
    return Candidate(module, name, len(sites), len({site.path for site in sites}))


def select(
    root: Path, package: str, seed: int, callers_count: int, impact_count: int, excluded: set[str]
) -> None:
    """Havuzları kurar, seçer ve yazar."""
    modules = parse_package(root, package)
    defined = top_level_functions(modules)
    # Ön eleme: ad metin olarak 4–60 kez çağrılıyor (kesin sayım `call_sites` iledir).
    counts = called_names(modules)
    usable = {
        name: owners
        for name, owners in defined.items()
        if len(name) >= 6
        and not name.startswith("_")
        and name not in excluded
        and 4 <= counts[name] <= 60
    }
    unique = [
        candidate(root, modules, owners[0], name)
        for name, owners in sorted(usable.items())
        if len(owners) == 1
    ]
    callers = [item for item in unique if 6 <= item.sites <= 20 and 3 <= item.files <= 6]
    impact = [item for item in unique if 6 <= item.sites <= 20 and item.files >= 7]
    methods = method_names(modules)
    traps: list[Candidate] = []
    for name, owners in sorted(usable.items()):
        if len(owners) == 2:
            pair = [candidate(root, modules, owner, name) for owner in owners]
            for chosen, other in ((pair[0], pair[1]), (pair[1], pair[0])):
                if 4 <= chosen.sites <= 20 and chosen.files >= 3 and other.sites >= 1:
                    traps.append(chosen)
    # Metot ya da dış kütüphane adıyla çakışan adlar çok çağrılabilir; ön elemeye uğramaz.
    external = external_attribute_calls(modules, package)
    shadowed = [
        candidate(root, modules, owners[0], name)
        for name, owners in sorted(defined.items())
        if len(owners) == 1
        and (name in methods or external[name] >= 2)
        and len(name) >= 6
        and not name.startswith("_")
        and name not in excluded
    ]
    traps.extend(
        item
        for item in shadowed
        if 4 <= item.sites <= 20
        and item.files >= 3
        and (external[item.name] >= 2 or counts[item.name] - item.sites >= 2)
    )
    traps.sort(key=lambda item: (item.name, item.module))
    generator = random.Random(seed)
    for label, pool, count in (
        ("callers", callers, callers_count),
        ("impact", impact, impact_count),
        ("trap", traps, 1),
    ):
        picked = generator.sample(pool, count) if len(pool) >= count else []
        print(f"{label}: pool {len(pool)}")
        for item in picked:
            print(f"  {item.module}.{item.name}\t{item.sites} sites\t{item.files} files")


def main() -> None:
    """Komut satırı."""
    root, package, seed = Path(sys.argv[1]), sys.argv[2], int(sys.argv[3])
    callers_count, impact_count = int(sys.argv[4]), int(sys.argv[5])
    excluded = set(sys.argv[6].split(",")) if len(sys.argv) > 6 else set()
    select(root, package, seed, callers_count, impact_count, excluded)


if __name__ == "__main__":
    main()
