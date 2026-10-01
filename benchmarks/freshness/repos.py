"""Çalışma kopyaları, amaca özel küçük repo üreticileri ve betikli düzenlemeler."""

from __future__ import annotations

import shutil
import subprocess
from dataclasses import dataclass
from pathlib import Path

GIT_ENV: dict[str, str] = {
    "PATH": "/usr/bin:/bin",
    "HOME": "/nonexistent",
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_AUTHOR_NAME": "ccm-bench",
    "GIT_AUTHOR_EMAIL": "bench@example.invalid",
    "GIT_COMMITTER_NAME": "ccm-bench",
    "GIT_COMMITTER_EMAIL": "bench@example.invalid",
    "GIT_AUTHOR_DATE": "2026-01-01T00:00:00Z",
    "GIT_COMMITTER_DATE": "2026-01-01T00:00:00Z",
}


class WorkspaceError(RuntimeError):
    """Çalışma kopyası kurulamadı ya da betikli düzenleme beklenen metni bulamadı."""


@dataclass(frozen=True)
class CorpusRepo:
    """S1 için gerçek repo ve enjeksiyon noktaları (repo köküne göreli yollar)."""

    name: str
    source: Path
    target_file: str
    target_module: str
    caller_a_file: str
    caller_b_file: str


def run_git(repo: Path, args: tuple[str, ...]) -> str:
    """Git komutunu sabit kimlik ve tarihle çalıştırır; hata açıkça bildirilir."""
    completed = subprocess.run(
        ["git", *args], cwd=repo, env=GIT_ENV, capture_output=True, text=True, check=False
    )
    if completed.returncode != 0:
        raise WorkspaceError(f"git {' '.join(args)} failed in {repo}: {completed.stderr.strip()}")
    return completed.stdout


def init_git(repo: Path, branch: str) -> None:
    """Dizini git deposu yapar ve tüm dosyaları tek commit'e koyar."""
    run_git(repo, ("init", "-q", "-b", branch))
    run_git(repo, ("add", "-A"))
    run_git(repo, ("commit", "-q", "-m", "baseline"))


def export_corpus(source: Path, destination: Path) -> None:
    """Sabitlenmiş korpus klonunun HEAD ağacını temiz bir dizine çıkarır."""
    destination.mkdir(parents=True)
    archive = subprocess.run(
        ["git", "-C", str(source), "archive", "--format=tar", "HEAD"],
        env=GIT_ENV,
        capture_output=True,
        check=False,
    )
    if archive.returncode != 0:
        raise WorkspaceError(f"git archive failed for {source}: {archive.stderr.decode()[:500]}")
    extract = subprocess.run(
        ["tar", "-x", "-C", str(destination)],
        input=archive.stdout,
        capture_output=True,
        check=False,
    )
    if extract.returncode != 0:
        raise WorkspaceError(f"tar -x failed into {destination}: {extract.stderr.decode()[:500]}")


def write_file(path: Path, text: str) -> None:
    """Dosyayı yerinde yazar (kes, yaz, kapat); üst dizinleri oluşturur."""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def append_text(path: Path, text: str) -> None:
    """Dosyanın sonuna metin ekler."""
    if not path.is_file():
        raise WorkspaceError(f"cannot append to missing file {path}")
    with path.open("a", encoding="utf-8") as handle:
        handle.write(text)


def replace_once(path: Path, old: str, new: str) -> None:
    """`old` tam bir kez geçiyorsa değiştirir; aksi halde düzenleme geçersizdir."""
    content = path.read_text(encoding="utf-8")
    count = content.count(old)
    if count != 1:
        raise WorkspaceError(f"expected exactly one {old!r} in {path}, found {count}")
    path.write_text(content.replace(old, new), encoding="utf-8")


def insert_top(path: Path, text: str) -> None:
    """Dosyanın başına metin ekler."""
    content = path.read_text(encoding="utf-8")
    path.write_text(text + content, encoding="utf-8")


