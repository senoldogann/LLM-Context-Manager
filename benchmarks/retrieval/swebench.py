"""SWE-bench Lite test bölümünden hakutehtävät: alt küme sırası, oikeat vastaukset, görev dosyaları.

Girdi, Lite test bölümünün JSON Lines biçimidir (satır başına `instance_id`, `repo`,
`base_commit`, `created_at`, `version`, `problem_statement`, `patch`). Sorgu yalnız issue
metnidir; ipuçları ve test yaması kullanılmaz.

Uygunluk: düzeltme yaması tam olarak bir mevcut `.py` dosyasını değiştirir (yeni, silinen ya da
yeniden adlandırılan dosya yok). Lite'ın 300 örneğinin hepsi bu koşulu sağlar.

Sıra (ön kayıt): her depoda örnekler `sha256("<tohum>:<instance_id>")` onaltılık değerine göre
sıralanır. Genel sırada önce her depo, ad sırasıyla, bir yer alır; sonraki her yer en büyük
Sainte-Laguë bölümü `N_r / (2·s_r + 1)` olan depoya gider (`N_r` depodaki örnek sayısı, `s_r`
verilmiş yer sayısı; eşitlikte ad sırası). Alt küme bu sıranın ilk n örneğidir; böylece her n
için depolar orantılı temsil edilir ve n sonradan büyütülse de önceki örnekler değişmez.

Oikeat vastaukset:
- Dosya düzeyi: yamanın değiştirdiği dosya.
- Fonksiyon düzeyi: eski dosya (`base_commit`) ve yama uygulanmış yeni dosya `ast` ile
  çözümlenir. Fonksiyonun aralığı ilk dekoratör satırından (yoksa `def` satırından)
  `end_lineno`'ya kadardır. Silinen satır, onu içeren en içteki eski fonksiyona; eklenen
  satır, onu içeren en içteki yeni fonksiyonlardan eski dosyada da (nitelikli ad ve aynı adın
  kaçıncı tanımı olduğu ile) bulunan ilkine aittir. Hiçbir fonksiyona düşmeyen satırlar
  sayılır ama oikea vastaus üretmez. Eski fonksiyonlar CCM'nin `Function` düğümlerine dosya
  yolu, ad ve `def` satırıyla eşlenir (CCM'de dekoratör düğüme dahil değildir). Eşleşme tek
  değilse ya da bir adın tanım sayısı yamada değiştiği için eşleme belirsizse hata verilir.

Kullanım:
  uv run python -m retrieval.swebench order --instances swe-bench-lite-test.jsonl \\
    --seed 20261005 --out order.json
  uv run python -m retrieval.swebench gold-check --instances swe-bench-lite-test.jsonl \\
    --repos-dir repos --out gold_check.json
  uv run python -m retrieval.swebench tasks --instances swe-bench-lite-test.jsonl \\
    --instance-id django__django-11099 --repo-root repos/django --out-dir tasks
"""

from __future__ import annotations

import argparse
import ast
import hashlib
import json
import re
import subprocess
import warnings
from collections import Counter
from collections.abc import Sequence
from dataclasses import asdict, dataclass
from fractions import Fraction
from pathlib import Path

from freshness.jsonio import as_int, as_list, as_object, as_str
from freshness.model import JsonValue
from retrieval.graph_stats import generation_dir, read_json

SCHEMA_VERSION: int = 1
TASK_SCHEMA_VERSION: int = 1
FILE_HEADER: re.Pattern[str] = re.compile(r"^diff --git a/(?P<old>\S+) b/(?P<new>\S+)$")
HUNK_HEADER: re.Pattern[str] = re.compile(
    r"^@@ -(?P<old_start>\d+)(?:,(?P<old_count>\d+))? "
    r"\+(?P<new_start>\d+)(?:,(?P<new_count>\d+))? @@"
)
LONE_CARRIAGE_RETURN: re.Pattern[str] = re.compile(r"\r(?!\n)")
ALLOWED_EXTENDED_HEADERS: tuple[str, ...] = ("index ", "old mode ", "new mode ")
NO_NEWLINE_MARKER: str = "\\"
PYTHON_SUFFIX: str = ".py"
QUERY_TYPE: str = "search_code"
TEST_SPLIT: str = "test"
TASK_LANGUAGES: tuple[str, ...] = ("python",)
MIN_RECALL: int = 1
CCM_FUNCTION_TYPE: str = "Function"
CCM_FILE_TYPE: str = "File"
ORDER_RULE: str = (
    "per repo: ascending sha256('<seed>:<instance_id>'); global: one seat per repo in name "
    "order, then the largest Sainte-Lague quotient N_r/(2*s_r+1), ties by repo name"
)


