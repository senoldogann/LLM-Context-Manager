# Promotion Gate: Holdout Kanıtlı Policy Aktivasyonu

* **Durum:** Kabul Edildi
* **Tarih:** 2026-08-13
* **Bağlam:** Self-improvement'ın "memory notu yazmak" olmaması; yalnızca
  ölçülebilir iyileşme kanıtlanınca policy değişmesi isteniyor.

## Seçenekler
1. Her optimize sonrası otomatik aktivasyon — overfit/gaming riski.
2. Holdout'ta istatistiksel gate + Rejected'ın normal sonuç olması — dürüst.
3. p-value/Bonferroni ile çok katı eşik — deterministik evaluator'da anlamsız.

## Karar
**Seçenek 2** — tek-winner holdout ölçümü + sign test + token guard + overfit
warning (hard-reject değil). Evaluator optimizasyon sırasında immutable.

## Neden?
Bonferroni deterministik ölçümde gereksiz (yalnızca winner holdout'ta ölçülür);
sign test null altında ~%3-5 false-positive verir; `Rejected` bilimsel sonuçtur
ve CI kırmızısı değildir. Bu, Goodhart tuzağından kaçınmanın en sağlam yoludur.
