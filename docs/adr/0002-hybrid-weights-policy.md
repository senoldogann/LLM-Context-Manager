# Hybrid Ağırlıkların Policy Olarak Modellenmesi

* **Durum:** Kabul Edildi
* **Tarih:** 2026-08-13
* **Bağlam:** Hybrid skorlama ağırlıkları (graph/semantic/spatial/recent) sabit
  ve env override'lıydı; self-improvement için versioned olmaları gerekiyor.

## Seçenekler
1. Env değişkenleriyle ağırlıkları canlı tutmak — ölçülebilir öğrenme yok.
2. `RetrievalPolicy` yapısı + `PolicyStore` + history — versioned, holdout
   kanıtlı, rollback edilebilir.
3. YAML tabanlı policy dosyası — serde_yaml dependency + schema drift riski.

## Karar
**Seçenek 2** — JSON/JSONL `RetrievalPolicy` + `PolicyStore` + `history.jsonl`.

## Neden?
Yeni dependency yok (JSON mevcut serde ile); baseline üretim varsayılanlarının
birebir karşılığı; promotion gate yalnızca holdout'ta kanıtlanan iyileşmeyi
aktive eder; env override yalnızca debug amaçlı (uyarı loglu) kalır.