class SweBenchError(RuntimeError):
    """Örnek dosyası, depo ya da indeks beklenen biçimde değil."""


class PatchError(SweBenchError):
    """Yama uygunluk koşulunu sağlamıyor ya da eski dosyaya uygulanamıyor."""


class GoldError(SweBenchError):
    """Fonksiyon düzeyindeki oikea vastaus belirsiz ya da CCM düğümüne eşlenemiyor."""


@dataclass(frozen=True)
class Instance:
    instance_id: str
    repo: str
    base_commit: str
    created_at: str
    version: str
    problem_statement: str
    patch: str


@dataclass(frozen=True)
class HunkLine:
    kind: str
    text: str


@dataclass(frozen=True)
class Hunk:
    old_start: int
    old_count: int
    new_start: int
    new_count: int
    lines: tuple[HunkLine, ...]


@dataclass(frozen=True)
class FilePatch:
    path: str
    hunks: tuple[Hunk, ...]


@dataclass(frozen=True)
class AppliedPatch:
    new_lines: tuple[str, ...]
    removed_lines: tuple[int, ...]
    added_lines: tuple[int, ...]
    offsets: tuple[int, ...]


@dataclass(frozen=True)
class FunctionSpan:
    qualname: str
    occurrence: int
    name: str
    def_line: int
    start_line: int
    end_line: int


@dataclass(frozen=True)
class FunctionGold:
    functions: tuple[FunctionSpan, ...]
    removed_outside: int
    added_outside: int


@dataclass(frozen=True)
class GraphNode:
    node_id: str
    node_type: str
    name: str
    start_line: int


@dataclass(frozen=True)
class OrderEntry:
    position: int
    instance_id: str
    repo: str
    base_commit: str
    created_at: str
    order_key: str


@dataclass(frozen=True)
class OrderFile:
    schema_version: int
    seed: int
    instances_file: str
    instances_sha256: str
    rule: str
    repo_sizes: dict[str, int]
    entries: list[OrderEntry]


@dataclass(frozen=True)
class TaskRepo:
    name: str
    path: str
    commit: str
    languages: list[str]


@dataclass(frozen=True)
class TaskQuery:
    type: str
    text: str


@dataclass(frozen=True)
class FileExpected:
    file_paths: list[str]
    min_recall: int


@dataclass(frozen=True)
class NodeExpected:
    node_ids: list[str]
    min_recall: int


@dataclass(frozen=True)
class FileTask:
    id: str
    repo: TaskRepo
    query: TaskQuery
    expected: FileExpected
    split: str


@dataclass(frozen=True)
class NodeTask:
    id: str
    repo: TaskRepo
    query: TaskQuery
    expected: NodeExpected
    split: str


@dataclass(frozen=True)
class FileTaskFile:
    schema_version: int
    tasks: list[FileTask]


@dataclass(frozen=True)
class NodeTaskFile:
    schema_version: int
    tasks: list[NodeTask]


@dataclass(frozen=True)
class GoldFunctionRecord:
    qualname: str
    occurrence: int
    def_line: int
    start_line: int
    end_line: int
    node_id: str


@dataclass(frozen=True)
class GoldRecord:
    schema_version: int
    instance_id: str
    repo: str
    base_commit: str
    file_path: str
    hunk_offsets: list[int]
    removed_lines: list[int]
    added_lines: list[int]
    removed_outside_functions: int
    added_outside_functions: int
    functions: list[GoldFunctionRecord]


@dataclass(frozen=True)
class GoldCheckEntry:
    instance_id: str
    file_path: str
    hunk_offsets: list[int]
    functions: list[str]
    removed_outside_functions: int
    added_outside_functions: int


