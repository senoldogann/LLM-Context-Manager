"""Ön-kayıtlı senaryolar: çalışma kopyası, betikli düzenleme ve inşa gereği bilinen kâhin."""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from enum import StrEnum
from pathlib import Path
from typing import TypeAlias

from freshness import repos
from freshness.classify import CallersExpect, ExistsExpect, NodeExpect

TARGET = "ccmb_target"
RENAMED = "ccmb_target_renamed"
CALLER_A = "ccmb_caller_a"
CALLER_B = "ccmb_caller_b"
CALLER_C = "ccmb_caller_c"
GENERATED_MODULE = "pkg.target"
CALLER_A_FILE = "pkg/caller_a.py"
CALLER_B_FILE = "pkg/caller_b.py"
MOVED_CALLER_B_FILE = "pkg/relocated/caller_b.py"
NESTED_CALLER_B_FILE = f"{repos.NESTED_ROOT}/inner_pkg/caller_b.py"
BULK_COUNT = 20
STORM_COUNT = 50
# S5: beş satır eklendikten sonra 16. satır `shifted_fn` (15-17) içinde kalır.
SHIFT_PROBE_LINE = 16


class UnknownScenarioError(ValueError):
    """İstenen senaryo kimliği ön-kayıtta yok."""


class PhaseKind(StrEnum):
    """Bir aşamanın hangi sınıflandırma kuralıyla değerlendirileceği."""

    CHANGE = "change"
    PRESERVE = "preserve"


@dataclass(frozen=True)
class CallersCheck:
    """`callers(symbol)` sorusu ve düzenleme sonrası/öncesi beklenen durumlar."""

    symbol: str
    post: CallersExpect
    pre: CallersExpect


@dataclass(frozen=True)
class ExistsCheck:
    """`exists(symbol)` sorusu ve beklenen durumlar."""

    symbol: str
    post: ExistsExpect
    pre: ExistsExpect


@dataclass(frozen=True)
class NodeCheck:
    """`node_at(file, line)` sorusu ve beklenen durumlar."""

    file: str
    line: int
    post: NodeExpect
    pre: NodeExpect


Check: TypeAlias = CallersCheck | ExistsCheck | NodeCheck


@dataclass(frozen=True)
class Phase:
    """Tek bir betikli düzenleme ve ardından her probda sorulan sorular."""

    name: str
    kind: PhaseKind
    edit: Callable[[Path], None]
    checks: tuple[Check, ...]


@dataclass(frozen=True)
class Scenario:
    """Ön-kayıtlı bir senaryonun tüm tanımı."""

    scenario_id: str
    repo_name: str
    auto_refresh: bool
    reindex_after_edit: bool
    ready_symbol: str
    build: Callable[[Path], None]
    phases: tuple[Phase, ...]


TARGET_MISSING = CallersExpect(target_found=False, present=frozenset(), absent=frozenset())


def callers_state(present: frozenset[str], absent: frozenset[str]) -> CallersExpect:
    """Hedefin bulunduğu bir `callers` durumu."""
    return CallersExpect(target_found=True, present=present, absent=absent)


def added_caller_check(added: str) -> CallersCheck:
    """S1-add biçimi: sonra `added` listede; önce hedef var, a listede, `added` yok."""
    return CallersCheck(
        symbol=TARGET,
        post=callers_state(frozenset({added}), frozenset()),
        pre=callers_state(frozenset({CALLER_A}), frozenset({added})),
    )


def write_edit(relative_file: str, text: str) -> Callable[[Path], None]:
    """Dosyayı verilen metinle (yeniden) yazan düzenleme."""

    def edit(root: Path) -> None:
        repos.write_file(root / relative_file, text)

    return edit


def append_caller_edit(relative_file: str, caller: str, module: str) -> Callable[[Path], None]:
    """Var olan bir modülün sonuna hedefi çağıran fonksiyon ekleyen düzenleme."""

    def edit(root: Path) -> None:
        repos.append_text(root / relative_file, repos.caller_source(caller, module, TARGET))

    return edit


