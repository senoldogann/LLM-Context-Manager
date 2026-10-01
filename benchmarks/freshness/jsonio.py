"""JSON değerlerini tipli Python değerlerine daraltan ortak yardımcılar.

Hata türünü çağıran verir: MCP mesajında `McpProtocolError`, sonuç dosyasında
`ResultsFileError`. Böylece aynı daraltma mantığı iki bağlamda tekrar yazılmaz.
"""

from __future__ import annotations

from freshness.model import JsonValue


def as_object(value: JsonValue, context: str, error: type[Exception]) -> dict[str, JsonValue]:
    """Değeri JSON nesnesine daraltır."""
    if isinstance(value, dict):
        return value
    raise error(f"{context}: expected an object, got {type(value).__name__}")


def as_list(value: JsonValue, context: str, error: type[Exception]) -> list[JsonValue]:
    """Değeri JSON dizisine daraltır."""
    if isinstance(value, list):
        return value
    raise error(f"{context}: expected an array, got {type(value).__name__}")


def as_str(value: JsonValue, context: str, error: type[Exception]) -> str:
    """Değeri metne daraltır."""
    if isinstance(value, str):
        return value
    raise error(f"{context}: expected a string, got {type(value).__name__}")


def as_bool(value: JsonValue, context: str, error: type[Exception]) -> bool:
    """Değeri mantıksal değere daraltır."""
    if isinstance(value, bool):
        return value
    raise error(f"{context}: expected a boolean, got {type(value).__name__}")


def as_int(value: JsonValue, context: str, error: type[Exception]) -> int:
    """Değeri tamsayıya daraltır; JSON true/false sayı sayılmaz."""
    if isinstance(value, int) and not isinstance(value, bool):
        return value
    raise error(f"{context}: expected an integer, got {type(value).__name__}")


def as_float(value: JsonValue, context: str, error: type[Exception]) -> float:
    """Değeri ondalık sayıya daraltır; tamsayılar da kabul edilir."""
    if isinstance(value, int | float) and not isinstance(value, bool):
        return float(value)
    raise error(f"{context}: expected a number, got {type(value).__name__}")


def as_optional_int(value: JsonValue, context: str, error: type[Exception]) -> int | None:
    """`null` ya da tamsayı."""
    return None if value is None else as_int(value, context, error)


def as_optional_float(value: JsonValue, context: str, error: type[Exception]) -> float | None:
    """`null` ya da sayı."""
    return None if value is None else as_float(value, context, error)