@dataclass(frozen=True)
class GoldCheckReport:
    schema_version: int
    instances: int
    with_function_gold: int
    entries: list[GoldCheckEntry]


def load_instances(path: Path) -> tuple[Instance, ...]:
    """JSON Lines örneklerini okur; kimlikler benzersiz olmalı."""
    # `str.splitlines` U+2028 gibi karakterlerde de böler; JSON satırları yalnız \n ile ayrılır.
    lines = path.read_bytes().decode("utf-8").split("\n")
    rows = lines[:-1] if lines and lines[-1] == "" else lines
    instances = tuple(
        _instance(row, f"{path}:{number}") for number, row in enumerate(rows, start=1)
    )
    ids = [instance.instance_id for instance in instances]
    duplicates = sorted(key for key, count in Counter(ids).items() if count > 1)
    if duplicates:
        raise SweBenchError(f"{path}: duplicate instance ids {duplicates}")
    return instances


def find_instance(instances: Sequence[Instance], instance_id: str) -> Instance:
    """Kimliği verilen örnek; yoksa hata."""
    for instance in instances:
        if instance.instance_id == instance_id:
            return instance
    raise SweBenchError(f"instance '{instance_id}' is not in the instances file")


def repo_name(repo: str) -> str:
    """`sahip/ad` biçimindeki deponun kısa adı; klon dizini ve görevlerin `repo.name` alanı."""
    owner, separator, name = repo.partition("/")
    if not separator or not owner or not name or "/" in name:
        raise SweBenchError(f"repo must be OWNER/NAME, got {repo!r}")
    return name


def order_key(seed: int, instance_id: str) -> str:
    """Depo içi sıranın anahtarı."""
    return hashlib.sha256(f"{seed}:{instance_id}".encode()).hexdigest()


def selection_order(instances: Sequence[Instance], seed: int) -> tuple[Instance, ...]:
    """Tüm örneklerin ön kayıtlı sırası; alt küme bunun ön ekidir."""
    by_repo: dict[str, list[Instance]] = {}
    for instance in instances:
        by_repo.setdefault(instance.repo, []).append(instance)
    queues = {
        repo: sorted(members, key=lambda member: order_key(seed, member.instance_id))
        for repo, members in by_repo.items()
    }
    order = [queues[repo][0] for repo in sorted(queues)]
    seats = dict.fromkeys(queues, 1)
    while len(order) < len(instances):
        open_repos = [repo for repo in sorted(queues) if seats[repo] < len(queues[repo])]
        chosen = min(
            open_repos,
            key=lambda repo: (-Fraction(len(queues[repo]), 2 * seats[repo] + 1), repo),
        )
        order.append(queues[chosen][seats[chosen]])
        seats[chosen] += 1
    return tuple(order)


def parse_patch(patch: str, context: str) -> FilePatch:
    """Tek dosyalık git yamasını çözer; uygunluk koşulu sağlanmıyorsa `PatchError`."""
    lines = _split_lines(patch)
    headers = [index for index, line in enumerate(lines) if line.startswith("diff --git ")]
    if headers != [0]:
        raise PatchError(f"{context}: expected one file diff at the start, found {len(headers)}")
    match = FILE_HEADER.match(lines[0])
    if match is None or match["old"] != match["new"]:
        raise PatchError(f"{context}: renamed or malformed diff header {lines[0]!r}")
    path = match["old"]
    if not path.endswith(PYTHON_SUFFIX):
        raise PatchError(f"{context}: patched file {path} is not a Python file")
    index = 1
    while index < len(lines) and not lines[index].startswith("--- "):
        if not lines[index].startswith(ALLOWED_EXTENDED_HEADERS):
            raise PatchError(f"{context}: unsupported diff header {lines[index]!r}")
        index += 1
    if lines[index : index + 2] != [f"--- a/{path}", f"+++ b/{path}"]:
        raise PatchError(f"{context}: {path} is not modified in place: {lines[index : index + 2]}")
    index += 2
    hunks: list[Hunk] = []
    while index < len(lines):
        hunk, index = _parse_hunk(lines, index, context)
        hunks.append(hunk)
    if not hunks:
        raise PatchError(f"{context}: patch has no hunks")
    return FilePatch(path=path, hunks=tuple(hunks))