def remove_call_edit(relative_file: str, module: str) -> Callable[[Path], None]:
    """`ccmb_caller_a` gövdesini (import ve çağrı) `return 0` ile değiştiren düzenleme."""

    def edit(root: Path) -> None:
        repos.replace_once(
            root / relative_file,
            repos.caller_source(CALLER_A, module, TARGET),
            repos.idle_source(CALLER_A),
        )

    return edit


def rename_target_edit(corpus: repos.CorpusRepo) -> Callable[[Path], None]:
    """Hedefi tanımında, sonra `ccmb_caller_a` içinde yeniden adlandıran düzenleme."""

    def edit(root: Path) -> None:
        repos.replace_once(root / corpus.target_file, f"def {TARGET}():", f"def {RENAMED}():")
        repos.replace_once(
            root / corpus.caller_a_file,
            repos.caller_source(CALLER_A, corpus.target_module, TARGET),
            repos.caller_source(CALLER_A, corpus.target_module, RENAMED),
        )

    return edit


def checkout_edit(branch: str) -> Callable[[Path], None]:
    """`git checkout` ile dal değiştiren düzenleme."""

    def edit(root: Path) -> None:
        repos.run_git(root, ("checkout", "-q", branch))

    return edit


def rewrite_bulk_edit(count: int, prefix: str) -> Callable[[Path], None]:
    """Toplu modüllerin hepsini tek geçişte hedefi çağıracak biçimde yazan düzenleme."""

    def edit(root: Path) -> None:
        repos.rewrite_bulk(root, count, prefix)

    return edit


def insert_top_edit(relative_file: str, text: str) -> Callable[[Path], None]:
    """Dosyanın başına satır ekleyen düzenleme."""

    def edit(root: Path) -> None:
        repos.insert_top(root / relative_file, text)

    return edit


def delete_edit(relative_file: str) -> Callable[[Path], None]:
    """Dosyayı silen düzenleme."""

    def edit(root: Path) -> None:
        repos.remove_path(root / relative_file)

    return edit


def move_edit(source: str, destination: str) -> Callable[[Path], None]:
    """Dosyayı yeni bir yola taşıyan düzenleme."""

    def edit(root: Path) -> None:
        repos.move_path(root / source, root / destination)

    return edit


def corpus_builder(corpus: repos.CorpusRepo) -> Callable[[Path], None]:
    """Gerçek repodan hedef ve ilk çağıran enjekte edilmiş çalışma kopyası üreticisi."""

    def build(root: Path) -> None:
        repos.build_corpus_copy(corpus, root)

    return build


def bulk_builder(count: int, prefix: str) -> Callable[[Path], None]:
    """`count` toplu modüllü küçük repo üreticisi."""

    def build(root: Path) -> None:
        repos.build_bulk_repo(root, count, prefix)

    return build


def one_phase_scenario(
    scenario_id: str,
    repo_name: str,
    build: Callable[[Path], None],
    edit: Callable[[Path], None],
    checks: tuple[Check, ...],
) -> Scenario:
    """Otomatik yenilemenin açık olduğu tek aşamalı senaryo."""
    return Scenario(
        scenario_id=scenario_id,
        repo_name=repo_name,
        auto_refresh=True,
        reindex_after_edit=False,
        ready_symbol=TARGET,
        build=build,
        phases=(Phase(name="edit", kind=PhaseKind.CHANGE, edit=edit, checks=checks),),
    )


