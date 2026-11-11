"""`agent run`'ı kota duruşlarında sınırın sıfırlanmasını bekleyerek sonuna kadar sürdürür.

Kullanım (`benchmarks/` içinde; kimlik bilgileri ortamdan aynen geçer):
    uv run python scripts/run_until_done.py <en çok deneme> -- <agent run argümanları>...

Her koşu kaydı atomik yazılır ve aynı komut kayıtlı koşuları atlar; bu yüzden durup sürmek veri
kaybettirmez. `agent run` kota yüzünden çıkarsa (kod 4) `<out-dir>/quota-stop.json`'daki
sıfırlanma anına kadar beklenir, sonra aynı komut yeniden çalıştırılır. Ajan sıfırlanma anını
bildirmediyse ya da o an geçtiği hâlde sınır sürüyorsa bekleme 15 dakikadan başlayıp her
denemede ikiye katlanır (en çok 2 saat). Diğer çıkış kodları aynen döner; deneme hakkı biterse
`RetryLimitError` yükselir.
"""

import json
import logging
import subprocess
import sys
import time
from pathlib import Path

from agent.__main__ import EXIT_QUOTA, QUOTA_STOP_FILE

# Sıfırlanma anından sonra sınırın gerçekten kalkması için bırakılan pay.
RESET_MARGIN_S = 120
BACKOFF_START_S = 15 * 60
BACKOFF_MAX_S = 2 * 60 * 60

log = logging.getLogger("run_until_done")


class RetryLimitError(RuntimeError):
    """Deneme hakkı bitti, koşular hâlâ kota yüzünden duruyor."""


def out_dir_of(arguments: list[str]) -> Path:
    """`agent run` argümanlarındaki `--out-dir` değeri; yoksa açık hata."""
    for index, argument in enumerate(arguments):
        if argument == "--out-dir" and index + 1 < len(arguments):
            return Path(arguments[index + 1])
        if argument.startswith("--out-dir="):
            return Path(argument.split("=", 1)[1])
    raise ValueError(f"agent run arguments have no --out-dir: {arguments}")


def reported_reset(out_dir: Path) -> float | None:
    """`quota-stop.json`'daki sıfırlanma anı (Unix saniyesi); bildirilmediyse None."""
    stop = json.loads((out_dir / QUOTA_STOP_FILE).read_text())
    value = stop.get("resets_at")
    if value is None:
        return None
    if not isinstance(value, int | float):
        raise ValueError(f"{out_dir / QUOTA_STOP_FILE}: resets_at is {value!r}")
    return float(value)


def wake_time(reset: float | None, now: float, attempt: int) -> float:
    """Bir sonraki denemenin anı: bildirilen sıfırlanma + pay ya da üstel bekleme."""
    if reset is not None and reset + RESET_MARGIN_S > now:
        return reset + RESET_MARGIN_S
    return now + min(BACKOFF_START_S * (1 << (attempt - 1)), BACKOFF_MAX_S)


def main() -> int:
    """Komutu sonuna kadar ya da kota dışı bir çıkışa kadar yürütür."""
    logging.basicConfig(level=logging.INFO, format="%(asctime)s %(levelname)s %(message)s")
    separator = sys.argv.index("--")
    max_attempts = int(sys.argv[1])
    arguments = sys.argv[separator + 1 :]
    out_dir = out_dir_of(arguments)
    for attempt in range(1, max_attempts + 1):
        log.info("starting agent run", extra={"attempt": attempt, "out_dir": str(out_dir)})
        code = subprocess.run(
            [sys.executable, "-u", "-m", "agent", "run", *arguments], check=False
        ).returncode
        if code != EXIT_QUOTA:
            return code
        reset = reported_reset(out_dir)
        wake = wake_time(reset, time.time(), attempt)
        log.warning(
            "usage limit; waiting until %s",
            time.strftime("%Y-%m-%d %H:%M:%S %Z", time.localtime(wake)),
            extra={"attempt": attempt, "reported_reset": reset},
        )
        time.sleep(max(0.0, wake - time.time()))
    raise RetryLimitError(f"{out_dir}: still stopped on the usage limit after {max_attempts} runs")


if __name__ == "__main__":
    sys.exit(main())