def source_lines(text: str, context: str) -> tuple[str, ...]:
    """Dosyanın satırları git'in numaralandırmasıyla (yalnız \\n ayırır)."""
    if LONE_CARRIAGE_RETURN.search(text):
        raise GoldError(f"{context}: lone carriage return; Python and git number lines differently")
    return tuple(_split_lines(text))


def apply_patch(old_lines: Sequence[str], patch: FilePatch, context: str) -> AppliedPatch:
    """Yamayı eski satırlara uygular; hunk eşleşmezse `PatchError`.

    Hunk'ın eski tarafı (bağlam ve silinen satırlar) dosyayla tam eşleşmelidir; bağlam
    gevşetilmez. Başlıktaki konumda eşleşmezse `git apply` gibi en yakın konum aranır (eşit
    uzaklıkta önce ileri) ve önceki hunk'ın kayması sonrakinin beklenen konumuna eklenir.
    """
    new_lines: list[str] = []
    removed: list[int] = []
    added: list[int] = []
    offsets: list[int] = []
    cursor = 0
    offset = 0
    for number, hunk in enumerate(patch.hunks, start=1):
        stated = hunk.old_start - 1 if hunk.old_count > 0 else hunk.old_start
        old_side = tuple(line.text for line in hunk.lines if line.kind != "+")
        start = locate_hunk(old_lines, old_side, stated + offset, cursor)
        if start is None:
            raise PatchError(
                f"{context}: hunk {number} (old line {hunk.old_start}) matches no position "
                f"at or after line {cursor + 1}"
            )
        offset = start - stated
        offsets.append(offset)
        new_lines.extend(old_lines[cursor:start])
        position = start
        for line in hunk.lines:
            if line.kind == "+":
                new_lines.append(line.text)
                added.append(len(new_lines))
                continue
            position += 1
            if line.kind == "-":
                removed.append(position)
            else:
                new_lines.append(line.text)
        cursor = position
    new_lines.extend(old_lines[cursor:])
    return AppliedPatch(
        new_lines=tuple(new_lines),
        removed_lines=tuple(removed),
        added_lines=tuple(added),
        offsets=tuple(offsets),
    )


def locate_hunk(
    old_lines: Sequence[str], old_side: Sequence[str], expected: int, cursor: int
) -> int | None:
    """Eski tarafın eşleştiği, `expected` konumuna en yakın başlangıç (0 tabanlı)."""
    last = len(old_lines) - len(old_side)
    if last < cursor:
        return None
    side = tuple(old_side)
    for distance in range(max(abs(expected - cursor), abs(last - expected)) + 1):
        for start in dict.fromkeys((expected + distance, expected - distance)):
            if cursor <= start <= last and tuple(old_lines[start : start + len(side)]) == side:
                return start
    return None


def function_spans(source: str, context: str) -> tuple[FunctionSpan, ...]:
    """Dosyadaki tüm fonksiyon tanımları (iç içe olanlar dahil) kaynak sırasıyla."""
    try:
        with warnings.catch_warnings():
            # Eski kaynaklardaki geçersiz kaçış dizileri yalnız uyarıdır; ayrıştırmayı etkilemez.
            warnings.simplefilter("ignore", SyntaxWarning)
            tree = ast.parse(source)
    except SyntaxError as error:
        raise GoldError(
            f"{context}: Python parse failed at line {error.lineno}: {error.msg}"
        ) from error
    definitions = sorted(
        _definitions(tree, (), context), key=lambda item: (item.def_line, item.column)
    )
    counts: Counter[str] = Counter()
    spans: list[FunctionSpan] = []
    for definition in definitions:
        spans.append(
            FunctionSpan(
                qualname=definition.qualname,
                occurrence=counts[definition.qualname],
                name=definition.name,
                def_line=definition.def_line,
                start_line=definition.start_line,
                end_line=definition.end_line,
            )
        )
        counts[definition.qualname] += 1
    return tuple(spans)


def enclosing_spans(spans: Sequence[FunctionSpan], line: int) -> tuple[FunctionSpan, ...]:
    """Satırı içeren fonksiyonlar, en içteki önce."""
    return tuple(
        sorted(
            (span for span in spans if span.start_line <= line <= span.end_line),
            key=lambda span: span.start_line,
            reverse=True,
        )
    )


