# LanceDB 0.31 Pin

* **Durum:** Kabul Edildi
* **Tarih:** 2026-08-13
* **Bağlam:** Vektör deposu olarak LanceDB seçildi; API yüzeyi sürümler arasında
  değişiyor (create_table/delete/vector_search).

## Seçenekler
1. `lancedb = "0.31"` aralığında akışkan yükseltme — API kırılmaları riski.
2. `lancedb = "0.31.0"` tam pin + yükseltme testleriyle doğrulama — deterministik.
3. LanceDB yerine başka vektör deposu — migrasyon maliyeti yüksek.

## Karar
**Seçenek 2** — `0.31.0` tam pin.

## Neden?
Yükseltme öncesi API uyumluluğu (create_table/delete/vector_search) testlerle
doğrulanmadan sürüm akışkan bırakılırsa CI/üretim sessizce kırılabilir. Cargo.lock
zaten tam versiyon kilitler; Cargo.toml da pin ile iki kat garanti sağlanır.
