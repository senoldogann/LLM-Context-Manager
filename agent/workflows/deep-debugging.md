---
description: Scientific debugging protocol based on Hypothesis-Driven Debugging." trigger: "Called automatically by pre-production or manually via 'debug this
---

# DEEP DEBUGGING PROTOCOL

**RULE:** Asla sorunu yeniden üretmeden (reproduce) kodu değiştirme.

## PHASE 1: REPRODUCTION (Kanıt)
1.  **Action:** Hatayı tetikleyen en küçük test senaryosunu (Minimal Reproduction Script) yaz.
2.  **Verify:** Bu testin başarısız olduğunu (FAIL) gör. Eğer test geçiyorsa, yanlış yere bakıyorsun.

## PHASE 2: ISOLATION (Teşhis)
1.  **Logs:** Hata loglarını ve Stack Trace'i analiz et.
2.  **Hypothesis:** "Sorun X modülündeki Y fonksiyonunun null dönmesinden kaynaklanıyor" gibi net bir hipotez kur.
3.  **Trace:** Gerekirse geçici `console.log` veya `print` ekleyerek veri akışını izle.

## PHASE 3: THE FIX (Cerrahi Müdahale)
1.  **Action:** Sadece hatalı davranışı düzelten kodu yaz. (Refactoring yapma, sadece düzelt).
2.  **Side Effects:** Bu düzeltmenin başka bir yeri bozup bozmadığını düşün.

## PHASE 4: VERIFICATION
1.  **Test:** Phase 1'de yazdığın testi çalıştır. (PASS olmalı).
2.  **Cleanup:** Geçici logları temizle.
3.  **Report:** Hatayı ve çözümünü özetle.