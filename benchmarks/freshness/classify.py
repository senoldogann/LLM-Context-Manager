"""Ön-kayıtlı kurallarla prob sınıflandırması (bkz. PREREGISTRATION.md); saf fonksiyonlar."""

from __future__ import annotations

from dataclasses import dataclass

from freshness.model import (
    CallersAnswer,
    ExistsAnswer,
    LabelChannel,
    LabelObservation,
    NodeAnswer,
    ProbeOutcome,
    Verdict,
)


@dataclass(frozen=True)
class CallersExpect:
    """`callers` için beklenen durum: hedef bulunur mu, kimler listede olmalı, kimler olmamalı."""

    target_found: bool
    present: frozenset[str]
    absent: frozenset[str]


@dataclass(frozen=True)
class ExistsExpect:
    """`exists` için beklenen durum; `files` None ise dosyalar karşılaştırılmaz."""

    found: bool
    files: frozenset[str] | None


@dataclass(frozen=True)
class NodeExpect:
    """`node_at` için beklenen en içteki düğüm ve satır aralığı."""

    name: str
    start_line: int
    end_line: int


@dataclass(frozen=True)
class CheckResult:
    """Bir sorunun cevabının düzenleme sonrası (post) ve öncesi (pre) durumla karşılaştırması."""

    post_holds: bool
    pre_holds: bool
    error: str
    label: LabelObservation
    applied_changes: int
    expected_changes: int
    observed: str


# Bayat cevabın sınıfı, işaretin ulaştığı kanala göre belirlenir.
STALE_VERDICTS: dict[LabelChannel, Verdict] = {
    LabelChannel.CONTENT_TEXT: Verdict.STALE_LABELED,
    LabelChannel.STRUCTURED_ONLY: Verdict.STALE_STRUCTURED_ONLY,
    LabelChannel.NONE: Verdict.STALE_SILENT,
}


def callers_hold(expect: CallersExpect, answer: CallersAnswer) -> bool:
    """Hatasız cevap beklenen `callers` durumunu sağlıyor mu?"""
    if answer.error or answer.target_found != expect.target_found:
        return False
    return expect.present <= answer.callers and expect.absent.isdisjoint(answer.callers)


def judge_callers(post: CallersExpect, pre: CallersExpect, answer: CallersAnswer) -> CheckResult:
    """`callers` cevabını değerlendirir; beklenen değişikliklerin kaçının yansıdığını sayar."""
    additions = post.present - pre.present
    removals = post.absent & pre.present
    applied = (
        len(additions & answer.callers) + len(removals - answer.callers)
        if answer.target_found
        else 0
    )
    return CheckResult(
        post_holds=callers_hold(post, answer),
        pre_holds=callers_hold(pre, answer),
        error=answer.error,
        label=answer.label,
        applied_changes=applied,
        expected_changes=len(additions) + len(removals),
        observed=f"found={answer.target_found} callers={sorted(answer.callers)}",
    )


def exists_hold(expect: ExistsExpect, answer: ExistsAnswer) -> bool:
    """Hatasız cevap beklenen `exists` durumunu sağlıyor mu?"""
    if answer.error or answer.found != expect.found:
        return False
    return expect.files is None or answer.files == expect.files


def judge_exists(post: ExistsExpect, pre: ExistsExpect, answer: ExistsAnswer) -> CheckResult:
    """`exists` cevabını değerlendirir."""
    return CheckResult(
        post_holds=exists_hold(post, answer),
        pre_holds=exists_hold(pre, answer),
        error=answer.error,
        label=answer.label,
        applied_changes=0,
        expected_changes=0,
        observed=f"found={answer.found} files={sorted(answer.files)}",
    )


def node_holds(expect: NodeExpect, answer: NodeAnswer) -> bool:
    """Hatasız cevap beklenen düğümü ve aralığı veriyor mu?"""
    return (
        not answer.error
        and answer.name == expect.name
        and answer.start_line == expect.start_line
        and answer.end_line == expect.end_line
    )


