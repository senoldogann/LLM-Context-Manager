"""Ham sonuç dosyası: şema, atomik yazma ve doğrulayarak geri okuma."""

from __future__ import annotations

import json
from dataclasses import dataclass
from pathlib import Path

from freshness.jsonio import (
    as_bool,
    as_float,
    as_int,
    as_list,
    as_object,
    as_optional_float,
    as_optional_int,
    as_str,
)
from freshness.model import (
    ArtifactInfo,
    JsonValue,
    LabelChannel,
    ProbeRecord,
    RunRecord,
    SystemInfo,
    Verdict,
)

SCHEMA_VERSION = 1


class ResultsFileError(ValueError):
    """Sonuç dosyası beklenen şemaya uymuyor."""


@dataclass(frozen=True)
class EnvironmentInfo:
    """Ölçüm makinesi; kullanıcıya özgü yol ya da ad içermez."""

    platform: str
    machine: str
    cpu: str
    logical_cpus: int
    memory_bytes: int
    python: str


@dataclass(frozen=True)
class Settings:
    """Koşunun ön-kayıtlı sabitleri ve komut satırından gelen ayarları."""

    t_grid_s: tuple[float, ...]
    settle_s: float
    ready_poll_s: float
    ready_timeout_s: float
    request_timeout_s: float
    repetitions: int
    scenarios: tuple[str, ...]


@dataclass(frozen=True)
class ResultsFile:
    """Bir sistemin tüm koşuları ve ölçüm bağlamı."""

    schema_version: int
    harness_revision: str
    preregistration: str
    system: SystemInfo
    environment: EnvironmentInfo
    settings: Settings
    started_at: str
    finished_at: str
    complete: bool
    runs: tuple[RunRecord, ...]


def artifact_to_json(artifact: ArtifactInfo) -> dict[str, JsonValue]:
    """İkili kimliğini JSON'a çevirir."""
    return {"name": artifact.name, "sha256": artifact.sha256}


def probe_to_json(probe: ProbeRecord) -> dict[str, JsonValue]:
    """Tek probu JSON'a çevirir."""
    return {
        "phase": probe.phase,
        "t_target_s": probe.t_target_s,
        "t_actual_s": probe.t_actual_s,
        "duration_s": probe.duration_s,
        "verdict": probe.verdict.value,
        "label_channel": probe.label_channel.value,
        "label_text": probe.label_text,
        "partial": probe.partial,
        "reflected_count": probe.reflected_count,
        "detail": probe.detail,
    }


def run_to_json(run: RunRecord) -> dict[str, JsonValue]:
    """Bir koşuyu ve problarını JSON'a çevirir."""
    probes: list[JsonValue] = [probe_to_json(probe) for probe in run.probes]
    return {
        "scenario": run.scenario,
        "repo": run.repo,
        "repetition": run.repetition,
        "index_seconds": run.index_seconds,
        "ready_seconds": run.ready_seconds,
        "baseline_ok": run.baseline_ok,
        "baseline_detail": run.baseline_detail,
        "crashed": run.crashed,
        "error": run.error,
        "probes": probes,
    }


def results_to_json(results: ResultsFile) -> dict[str, JsonValue]:
    """Sonuç dosyasının tamamını JSON'a çevirir."""
    artifacts: list[JsonValue] = [artifact_to_json(item) for item in results.system.artifacts]
    t_grid: list[JsonValue] = [value for value in results.settings.t_grid_s]
    scenarios: list[JsonValue] = [value for value in results.settings.scenarios]
    runs: list[JsonValue] = [run_to_json(run) for run in results.runs]
    environment = results.environment
    settings = results.settings
    return {
        "schema_version": results.schema_version,
        "harness_revision": results.harness_revision,
        "preregistration": results.preregistration,
        "system": {
            "name": results.system.name,
            "version": results.system.version,
            "artifacts": artifacts,
        },
        "environment": {
            "platform": environment.platform,
            "machine": environment.machine,
            "cpu": environment.cpu,
            "logical_cpus": environment.logical_cpus,
            "memory_bytes": environment.memory_bytes,
            "python": environment.python,
        },
        "settings": {
            "t_grid_s": t_grid,
            "settle_s": settings.settle_s,
            "ready_poll_s": settings.ready_poll_s,
            "ready_timeout_s": settings.ready_timeout_s,
            "request_timeout_s": settings.request_timeout_s,
            "repetitions": settings.repetitions,
            "scenarios": scenarios,
        },
        "started_at": results.started_at,
        "finished_at": results.finished_at,
        "complete": results.complete,
        "runs": runs,
    }


def write_results(path: Path, results: ResultsFile) -> None:
    """Sonuç dosyasını atomik yazar: geçici dosya, sonra tek `rename`."""
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(path.name + ".tmp")
    text = json.dumps(results_to_json(results), ensure_ascii=False, indent=2)
    temporary.write_text(text + "\n", encoding="utf-8")
    temporary.replace(path)


def field(obj: dict[str, JsonValue], key: str, context: str) -> JsonValue:
    """Zorunlu alan; yoksa açık hata."""
    if key not in obj:
        raise ResultsFileError(f"{context}: missing field {key!r}")
    return obj[key]


def read_str(obj: dict[str, JsonValue], key: str, context: str) -> str:
    """Zorunlu metin alanı."""
    return as_str(field(obj, key, context), f"{context}.{key}", ResultsFileError)


def read_int(obj: dict[str, JsonValue], key: str, context: str) -> int:
    """Zorunlu tamsayı alanı."""
    return as_int(field(obj, key, context), f"{context}.{key}", ResultsFileError)


def read_float(obj: dict[str, JsonValue], key: str, context: str) -> float:
    """Zorunlu sayı alanı."""
    return as_float(field(obj, key, context), f"{context}.{key}", ResultsFileError)