def corpus_repos(corpus_dir: Path) -> tuple[repos.CorpusRepo, ...]:
    """S1'in gerçek repoları (corpus.json'daki sabit sürümler) ve enjeksiyon noktaları."""
    return (
        repos.CorpusRepo(
            name="flask",
            source=corpus_dir / "flask",
            target_file="src/flask/helpers.py",
            target_module="flask.helpers",
            caller_a_file="src/flask/app.py",
            caller_b_file="src/flask/cli.py",
        ),
        repos.CorpusRepo(
            name="django",
            source=corpus_dir / "django",
            target_file="django/utils/text.py",
            target_module="django.utils.text",
            caller_a_file="django/utils/html.py",
            caller_b_file="django/utils/functional.py",
        ),
    )


def s1_scenarios(corpus: repos.CorpusRepo) -> tuple[Scenario, ...]:
    """Bir gerçek repoda S1-add, S1-remove ve S1-rename."""
    build = corpus_builder(corpus)
    caller_a_listed = callers_state(frozenset({CALLER_A}), frozenset())
    return (
        one_phase_scenario(
            "S1-add",
            corpus.name,
            build,
            append_caller_edit(corpus.caller_b_file, CALLER_B, corpus.target_module),
            (added_caller_check(CALLER_B),),
        ),
        one_phase_scenario(
            "S1-remove",
            corpus.name,
            build,
            remove_call_edit(corpus.caller_a_file, corpus.target_module),
            (
                CallersCheck(
                    symbol=TARGET,
                    post=callers_state(frozenset(), frozenset({CALLER_A})),
                    pre=caller_a_listed,
                ),
            ),
        ),
        one_phase_scenario(
            "S1-rename",
            corpus.name,
            build,
            rename_target_edit(corpus),
            (
                CallersCheck(symbol=RENAMED, post=caller_a_listed, pre=TARGET_MISSING),
                CallersCheck(symbol=TARGET, post=TARGET_MISSING, pre=caller_a_listed),
            ),
        ),
    )