def judge_node(post: NodeExpect, pre: NodeExpect, answer: NodeAnswer) -> CheckResult:
    """`node_at` cevabını değerlendirir."""
    return CheckResult(
        post_holds=node_holds(post, answer),
        pre_holds=node_holds(pre, answer),
        error=answer.error,
        label=answer.label,
        applied_changes=0,
        expected_changes=0,
        observed=f"node={answer.name or '-'} lines={answer.start_line}-{answer.end_line}",
    )


def merge_labels(results: tuple[CheckResult, ...]) -> LabelObservation:
    """Probun tüm cevaplarındaki işaretleri birleştirir; model-görünür metin en güçlü kanaldır."""
    channels = {result.label.channel for result in results}
    text = " | ".join(dict.fromkeys(result.label.text for result in results if result.label.text))
    if LabelChannel.CONTENT_TEXT in channels:
        return LabelObservation(channel=LabelChannel.CONTENT_TEXT, text=text)
    if LabelChannel.STRUCTURED_ONLY in channels:
        return LabelObservation(channel=LabelChannel.STRUCTURED_ONLY, text=text)
    return LabelObservation(channel=LabelChannel.NONE, text=text)


def observed_of(results: tuple[CheckResult, ...]) -> str:
    """Probun ham cevaplarının kısa özeti."""
    return "; ".join(result.observed for result in results)


def errors_of(results: tuple[CheckResult, ...]) -> str:
    """Probun hata metinleri; hata yoksa boş."""
    return "; ".join(result.error for result in results if result.error)


def classify_change(results: tuple[CheckResult, ...]) -> ProbeOutcome:
    """Düzenleme aşamasındaki probu tek bir sınıfa atar.

    Sıra: hata → ERROR_EMPTY; son-durum → CORRECT; tam olarak ön-durum → STALE_*
    (kanala göre); ikisi de değilse ERROR_EMPTY, kısmi güncelleme ise `partial`.
    """
    label = merge_labels(results)
    applied = sum(result.applied_changes for result in results)
    expected = sum(result.expected_changes for result in results)
    reflected = applied if expected > 0 else None
    errors = errors_of(results)
    if errors:
        return ProbeOutcome(
            verdict=Verdict.ERROR_EMPTY,
            label=label,
            partial=False,
            reflected_count=reflected,
            detail=errors,
        )
    if all(result.post_holds for result in results):
        verdict = Verdict.CORRECT
    elif all(result.pre_holds for result in results):
        verdict = STALE_VERDICTS[label.channel]
    else:
        partial = any(result.post_holds for result in results) or 0 < applied < expected
        return ProbeOutcome(
            verdict=Verdict.ERROR_EMPTY,
            label=label,
            partial=partial,
            reflected_count=reflected,
            detail=observed_of(results),
        )
    return ProbeOutcome(
        verdict=verdict,
        label=label,
        partial=False,
        reflected_count=reflected,
        detail=observed_of(results),
    )


def classify_preserve(results: tuple[CheckResult, ...]) -> ProbeOutcome:
    """S7 1. aşama: bozuk kayıttan sonra son geçerli durum korunuyor mu, söyleniyor mu?

    Yalnız `structuredContent`'teki işaret, H1'deki gibi sessiz sayılır.
    """
    label = merge_labels(results)
    errors = errors_of(results)
    if errors:
        verdict = Verdict.ERROR_EMPTY
    elif not all(result.pre_holds for result in results):
        verdict = Verdict.LOST
    elif label.channel is LabelChannel.CONTENT_TEXT:
        verdict = Verdict.PRESERVED_LABELED
    else:
        verdict = Verdict.PRESERVED_SILENT
    return ProbeOutcome(
        verdict=verdict,
        label=label,
        partial=False,
        reflected_count=None,
        detail=errors or observed_of(results),
    )