def changed_functions(
    old_spans: Sequence[FunctionSpan],
    new_spans: Sequence[FunctionSpan],
    applied: AppliedPatch,
    context: str,
) -> FunctionGold:
    """Yamanın değiştirdiği eski fonksiyonlar ve hiçbir fonksiyona düşmeyen satır sayıları."""
    old_by_key = {(span.qualname, span.occurrence): span for span in old_spans}
    old_counts = Counter(span.qualname for span in old_spans)
    new_counts = Counter(span.qualname for span in new_spans)
    selected: dict[tuple[str, int], FunctionSpan] = {}
    removed_outside = 0
    for line in applied.removed_lines:
        enclosing = enclosing_spans(old_spans, line)
        if not enclosing:
            removed_outside += 1
            continue
        selected[(enclosing[0].qualname, enclosing[0].occurrence)] = enclosing[0]
    added_outside = 0
    for line in applied.added_lines:
        match = _existing_enclosing(
            enclosing_spans(new_spans, line), old_by_key, old_counts, new_counts, context
        )
        if match is None:
            added_outside += 1
            continue
        selected[(match.qualname, match.occurrence)] = match
    return FunctionGold(
        functions=tuple(sorted(selected.values(), key=lambda span: (span.def_line, span.qualname))),
        removed_outside=removed_outside,
        added_outside=added_outside,
    )


def instance_function_gold(old_text: str, applied: AppliedPatch, context: str) -> FunctionGold:
    """Eski dosya metninden ve ona uygulanmış yamadan fonksiyon düzeyindeki değişiklikler."""
    new_text = "\n".join(applied.new_lines) + "\n"
    source_lines(new_text, f"{context} new")
    return changed_functions(
        function_spans(old_text, f"{context} old"),
        function_spans(new_text, f"{context} new"),
        applied,
        context,
    )


def graph_nodes(graph: dict[str, JsonValue]) -> tuple[GraphNode, ...]:
    """`ccm_graph.json` düğümlerinin eşleme için gereken alanları."""
    nodes: list[GraphNode] = []
    for value in as_list(graph.get("nodes"), "nodes", SweBenchError):
        node = as_object(value, "node", SweBenchError)
        node_id = as_str(node.get("id"), "node.id", SweBenchError)
        nodes.append(
            GraphNode(
                node_id=node_id,
                node_type=as_str(node.get("node_type"), f"{node_id}.node_type", SweBenchError),
                name=as_str(node.get("name"), f"{node_id}.name", SweBenchError),
                start_line=as_int(node.get("start_line"), f"{node_id}.start_line", SweBenchError),
            )
        )
    return tuple(nodes)


def has_file_node(nodes: Sequence[GraphNode], file_path: str) -> bool:
    """Dosya CCM indeksinde `File` düğümü olarak var mı."""
    target = f"./{file_path}"
    return any(node.node_type == CCM_FILE_TYPE and node.node_id == target for node in nodes)


def function_node_ids(
    functions: Sequence[FunctionSpan], nodes: Sequence[GraphNode], file_path: str, context: str
) -> tuple[str, ...]:
    """Eski fonksiyonların CCM `Function` düğüm kimlikleri; her fonksiyona tam bir düğüm."""
    prefix = f"./{file_path}:"
    candidates = [
        node
        for node in nodes
        if node.node_type == CCM_FUNCTION_TYPE and node.node_id.startswith(prefix)
    ]
    node_ids: list[str] = []
    for span in functions:
        matches = [
            node
            for node in candidates
            if node.name == span.name and node.start_line == span.def_line
        ]
        if len(matches) != 1:
            raise GoldError(
                f"{context}: {span.qualname} (def line {span.def_line}) matches "
                f"{len(matches)} CCM Function nodes in {file_path}"
            )
        node_ids.append(matches[0].node_id)
    return tuple(node_ids)


def task_repo(instance: Instance, repo_root: Path) -> TaskRepo:
    return TaskRepo(
        name=repo_name(instance.repo),
        path=str(repo_root.resolve()),
        commit=instance.base_commit,
        languages=list(TASK_LANGUAGES),
    )