def remove_path(path: Path) -> None:
    """Çalışma kopyasını ya da üretilmiş bir dosyayı siler."""
    if path.is_dir():
        shutil.rmtree(path)
    elif path.exists():
        path.unlink()


def move_path(source: Path, destination: Path) -> None:
    """Dosyayı tek bir `rename` ile yeni yola taşır; hedef dizini önce oluşturur."""
    if not source.is_file():
        raise WorkspaceError(f"cannot move missing file {source}")
    destination.parent.mkdir(parents=True, exist_ok=True)
    source.rename(destination)


def target_source(name: str) -> str:
    """Enjekte edilen hedef fonksiyon."""
    return f"\n\ndef {name}():\n    return 1\n"


def caller_source(caller: str, module: str, target: str) -> str:
    """Hedefi fonksiyon içinde import edip çağıran fonksiyon."""
    return f"\n\ndef {caller}():\n    from {module} import {target}\n\n    return {target}()\n"


def idle_source(caller: str) -> str:
    """Hedefi ne import eden ne çağıran gövde: çağrı kaldırıldıktan sonraki hâl."""
    return f"\n\ndef {caller}():\n    return 0\n"


def module_with_caller(caller: str, module: str, target: str) -> str:
    """Yalnız bir çağıran içeren modül dosyası."""
    return f'"""{caller} modülü."""\n' + caller_source(caller, module, target)


def module_without_call(caller: str) -> str:
    """Fonksiyonu hedefle ilişkisi olmayan modül dosyası."""
    return f'"""{caller} modülü."""\n' + idle_source(caller)


def build_corpus_copy(corpus: CorpusRepo, destination: Path) -> None:
    """S1: gerçek repoyu çıkarır, hedefi ve ilk çağıranı enjekte edip commit'ler."""
    export_corpus(corpus.source, destination)
    append_text(destination / corpus.target_file, target_source("ccmb_target"))
    append_text(
        destination / corpus.caller_a_file,
        caller_source("ccmb_caller_a", corpus.target_module, "ccmb_target"),
    )
    init_git(destination, "main")


def build_small_repo(destination: Path, filler_modules: int) -> None:
    """Hedef ve ilk çağıranı olan küçük Python paketi (git'siz)."""
    write_file(destination / "pkg" / "__init__.py", '"""Üretilmiş paket."""\n')
    write_file(
        destination / "pkg" / "target.py", '"""Hedef modül."""' + target_source("ccmb_target")
    )
    write_file(
        destination / "pkg" / "caller_a.py",
        module_with_caller("ccmb_caller_a", "pkg.target", "ccmb_target"),
    )
    for index in range(filler_modules):
        write_file(
            destination / "pkg" / f"filler_{index:02d}.py",
            f'"""Dolgu modülü {index}."""\n\n\ndef filler_{index:02d}():\n    return {index}\n',
        )


def build_plain_repo(destination: Path) -> None:
    """S4-nogit: `.git` olmayan küçük repo."""
    build_small_repo(destination, 10)


def build_git_repo(destination: Path) -> None:
    """S2 ve S7: tek commit'lik küçük git deposu."""
    build_small_repo(destination, 10)
    init_git(destination, "main")


def build_repo_with_caller_b(destination: Path) -> None:
    """S8: başlangıçta `ccmb_caller_b` de hedefi çağırır."""
    build_small_repo(destination, 10)
    write_file(
        destination / "pkg" / "caller_b.py",
        module_with_caller("ccmb_caller_b", "pkg.target", "ccmb_target"),
    )
    init_git(destination, "main")


def build_branch_repo(destination: Path) -> None:
    """S3-branch: `feature` dalında caller_a hedefi bırakır (import dahil), caller_b eklenir."""
    build_small_repo(destination, 10)
    init_git(destination, "main")
    run_git(destination, ("checkout", "-q", "-b", "feature"))
    write_file(destination / "pkg" / "caller_a.py", module_without_call("ccmb_caller_a"))
    write_file(
        destination / "pkg" / "caller_b.py",
        module_with_caller("ccmb_caller_b", "pkg.target", "ccmb_target"),
    )
    run_git(destination, ("add", "-A"))
    run_git(destination, ("commit", "-q", "-m", "feature"))
    run_git(destination, ("checkout", "-q", "main"))


