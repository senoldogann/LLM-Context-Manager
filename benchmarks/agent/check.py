"""Görevlerin korpusa karşı denetimi; API çağrısı yapmaz.

Her görev için beklenen dosyalar ve adlar korpusta var mı, referans düzenleme temiz uygulanıyor
mu, işaretler düzenlemeyi ayırt ediyor mu, beklenen cevap başarılı ve bayat cevap bayat mı
sayılıyor, sınanır. Böylece hem görev verisi hem puanlayıcı gerçek veriyle denetlenmiş olur.
"""

from __future__ import annotations

import json
import re
import tempfile
from collections import Counter
from pathlib import Path

from agent.score import marker_files, score
from agent.tasks import Append, Item, Task, label
from freshness.repos import export_corpus, run_git

WORKSPACE_NAME = "ws-check"


def answer_text(entries: list[str]) -> str:
    """Puanlayıcıya verilecek, ajan cevabı biçimindeki metin."""
    return f"Done.\n\n```json\n{json.dumps({'answer': entries})}\n```\n"


def absolute_entry(item: Item) -> str:
    """Ajanın yazabileceği başka bir biçim: mutlak yol ve sınıf önekli, parantezli ad."""
    path = f"/private/var/folders/x/{WORKSPACE_NAME}/{item.path}"
    return f"{path}:Owner.{item.name}()" if item.name else path


def item_problems(items: tuple[Item, ...], root: Path, context: str) -> list[str]:
    """Dosyası olmayan ya da adı dosyada geçmeyen öğeler."""
    problems: list[str] = []
    for item in items:
        path = root / item.path
        if not path.is_file():
            problems.append(f"{context}: {item.path} does not exist")
            continue
        text = path.read_text()
        for name in (item.name, *item.aliases):
            if name and re.search(rf"\b{re.escape(name)}\b", text) is None:
                problems.append(f"{context}: {name} does not occur in {item.path}")
    return problems


def apply_steps(task: Task, root: Path) -> list[str]:
    """Referans düzenlemeyi kopyaya uygular; bağlamı bulunamayan adımlar sorun olarak döner."""
    if task.edit is None:
        return []
    problems: list[str] = []
    for step in task.edit.steps:
        path = root / step.file
        text = path.read_text()
        if isinstance(step, Append):
            path.write_text(text + step.text)
            continue
        start = text.find(step.after)
        at = text.find(step.old, start) if start >= 0 else -1
        if at < 0:
            problems.append(f"{task.id}: {step.old!r} after {step.after!r} not in {step.file}")
            continue
        path.write_text(text[:at] + step.new + text[at + len(step.old) :])
    return problems


def score_problems(task: Task, contents: dict[str, str], original: dict[str, str]) -> list[str]:
    """Puanlayıcının bu görevde beklendiği gibi davrandığını sınar."""
    problems: list[str] = []
    expected = [label(item) for item in task.expected]
    perfect = score(task, answer_text(expected), WORKSPACE_NAME, contents)
    if not (perfect.success and perfect.precision == 1.0 and not perfect.stale):
        problems.append(f"{task.id}: the expected answer is not a clean success: {perfect}")
    absolute = [absolute_entry(item) for item in task.expected]
    if not score(task, answer_text(absolute), WORKSPACE_NAME, contents).success:
        problems.append(f"{task.id}: absolute paths and qualified names are not recognised")
    if task.edit is None:
        return problems
    before = [label(item) for item in task.expected if item not in task.edit.added]
    stale = score(task, answer_text(before + [label(i) for i in task.edit.removed]), "", contents)
    if stale.success or not stale.stale:
        problems.append(f"{task.id}: the pre-edit answer is not classified as stale: {stale}")
    untouched = score(task, answer_text(expected), WORKSPACE_NAME, original)
    if untouched.edit_applied is not False:
        problems.append(f"{task.id}: the markers do not tell the edited file from the original")
    return problems


def check_task(task: Task, corpus_dir: Path) -> list[str]:
    """Bir görevin sorunları; boş liste görevin kullanılabilir olduğunu gösterir."""
    repo = corpus_dir / task.repo
    if not repo.is_dir():
        return [f"{task.id}: corpus {repo} is missing; run benchmarks/scripts/fetch_corpus.sh"]
    problems: list[str] = []
    if run_git(repo, ("ls-files", "data")).strip():
        problems.append(
            f"{task.id}: {task.repo} tracks files under data/, which the agent cannot see"
        )
    if task.edit is None:
        return problems + item_problems(task.expected, repo, task.id) + score_problems(task, {}, {})
    with tempfile.TemporaryDirectory() as scratch:
        copy = Path(scratch) / WORKSPACE_NAME
        export_corpus(repo, copy)
        problems += apply_steps(task, copy)
        files = marker_files(task.edit)
        contents = {name: (copy / name).read_text() for name in files}
        original = {name: (repo / name).read_text() for name in files}
        problems += item_problems(task.expected, copy, task.id)
        problems += item_problems(task.edit.removed, repo, f"{task.id} (before the change)")
        problems += score_problems(task, contents, original)
    return problems


def check_tasks(tasks: tuple[Task, ...], corpus_dir: Path) -> list[str]:
    """Tüm görevlerin sorunları; kimlikler tekil olmalı."""
    counts = Counter(task.id for task in tasks)
    problems = [f"duplicate task id {task_id}" for task_id, count in counts.items() if count > 1]
    for task in tasks:
        problems += check_task(task, corpus_dir)
    return problems