def file_task_file(instance: Instance, repo_root: Path, file_path: str) -> FileTaskFile:
    """Dosya düzeyinin görev dosyası: oikea vastaus yamanın değiştirdiği dosya."""
    return FileTaskFile(
        schema_version=TASK_SCHEMA_VERSION,
        tasks=[
            FileTask(
                id=instance.instance_id,
                repo=task_repo(instance, repo_root),
                query=TaskQuery(type=QUERY_TYPE, text=instance.problem_statement),
                expected=FileExpected(file_paths=[file_path], min_recall=MIN_RECALL),
                split=TEST_SPLIT,
            )
        ],
    )


def node_task_file(instance: Instance, repo_root: Path, node_ids: Sequence[str]) -> NodeTaskFile:
    """Fonksiyon düzeyinin görev dosyası: oikeat vastaukset değişen fonksiyonların düğümleri."""
    return NodeTaskFile(
        schema_version=TASK_SCHEMA_VERSION,
        tasks=[
            NodeTask(
                id=instance.instance_id,
                repo=task_repo(instance, repo_root),
                query=TaskQuery(type=QUERY_TYPE, text=instance.problem_statement),
                expected=NodeExpected(node_ids=list(node_ids), min_recall=MIN_RECALL),
                split=TEST_SPLIT,
            )
        ],
    )


def file_sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def git_show(repo_root: Path, commit: str, path: str) -> str:
    """Dosyanın commit'teki içeriği UTF-8 metin olarak."""
    try:
        completed = subprocess.run(
            ["git", "-C", str(repo_root), "show", f"{commit}:{path}"],
            capture_output=True,
            check=True,
        )
    except subprocess.CalledProcessError as error:
        raise SweBenchError(
            f"git show {commit}:{path} in {repo_root} failed with {error.returncode}: "
            f"{error.stderr.decode(errors='replace').strip()}"
        ) from error
    try:
        return completed.stdout.decode("utf-8")
    except UnicodeDecodeError as error:
        raise GoldError(f"{repo_root}/{path} at {commit} is not UTF-8: {error}") from error