def build_bulk_repo(destination: Path, count: int, prefix: str) -> None:
    """S3-bulk ve S6: `count` modülün her biri henüz çağırmayan bir fonksiyon taşır."""
    build_small_repo(destination, 10)
    for index in range(count):
        write_file(
            destination / prefix / f"m_{index:02d}.py",
            f'"""Toplu modül {index}."""\n\n\ndef ccmb_{prefix}_{index:02d}():\n    return 0\n',
        )
    init_git(destination, "main")


def bulk_callers(count: int, prefix: str) -> frozenset[str]:
    """Toplu düzenlemeden sonra hedefi çağırması beklenen fonksiyonlar."""
    return frozenset(f"ccmb_{prefix}_{index:02d}" for index in range(count))


def rewrite_bulk(destination: Path, count: int, prefix: str) -> None:
    """Toplu modüllerin hepsini hedefi çağıracak biçimde tek geçişte yeniden yazar."""
    for index in range(count):
        name = f"ccmb_{prefix}_{index:02d}"
        write_file(
            destination / prefix / f"m_{index:02d}.py",
            f'"""Toplu modül {index}."""' + caller_source(name, "pkg.target", "ccmb_target"),
        )


# `vendor/` CCM'nin dışlama listesinde; iç içe repo davranışını ölçmek için nötr bir ad.
NESTED_ROOT = "libs/inner"
NESTED_MODULE = "inner_pkg.target"


def build_nested_repo(destination: Path) -> None:
    """S4-nested: dış repo + kendi .git'i olan `libs/inner` iç reposu."""
    write_file(destination / "outer.py", '"""Dış repo."""\n\n\ndef outer_fn():\n    return 0\n')
    init_git(destination, "main")
    inner = destination / NESTED_ROOT
    write_file(inner / "inner_pkg" / "__init__.py", '"""İç paket."""\n')
    write_file(inner / "inner_pkg" / "target.py", '"""İç hedef."""' + target_source("ccmb_target"))
    write_file(
        inner / "inner_pkg" / "caller_a.py",
        module_with_caller("ccmb_caller_a", NESTED_MODULE, "ccmb_target"),
    )
    init_git(inner, "main")


SHIFT_FILE = "pkg/shift.py"
SHIFT_INSERT = "# 1\n# 2\n# 3\n# 4\n# 5\n"
SHIFT_SOURCE = "\n".join(
    [
        '"""Kaydırma fikstürü."""',
        "",
        "",
        "CONSTANT = 1",
        "",
        "",
        "def first_fn():",
        "    return CONSTANT",
        "",
        "def shifted_fn():",
        "    value = CONSTANT",
        "    return value",
        "",
        "",
        "def occupant_fn():",
        "    value = CONSTANT + 1",
        "    return value",
        "",
    ]
)


def build_shift_repo(destination: Path) -> None:
    """S5: `shifted_fn` 10-12. satırda, `occupant_fn` 15-17. satırda."""
    build_small_repo(destination, 10)
    write_file(destination / SHIFT_FILE, SHIFT_SOURCE)
    init_git(destination, "main")


BROKEN_CALLER_A = (
    '"""ccmb_caller_a modülü (yarım kayıt)."""\n\n\ndef ccmb_caller_a(:\n'
    "    from pkg.target import ccmb_target\n\n    return ccmb_target(\n"
)


def fixed_caller_a() -> str:
    """S7 ikinci aşama: geçerli caller_a ve yeni ccmb_caller_c."""
    return (
        '"""ccmb_caller_a modülü."""'
        + caller_source("ccmb_caller_a", "pkg.target", "ccmb_target")
        + caller_source("ccmb_caller_c", "pkg.target", "ccmb_target")
    )
