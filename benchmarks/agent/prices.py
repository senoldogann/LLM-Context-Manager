"""Maliyet bildirmeyen ajanlar için API fiyatıyla eşdeğer maliyet tahmini.

Codex ChatGPT aboneliğinde dolar maliyeti bildirmez; aynı token'ların API'de ne tutacağı,
kişinin CCM'i kullanıp kullanmamaya karar verebilmesi için rapora yazılır. Koşu kayıtları ham
token'ları tutar; tahmin rapor anında buradaki fiyatlarla hesaplanır. Fiyatlar 1M token başına
USD, standart katmandır; kaynak ve alınma tarihi her girdinin yanındadır ve rapora yazılır.
"""

from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class Price:
    """Bir modelin 1M token başına standart API fiyatı ve kaynağı."""

    input: float
    cached_input: float
    output: float
    source: str
    retrieved: str


PRICES: dict[str, Price] = {
    "gpt-6.1-sol": Price(
        input=2.00,
        cached_input=0.10,
        output=10.00,
        source="https://developers.openai.com/api/docs/pricing",
        retrieved="2026-10-02",
    ),
}


def estimated_cost(model: str, uncached_input: int, cached_input: int, output: int) -> float | None:
    """Token'ların API fiyatıyla maliyeti; fiyatı kayıtlı olmayan model için None (bilinmiyor)."""
    price = PRICES.get(model)
    if price is None:
        return None
    return (
        uncached_input * price.input + cached_input * price.cached_input + output * price.output
    ) / 1_000_000