def write_json(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")


def run_order(args: argparse.Namespace) -> None:
    instances_path: Path = args.instances
    seed: int = args.seed
    instances = load_instances(instances_path)
    for instance in instances:
        parse_patch(instance.patch, instance.instance_id)
    order = selection_order(instances, seed)
    sizes = Counter(instance.repo for instance in instances)
    result = OrderFile(
        schema_version=SCHEMA_VERSION,
        seed=seed,
        instances_file=instances_path.name,
        instances_sha256=file_sha256(instances_path),
        rule=ORDER_RULE,
        repo_sizes=dict(sorted(sizes.items())),
        entries=[
            OrderEntry(
                position=position,
                instance_id=instance.instance_id,
                repo=instance.repo,
                base_commit=instance.base_commit,
                created_at=instance.created_at,
                order_key=order_key(seed, instance.instance_id),
            )
            for position, instance in enumerate(order, start=1)
        ],
    )
    write_json(args.out, asdict(result))
    print(f"order: {len(order)} instances from {len(sizes)} repos -> {args.out}")


def run_gold_check(args: argparse.Namespace) -> None:
    repos_dir: Path = args.repos_dir
    entries: list[GoldCheckEntry] = []
    for instance in load_instances(args.instances):
        patch = parse_patch(instance.patch, instance.instance_id)
        old_text = git_show(repos_dir / repo_name(instance.repo), instance.base_commit, patch.path)
        applied = apply_patch(
            source_lines(old_text, instance.instance_id), patch, instance.instance_id
        )
        gold = instance_function_gold(old_text, applied, instance.instance_id)
        entries.append(
            GoldCheckEntry(
                instance_id=instance.instance_id,
                file_path=patch.path,
                hunk_offsets=list(applied.offsets),
                functions=[span.qualname for span in gold.functions],
                removed_outside_functions=gold.removed_outside,
                added_outside_functions=gold.added_outside,
            )
        )
    report = GoldCheckReport(
        schema_version=SCHEMA_VERSION,
        instances=len(entries),
        with_function_gold=sum(1 for entry in entries if entry.functions),
        entries=entries,
    )
    write_json(args.out, asdict(report))
    print(f"gold-check: {report.with_function_gold}/{report.instances} with function gold")


def run_tasks(args: argparse.Namespace) -> None:
    repo_root: Path = args.repo_root
    out_dir: Path = args.out_dir
    instance = find_instance(load_instances(args.instances), args.instance_id)
    _generation_id, directory = generation_dir(repo_root)
    manifest = as_object(read_json(directory / "ccm_manifest.json"), "manifest", SweBenchError)
    indexed_commit = as_str(manifest.get("indexed_commit"), "indexed_commit", SweBenchError)
    if indexed_commit != instance.base_commit:
        raise SweBenchError(
            f"{instance.instance_id}: index is at {indexed_commit}, "
            f"base commit is {instance.base_commit}"
        )
    patch = parse_patch(instance.patch, instance.instance_id)
    # Satır sonları çevrilmeden okunur; yama satırları \r karakterlerini olduğu gibi taşır.
    old_text = (repo_root / patch.path).read_bytes().decode("utf-8")
    applied = apply_patch(source_lines(old_text, instance.instance_id), patch, instance.instance_id)
    gold = instance_function_gold(old_text, applied, instance.instance_id)
    nodes = graph_nodes(as_object(read_json(directory / "ccm_graph.json"), "graph", SweBenchError))
    if not has_file_node(nodes, patch.path):
        raise GoldError(f"{instance.instance_id}: {patch.path} is not in the CCM index")
    node_ids = function_node_ids(gold.functions, nodes, patch.path, instance.instance_id)
    out_dir.mkdir(parents=True, exist_ok=True)
    write_json(
        out_dir / f"{instance.instance_id}.file.json",
        asdict(file_task_file(instance, repo_root, patch.path)),
    )
    if node_ids:
        write_json(
            out_dir / f"{instance.instance_id}.function.json",
            asdict(node_task_file(instance, repo_root, node_ids)),
        )
    record = GoldRecord(
        schema_version=SCHEMA_VERSION,
        instance_id=instance.instance_id,
        repo=instance.repo,
        base_commit=instance.base_commit,
        file_path=patch.path,
        hunk_offsets=list(applied.offsets),
        removed_lines=list(applied.removed_lines),
        added_lines=list(applied.added_lines),
        removed_outside_functions=gold.removed_outside,
        added_outside_functions=gold.added_outside,
        functions=[
            GoldFunctionRecord(
                qualname=span.qualname,
                occurrence=span.occurrence,
                def_line=span.def_line,
                start_line=span.start_line,
                end_line=span.end_line,
                node_id=node_id,
            )
            for span, node_id in zip(gold.functions, node_ids, strict=True)
        ],
    )
    write_json(out_dir / f"{instance.instance_id}.gold.json", asdict(record))
    print(f"{instance.instance_id}: file {patch.path}, functions {len(node_ids)}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    commands = parser.add_subparsers(dest="command", required=True)
    order = commands.add_parser("order", help="write the pre-registered selection order")
    order.add_argument("--instances", type=Path, required=True)
    order.add_argument("--seed", type=int, required=True)
    order.add_argument("--out", type=Path, required=True)
    order.set_defaults(handler=run_order)
    check = commands.add_parser("gold-check", help="compute function gold for all instances")
    check.add_argument("--instances", type=Path, required=True)
    check.add_argument("--repos-dir", type=Path, required=True)
    check.add_argument("--out", type=Path, required=True)
    check.set_defaults(handler=run_gold_check)
    tasks = commands.add_parser("tasks", help="write the task files of one indexed instance")
    tasks.add_argument("--instances", type=Path, required=True)
    tasks.add_argument("--instance-id", required=True)
    tasks.add_argument("--repo-root", type=Path, required=True)
    tasks.add_argument("--out-dir", type=Path, required=True)
    tasks.set_defaults(handler=run_tasks)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    args.handler(args)


def _instance(row: str, context: str) -> Instance:
    try:
        loaded: JsonValue = json.loads(row)
    except json.JSONDecodeError as error:
        raise SweBenchError(f"{context} is not valid JSON: {error}") from error
    value = as_object(loaded, context, SweBenchError)
    instance_id = as_str(value.get("instance_id"), f"{context}.instance_id", SweBenchError)
    return Instance(
        instance_id=instance_id,
        repo=as_str(value.get("repo"), f"{instance_id}.repo", SweBenchError),
        base_commit=as_str(value.get("base_commit"), f"{instance_id}.base_commit", SweBenchError),
        created_at=as_str(value.get("created_at"), f"{instance_id}.created_at", SweBenchError),
        version=as_str(value.get("version"), f"{instance_id}.version", SweBenchError),
        problem_statement=as_str(
            value.get("problem_statement"), f"{instance_id}.problem_statement", SweBenchError
        ),
        patch=as_str(value.get("patch"), f"{instance_id}.patch", SweBenchError),
    )


@dataclass(frozen=True)
class _Definition:
    qualname: str
    name: str
    def_line: int
    column: int
    start_line: int
    end_line: int


def _definitions(node: ast.AST, scope: tuple[str, ...], context: str) -> list[_Definition]:
    found: list[_Definition] = []
    for child in ast.iter_child_nodes(node):
        if isinstance(child, ast.FunctionDef | ast.AsyncFunctionDef):
            if child.end_lineno is None:
                raise GoldError(f"{context}: {child.name} at line {child.lineno} has no end line")
            found.append(
                _Definition(
                    qualname=".".join((*scope, child.name)),
                    name=child.name,
                    def_line=child.lineno,
                    column=child.col_offset,
                    start_line=min([child.lineno, *(item.lineno for item in child.decorator_list)]),
                    end_line=child.end_lineno,
                )
            )
            found.extend(_definitions(child, (*scope, child.name), context))
        elif isinstance(child, ast.ClassDef):
            found.extend(_definitions(child, (*scope, child.name), context))
        else:
            found.extend(_definitions(child, scope, context))
    return found


def _existing_enclosing(
    enclosing: Sequence[FunctionSpan],
    old_by_key: dict[tuple[str, int], FunctionSpan],
    old_counts: Counter[str],
    new_counts: Counter[str],
    context: str,
) -> FunctionSpan | None:
    for span in enclosing:
        if old_counts[span.qualname] == 0:
            continue
        if old_counts[span.qualname] != new_counts[span.qualname]:
            raise GoldError(
                f"{context}: {span.qualname} is defined {old_counts[span.qualname]} times before "
                f"and {new_counts[span.qualname]} times after the patch"
            )
        return old_by_key[(span.qualname, span.occurrence)]
    return None


def _split_lines(text: str) -> list[str]:
    lines = text.split("\n")
    return lines[:-1] if lines and lines[-1] == "" else lines


def _hunk_count(raw: str | None) -> int:
    # Birleşik diff biçimi: sayı yazılmamışsa aralık tek satırdır.
    return 1 if raw is None else int(raw)


def _parse_hunk(lines: Sequence[str], index: int, context: str) -> tuple[Hunk, int]:
    match = HUNK_HEADER.match(lines[index])
    if match is None:
        raise PatchError(f"{context}: line {index + 1} is not a hunk header: {lines[index]!r}")
    old_count = _hunk_count(match["old_count"])
    new_count = _hunk_count(match["new_count"])
    old_left, new_left = old_count, new_count
    body: list[HunkLine] = []
    position = index + 1
    while old_left > 0 or new_left > 0:
        if position >= len(lines):
            raise PatchError(f"{context}: hunk at line {index + 1} is truncated")
        line = lines[position]
        position += 1
        if line.startswith(NO_NEWLINE_MARKER):
            continue
        kind = line[:1]
        if kind not in {" ", "-", "+"}:
            raise PatchError(f"{context}: unexpected hunk line {position}: {line!r}")
        old_left -= 0 if kind == "+" else 1
        new_left -= 0 if kind == "-" else 1
        if old_left < 0 or new_left < 0:
            raise PatchError(f"{context}: hunk at line {index + 1} exceeds its line counts")
        body.append(HunkLine(kind=kind, text=line[1:]))
    while position < len(lines) and lines[position].startswith(NO_NEWLINE_MARKER):
        position += 1
    hunk = Hunk(
        old_start=int(match["old_start"]),
        old_count=old_count,
        new_start=int(match["new_start"]),
        new_count=new_count,
        lines=tuple(body),
    )
    return hunk, position


if __name__ == "__main__":
    main()
