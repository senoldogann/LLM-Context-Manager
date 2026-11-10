"""Bir görevin kayıtlı koşularını güncel altın cevapla yeniden puanlar.

Kullanım:
    uv run python scripts/rescore.py <sonuç dizini> <görev kimliği> <yedek dizin adı>

Altın cevap koşudan sonra düzeltildiğinde (bkz. PREREGISTRATION.md) kullanılır. Orijinal
kayıtlar `<sonuç dizini>/<yedek dizin adı>/` altına aynen kopyalanır, `runs/` içindeki
kayıtların yalnız `score` alanı yeniden hesaplanır. Ajanın cevabı (`transcript.result_text`)
değişmez. Düzenleme görevleri yeniden puanlanmaz: işaret dosyaları koşudan sonra yoktur.
"""

import json
import shutil
import sys
from dataclasses import asdict
from pathlib import Path

from agent.run import write_atomic
from agent.score import score
from agent.tasks import KNOWN_TASKS, Task


class RescoreError(Exception):
    """Yeniden puanlama yapılamıyor (bilinmeyen görev, düzenleme görevi, eksik alan)."""


def task_by_id(task_id: str) -> Task:
    """Kimliğe göre görev; bulunamazsa açık hata."""
    for task in KNOWN_TASKS:
        if task.id == task_id:
            if task.edit is not None:
                raise RescoreError(f"{task_id} is an edit task; its marker files are gone")
            return task
    raise RescoreError(f"unknown task {task_id}")


def rescore_record(task: Task, record: dict[str, object]) -> dict[str, object]:
    """Kaydın yeni puanlı kopyası; cevap metni kayıttaki dökümden alınır."""
    transcript = record.get("transcript")
    name = record.get("run_id")
    if not isinstance(name, str):
        raise RescoreError("record has no run_id")
    if not isinstance(transcript, dict) or not isinstance(transcript.get("result_text"), str):
        raise RescoreError(f"{name}: record has no transcript result text")
    # Çalışma kopyasının adı koşuda `ws-<run_id>` idi; mutlak yollar buna göre kırpılır.
    result = score(task, transcript["result_text"], f"ws-{name}", {})
    return {**record, "score": asdict(result)}


def asdict_success(record: dict[str, object]) -> object:
    """Yeni puanın `success` alanı (yalnız çıktı için)."""
    result = record["score"]
    return result.get("success") if isinstance(result, dict) else None


def main() -> None:
    """Görevin tüm kayıtlarını yedekler ve yeniden puanlar."""
    out_dir, task_id, backup_name = Path(sys.argv[1]), sys.argv[2], sys.argv[3]
    task = task_by_id(task_id)
    runs = out_dir / "runs"
    records = sorted(runs.glob(f"{task_id}__*.json"))
    if not records:
        raise RescoreError(f"no records for {task_id} under {runs}")
    backup = out_dir / backup_name
    backup.mkdir(exist_ok=False)
    for path in records:
        shutil.copy2(path, backup / path.name)
        record = json.loads(path.read_text())
        updated = rescore_record(task, record)
        write_atomic(path, json.dumps(updated, indent=2, ensure_ascii=False))
        print(f"{path.name}: success {record['score']['success']} -> {asdict_success(updated)}")


if __name__ == "__main__":
    main()