def generated_scenarios() -> tuple[Scenario, ...]:
    """Amaca özel küçük repolarda S2-S8; değer kanıtı değil, bilinen durumda davranış."""
    add_b = write_edit(CALLER_B_FILE, repos.module_with_caller(CALLER_B, GENERATED_MODULE, TARGET))
    a_listed = callers_state(frozenset({CALLER_A}), frozenset())
    bulk = repos.bulk_callers(BULK_COUNT, "bulk")
    storm = repos.bulk_callers(STORM_COUNT, "storm")
    return (
        Scenario(
            scenario_id="S2",
            repo_name="generated",
            auto_refresh=False,
            reindex_after_edit=True,
            ready_symbol=TARGET,
            build=repos.build_git_repo,
            phases=(
                Phase(
                    name="edit",
                    kind=PhaseKind.CHANGE,
                    edit=add_b,
                    checks=(added_caller_check(CALLER_B),),
                ),
            ),
        ),
        one_phase_scenario(
            "S3-branch",
            "generated",
            repos.build_branch_repo,
            checkout_edit("feature"),
            (
                CallersCheck(
                    symbol=TARGET,
                    post=callers_state(frozenset({CALLER_B}), frozenset({CALLER_A})),
                    pre=callers_state(frozenset({CALLER_A}), frozenset({CALLER_B})),
                ),
            ),
        ),
        one_phase_scenario(
            "S3-bulk",
            "generated",
            bulk_builder(BULK_COUNT, "bulk"),
            rewrite_bulk_edit(BULK_COUNT, "bulk"),
            (
                CallersCheck(
                    symbol=TARGET,
                    post=callers_state(bulk, frozenset()),
                    pre=callers_state(frozenset({CALLER_A}), bulk),
                ),
            ),
        ),
        one_phase_scenario(
            "S4-nested",
            "generated",
            repos.build_nested_repo,
            write_edit(
                NESTED_CALLER_B_FILE,
                repos.module_with_caller(CALLER_B, repos.NESTED_MODULE, TARGET),
            ),
            (added_caller_check(CALLER_B),),
        ),
        one_phase_scenario(
            "S4-nogit",
            "generated",
            repos.build_plain_repo,
            add_b,
            (added_caller_check(CALLER_B),),
        ),
        one_phase_scenario(
            "S5",
            "generated",
            repos.build_shift_repo,
            insert_top_edit(repos.SHIFT_FILE, repos.SHIFT_INSERT),
            (
                NodeCheck(
                    file=repos.SHIFT_FILE,
                    line=SHIFT_PROBE_LINE,
                    post=NodeExpect(name="shifted_fn", start_line=15, end_line=17),
                    pre=NodeExpect(name="occupant_fn", start_line=15, end_line=17),
                ),
            ),
        ),
        one_phase_scenario(
            "S6",
            "generated",
            bulk_builder(STORM_COUNT, "storm"),
            rewrite_bulk_edit(STORM_COUNT, "storm"),
            (
                CallersCheck(
                    symbol=TARGET,
                    post=callers_state(storm, frozenset()),
                    pre=callers_state(frozenset({CALLER_A}), storm),
                ),
            ),
        ),
        Scenario(
            scenario_id="S7",
            repo_name="generated",
            auto_refresh=True,
            reindex_after_edit=False,
            ready_symbol=TARGET,
            build=repos.build_git_repo,
            phases=(
                Phase(
                    name="broken",
                    kind=PhaseKind.PRESERVE,
                    edit=write_edit(CALLER_A_FILE, repos.BROKEN_CALLER_A),
                    checks=(CallersCheck(symbol=TARGET, post=a_listed, pre=a_listed),),
                ),
                Phase(
                    name="fixed",
                    kind=PhaseKind.CHANGE,
                    edit=write_edit(CALLER_A_FILE, repos.fixed_caller_a()),
                    checks=(
                        CallersCheck(
                            symbol=TARGET,
                            post=callers_state(frozenset({CALLER_C}), frozenset()),
                            pre=callers_state(frozenset({CALLER_A}), frozenset({CALLER_C})),
                        ),
                    ),
                ),
            ),
        ),
        one_phase_scenario(
            "S8-delete",
            "generated",
            repos.build_repo_with_caller_b,
            delete_edit(CALLER_B_FILE),
            (
                CallersCheck(
                    symbol=TARGET,
                    post=callers_state(frozenset(), frozenset({CALLER_B})),
                    pre=callers_state(frozenset({CALLER_A, CALLER_B}), frozenset()),
                ),
                ExistsCheck(
                    symbol=CALLER_B,
                    post=ExistsExpect(found=False, files=None),
                    pre=ExistsExpect(found=True, files=None),
                ),
            ),
        ),
        one_phase_scenario(
            "S8-move",
            "generated",
            repos.build_repo_with_caller_b,
            move_edit(CALLER_B_FILE, MOVED_CALLER_B_FILE),
            (
                ExistsCheck(
                    symbol=CALLER_B,
                    post=ExistsExpect(found=True, files=frozenset({MOVED_CALLER_B_FILE})),
                    pre=ExistsExpect(found=True, files=frozenset({CALLER_B_FILE})),
                ),
            ),
        ),
    )


def all_scenarios(corpus_dir: Path) -> tuple[Scenario, ...]:
    """Ön-kayıttaki tüm senaryolar, tablodaki sırayla (S1 her gerçek repo için)."""
    real = tuple(
        scenario for corpus in corpus_repos(corpus_dir) for scenario in s1_scenarios(corpus)
    )
    return (*real, *generated_scenarios())


def select_scenarios(scenarios: tuple[Scenario, ...], ids: frozenset[str]) -> tuple[Scenario, ...]:
    """Kimliği istenen senaryolar; bilinmeyen kimlik açık hatadır."""
    unknown = ids - {scenario.scenario_id for scenario in scenarios}
    if unknown:
        raise UnknownScenarioError(f"unknown scenario ids: {sorted(unknown)}")
    return tuple(scenario for scenario in scenarios if scenario.scenario_id in ids)
