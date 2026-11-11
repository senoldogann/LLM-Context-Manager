"""Kayıtlı cevapları güncel puanlayıcıyla yeniden puanlar ve farkı tablo olarak yazar.

Kullanım (`benchmarks/` içinde):
    PYTHONPATH=. uv run python scripts/rescore_report.py <ajan sonuç dizini>...

Kayıtlara dokunmaz: her koşunun `transcript.result_text`'i `agent.score.score` ile yeniden
puanlanır ve kayıttaki başarıyla karşılaştırılır. Düzenleme görevleri yeniden puanlanmaz (işaret
dosyalarının son hâli kayıtta yoktur) ve sayıları ayrıca yazılır. Çıktı: görev × kol başına
kayıtlı ve yeniden puanlanmış başarı (yalnız değişen satırlar), kol toplamları ve başarısı
değişen koşular.
"""

import json
import sys
from collections import Counter
from dataclasses import dataclass
from pathlib import Path

from agent.score import score
from agent.tasks import KNOWN_TASKS, Task

TASKS: dict[str, Task] = {task.id: task for task in KNOWN_TASKS}


class RescoreReportError(ValueError):
    """Kayıt yeniden puanlanamıyor (bilinmeyen görev ya da eksik alan)."""


@dataclass(frozen=True)
class Rescored:
    """Bir koşunun kayıtlı ve yeniden hesaplanmış başarısı."""

    run_id: str
    task_id: str
    arm: str
    recorded: bool
    rescored: bool


def recorded_success(record: dict[str, object], name: str) -> bool:
    """Kayıttaki başarı; puan yoksa (sonuç olayı olmayan koşu) başarısızdır."""
    result = record.get("score")
    if result is None:
        return False
    if not isinstance(result, dict) or not isinstance(result.get("success"), bool):
        raise RescoreReportError(f"{name}: score has no success field")
    return bool(result["success"])


def rescore_run(record: dict[str, object], name: str) -> Rescored | None:
    """Koşunun yeni başarısı; düzenleme görevinde None."""
    task_id = record.get("task_id")
    arm = record.get("arm")
    run_id = record.get("run_id")
    if not isinstance(task_id, str) or not isinstance(arm, str) or not isinstance(run_id, str):
        raise RescoreReportError(f"{name}: record lacks task_id, arm or run_id")
    task = TASKS.get(task_id)
    if task is None:
        raise RescoreReportError(f"{name}: unknown task {task_id}")
    if task.edit is not None:
        return None
    before = recorded_success(record, name)
    transcript = record.get("transcript")
    if transcript is None:
        return Rescored(run_id, task_id, arm, before, before)
    if not isinstance(transcript, dict) or not isinstance(transcript.get("result_text"), str):
        raise RescoreReportError(f"{name}: transcript has no result_text")
    after = score(task, transcript["result_text"], f"ws-{run_id}", {}).success
    return Rescored(run_id, task_id, arm, before, after)


def report(agent_dir: Path) -> list[str]:
    """Bir ajan dizininin Markdown raporu."""
    runs: list[Rescored] = []
    skipped = 0
    for path in sorted((agent_dir / "runs").glob("*.json")):
        item = rescore_run(json.loads(path.read_text()), str(path))
        if item is None:
            skipped += 1
        else:
            runs.append(item)
    lines = [f"### {agent_dir.parent.name}/{agent_dir.name}", ""]
    before = Counter(run.arm for run in runs if run.recorded)
    after = Counter(run.arm for run in runs if run.rescored)
    total = Counter(run.arm for run in runs)
    lines += ["| Arm | Recorded success | Rescored success |", "|---|---:|---:|"]
    lines += [
        f"| {arm} | {before[arm]}/{total[arm]} | {after[arm]}/{total[arm]} |"
        for arm in sorted(total)
    ]
    changed = [run for run in runs if run.recorded != run.rescored]
    lines.append("")
    lines.append(
        "Runs whose success changes: "
        + (
            ", ".join(
                f"`{run.run_id}` ({'fail→pass' if run.rescored else 'pass→fail'})"
                for run in changed
            )
            or "none"
        )
        + "."
    )
    if skipped:
        lines.append(f"Edit-task runs not rescored: {skipped}.")
    return [*lines, ""]


def main() -> None:
    """Komut satırındaki her ajan dizinini raporlar."""
    for argument in sys.argv[1:]:
        print("\n".join(report(Path(argument))))


if __name__ == "__main__":
    main()
