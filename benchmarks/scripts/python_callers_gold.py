"""Python modül düzeyi fonksiyonlarının çağıranları, `ast` ile (CCM'den bağımsız altın cevap).

Kullanım:
    uv run python scripts/python_callers_gold.py <depo kökü> <paket dizini> <modül> <ad>
    uv run python scripts/python_callers_gold.py <depo kökü> <paket dizini> --candidates

Çağrılar şu bağlarla tanınır: `from M import ad` (`as` dahil), `import M as m` / `from P import M`
sonrası `m.ad(...)`; M tanımlayan modül ya da adı yeniden dışa aktaran bir modüldür (zincir
halinde de: `sklearn.utils` → `sklearn.utils.validation`). Göreli import'lar çözülür. Test
dosyaları (`tests/` dizini, `test_*.py`, `conftest.py`) sayılmaz. Bir çağrı, kapsayan en dıştaki
adlandırılmış fonksiyona (sınıf adıyla) atanır; modül düzeyindeki çağrı `<module>` olur.
`--candidates` görev adayı listeler: adı pakette tek, 6–20 çağrı yeri, en az 3 dosya.
"""

import ast
import sys
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class Site:
    """Bir çağrı yeri ve kapsayan fonksiyon."""

    path: str
    line: int
    caller: str


def is_test(path: Path) -> bool:
    """Test dosyası mı."""
    return "tests" in path.parts or path.name.startswith("test_") or path.name == "conftest.py"


def module_name(root: Path, path: Path) -> str:
    """Dosya yolunun modül adı (`a/b/__init__.py` → `a.b`)."""
    parts = list(path.relative_to(root).with_suffix("").parts)
    if parts[-1] == "__init__":
        parts = parts[:-1]
    return ".".join(parts)


def resolve_from(module: str, node: ast.ImportFrom, is_package: bool) -> str:
    """`from ... import` bildiriminin mutlak modül adı."""
    if node.level == 0:
        return node.module or ""
    base = module.split(".")
    if not is_package:
        base = base[:-1]
    base = base[: len(base) - (node.level - 1)]
    return ".".join(base + ([node.module] if node.module else []))


@dataclass(frozen=True)
class ParsedModule:
    """Ayrıştırılmış modül: yol, ad, ağaç."""

    path: Path
    name: str
    tree: ast.Module
    is_package: bool


def parse_package(root: Path, package: str) -> list[ParsedModule]:
    """Paketteki tüm Python dosyaları (testler dahil; çağrı sayımında ayrılır)."""
    modules = []
    for path in sorted((root / package).rglob("*.py")):
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        modules.append(
            ParsedModule(path, module_name(root, path), tree, path.name == "__init__.py")
        )
    return modules


def providers(modules: list[ParsedModule], target_module: str, name: str) -> set[str]:
    """Adı sağlayan modüller: tanımlayan ve (zincirle) yeniden dışa aktaranlar."""
    found = {target_module}
    changed = True
    while changed:
        changed = False
        for module in modules:
            if module.name in found:
                continue
            for node in ast.walk(module.tree):
                if (
                    isinstance(node, ast.ImportFrom)
                    and resolve_from(module.name, node, module.is_package) in found
                    and any(
                        alias.name == name and alias.asname in (None, name) for alias in node.names
                    )
                ):
                    found.add(module.name)
                    changed = True
                    break
    return found


def call_sites(
    root: Path, modules: list[ParsedModule], target_module: str, name: str
) -> list[Site]:
    """Test dışı dosyalardaki doğrudan çağrılar."""
    provided = providers(modules, target_module, name)
    sites: list[Site] = []
    for module in modules:
        if is_test(module.path):
            continue
        names: set[str] = {name} if module.name == target_module else set()
        receivers: set[str] = set()
        for node in ast.walk(module.tree):
            if isinstance(node, ast.ImportFrom):
                source = resolve_from(module.name, node, module.is_package)
                for alias in node.names:
                    if source in provided and alias.name == name:
                        names.add(alias.asname or alias.name)
                    if f"{source}.{alias.name}" in provided:
                        receivers.add(alias.asname or alias.name)
            elif isinstance(node, ast.Import):
                for alias in node.names:
                    if alias.name in provided and alias.asname:
                        receivers.add(alias.asname)
        if not names and not receivers:
            continue
        relative = str(module.path.relative_to(root))
        sites.extend(collect_calls(module.tree, names, receivers, relative, name))
    return sites


def collect_calls(
    tree: ast.Module, names: set[str], receivers: set[str], relative: str, name: str
) -> list[Site]:
    """Ağaçtaki `ad(...)` ve `alıcı.ad(...)` çağrıları, kapsayan fonksiyonla."""
    found: list[Site] = []

    def visit(node: ast.AST, stack: list[ast.AST]) -> None:
        for child in ast.iter_child_nodes(node):
            if isinstance(child, ast.FunctionDef | ast.AsyncFunctionDef | ast.ClassDef):
                visit(child, [*stack, child])
                continue
            if isinstance(child, ast.Call):
                function = child.func
                hit = (isinstance(function, ast.Name) and function.id in names) or (
                    isinstance(function, ast.Attribute)
                    and function.attr == name
                    and isinstance(function.value, ast.Name)
                    and function.value.id in receivers
                )
                if hit:
                    found.append(Site(relative, child.lineno, caller_name(stack)))
            visit(child, stack)

    visit(tree, [])
    return found


def caller_name(stack: list[ast.AST]) -> str:
    """Kapsayan en dıştaki fonksiyon, önündeki sınıflarla (`Class.method`)."""
    functions = [node for node in stack if isinstance(node, ast.FunctionDef | ast.AsyncFunctionDef)]
    if not functions:
        return "<module>"
    outer = functions[0]
    index = stack.index(outer)
    classes = [node.name for node in stack[:index] if isinstance(node, ast.ClassDef)]
    return ".".join([*classes, outer.name])


def candidates(root: Path, modules: list[ParsedModule]) -> None:
    """Görev adayları: modül düzeyinde, adı pakette tek, 6–20 çağrı, en az 3 dosya."""
    definitions: Counter[str] = Counter()
    where: dict[str, str] = {}
    for module in modules:
        if is_test(module.path):
            continue
        for node in ast.walk(module.tree):
            if isinstance(node, ast.FunctionDef | ast.AsyncFunctionDef | ast.ClassDef):
                definitions[node.name] += 1
        for node in module.tree.body:
            if isinstance(node, ast.FunctionDef | ast.AsyncFunctionDef):
                where.setdefault(node.name, module.name)
    by_name: dict[str, list[Site]] = defaultdict(list)
    for name, defining_module in where.items():
        if definitions[name] != 1 or len(name) < 6:
            continue
        by_name[name] = call_sites(root, modules, defining_module, name)
    for name, sites in sorted(by_name.items(), key=lambda item: len(item[1])):
        files = {site.path for site in sites}
        if 6 <= len(sites) <= 20 and len(files) >= 3:
            print(f"{len(sites)}\t{len(files)}\t{where[name]}\t{name}")


def main() -> None:
    """Komut satırı."""
    root, package = Path(sys.argv[1]), sys.argv[2]
    modules = parse_package(root, package)
    if sys.argv[3] == "--candidates":
        candidates(root, modules)
        return
    target_module, name = sys.argv[3], sys.argv[4]
    sites = call_sites(root, modules, target_module, name)
    for site in sites:
        print(f"{site.path}\t{site.line}\t{site.caller}")
    callers = {(site.path, site.caller) for site in sites}
    print(f"{len(sites)} sites in {len(callers)} functions", file=sys.stderr)


if __name__ == "__main__":
    main()