def read_bool(obj: dict[str, JsonValue], key: str, context: str) -> bool:
    """Zorunlu mantıksal alan."""
    return as_bool(field(obj, key, context), f"{context}.{key}", ResultsFileError)


def read_object(obj: dict[str, JsonValue], key: str, context: str) -> dict[str, JsonValue]:
    """Zorunlu nesne alanı."""
    return as_object(field(obj, key, context), f"{context}.{key}", ResultsFileError)


def read_list(obj: dict[str, JsonValue], key: str, context: str) -> list[JsonValue]:
    """Zorunlu dizi alanı."""
    return as_list(field(obj, key, context), f"{context}.{key}", ResultsFileError)


def probe_from_json(value: JsonValue, context: str) -> ProbeRecord:
    """JSON'dan tek prob."""
    obj = as_object(value, context, ResultsFileError)
    return ProbeRecord(
        phase=read_str(obj, "phase", context),
        t_target_s=read_float(obj, "t_target_s", context),
        t_actual_s=read_float(obj, "t_actual_s", context),
        duration_s=read_float(obj, "duration_s", context),
        verdict=Verdict(read_str(obj, "verdict", context)),
        label_channel=LabelChannel(read_str(obj, "label_channel", context)),
        label_text=read_str(obj, "label_text", context),
        partial=read_bool(obj, "partial", context),
        reflected_count=as_optional_int(
            field(obj, "reflected_count", context), f"{context}.reflected_count", ResultsFileError
        ),
        detail=read_str(obj, "detail", context),
    )


def run_from_json(value: JsonValue, context: str) -> RunRecord:
    """JSON'dan tek koşu."""
    obj = as_object(value, context, ResultsFileError)
    return RunRecord(
        scenario=read_str(obj, "scenario", context),
        repo=read_str(obj, "repo", context),
        repetition=read_int(obj, "repetition", context),
        index_seconds=as_optional_float(
            field(obj, "index_seconds", context), f"{context}.index_seconds", ResultsFileError
        ),
        ready_seconds=as_optional_float(
            field(obj, "ready_seconds", context), f"{context}.ready_seconds", ResultsFileError
        ),
        baseline_ok=read_bool(obj, "baseline_ok", context),
        baseline_detail=read_str(obj, "baseline_detail", context),
        probes=tuple(
            probe_from_json(item, f"{context}.probes[{index}]")
            for index, item in enumerate(read_list(obj, "probes", context))
        ),
        crashed=read_bool(obj, "crashed", context),
        error=read_str(obj, "error", context),
    )


def results_from_json(value: JsonValue, context: str) -> ResultsFile:
    """JSON'dan sonuç dosyası; şema sürümü farklıysa açık hata."""
    obj = as_object(value, context, ResultsFileError)
    version = read_int(obj, "schema_version", context)
    if version != SCHEMA_VERSION:
        raise ResultsFileError(f"{context}: schema_version {version}, expected {SCHEMA_VERSION}")
    system = read_object(obj, "system", context)
    environment = read_object(obj, "environment", context)
    settings = read_object(obj, "settings", context)
    return ResultsFile(
        schema_version=version,
        harness_revision=read_str(obj, "harness_revision", context),
        preregistration=read_str(obj, "preregistration", context),
        system=SystemInfo(
            name=read_str(system, "name", f"{context}.system"),
            version=read_str(system, "version", f"{context}.system"),
            artifacts=tuple(
                ArtifactInfo(
                    name=read_str(item_obj, "name", f"{context}.system.artifacts"),
                    sha256=read_str(item_obj, "sha256", f"{context}.system.artifacts"),
                )
                for item_obj in (
                    as_object(item, f"{context}.system.artifacts", ResultsFileError)
                    for item in read_list(system, "artifacts", f"{context}.system")
                )
            ),
        ),
        environment=EnvironmentInfo(
            platform=read_str(environment, "platform", f"{context}.environment"),
            machine=read_str(environment, "machine", f"{context}.environment"),
            cpu=read_str(environment, "cpu", f"{context}.environment"),
            logical_cpus=read_int(environment, "logical_cpus", f"{context}.environment"),
            memory_bytes=read_int(environment, "memory_bytes", f"{context}.environment"),
            python=read_str(environment, "python", f"{context}.environment"),
        ),
        settings=Settings(
            t_grid_s=tuple(
                as_float(item, f"{context}.settings.t_grid_s", ResultsFileError)
                for item in read_list(settings, "t_grid_s", f"{context}.settings")
            ),
            settle_s=read_float(settings, "settle_s", f"{context}.settings"),
            ready_poll_s=read_float(settings, "ready_poll_s", f"{context}.settings"),
            ready_timeout_s=read_float(settings, "ready_timeout_s", f"{context}.settings"),
            request_timeout_s=read_float(settings, "request_timeout_s", f"{context}.settings"),
            repetitions=read_int(settings, "repetitions", f"{context}.settings"),
            scenarios=tuple(
                as_str(item, f"{context}.settings.scenarios", ResultsFileError)
                for item in read_list(settings, "scenarios", f"{context}.settings")
            ),
        ),
        started_at=read_str(obj, "started_at", context),
        finished_at=read_str(obj, "finished_at", context),
        complete=read_bool(obj, "complete", context),
        runs=tuple(
            run_from_json(item, f"{context}.runs[{index}]")
            for index, item in enumerate(read_list(obj, "runs", context))
        ),
    )


def read_results(path: Path) -> ResultsFile:
    """Sonuç dosyasını okur ve şemaya göre doğrular."""
    try:
        value: JsonValue = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as error:
        raise ResultsFileError(f"{path}: invalid JSON: {error}") from error
    return results_from_json(value, str(path))
