"""Yapılandırılmış (JSON satırı) log yardımcıları."""

from __future__ import annotations

import json
import logging

from freshness.model import JsonValue


def log_event(logger: logging.Logger, level: int, event: str, fields: dict[str, JsonValue]) -> None:
    """Olayı sabit bir adla loglar; dinamik değerler ayrı alanlarda taşınır."""
    logger.log(level, event, extra={"fields": fields})


class JsonLineFormatter(logging.Formatter):
    """Her kaydı tek satırlık JSON olarak biçimlendirir."""

    def format(self, record: logging.LogRecord) -> str:
        fields: JsonValue = getattr(record, "fields", {})
        payload: dict[str, JsonValue] = {
            "level": record.levelname,
            "logger": record.name,
            "event": record.getMessage(),
            "fields": fields,
        }
        return json.dumps(payload, ensure_ascii=False, default=str)
