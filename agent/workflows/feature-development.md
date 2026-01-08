---
description: End-to-End Feature Implementation Workflow for Senior Architect
---

# 🏗️ FEATURE DEVELOPMENT PIPELINE

Bu akış, bir Senior Architect'in bir özelliği geliştirirken izlediği zihinsel süreci simüle eder.

## STEP 1: 🕵️‍♂️ Discovery & Architecture (Sequential)
1.  **Read Context:** İlgili dosyaları oku.
2.  **Plan:** Yapılacak değişikliği planla.
3.  [cite_start]**Risk Analysis:** `.gemini/GEMINI.md` dosyasındaki "2-Year Horizon" kuralına göre riskleri değerlendir. [cite: 61]

## STEP 2: 🧱 Implementation (Parallel Capable)
1.  **Instruction:** Planlanan dosyaları oluştur veya güncelle.
2.  **Strategy:** Eğer Frontend bileşeni ve Backend servisi birbirinden bağımsızsa, bu dosyaları **PARALEL** olarak oluştur (Bkz: `10-parallel-execution.md`).

## STEP 3: 🧪 Verification & Quality Gate (Sequential)
1.  [cite_start]**Test Generation:** Kod ile eş zamanlı olarak Unit Test'leri yaz (AAA Pattern). [cite: 128]
2.  **Execution:** Testleri çalıştır.
3.  **Fix:** Hata varsa düzelt ve tekrar test et.

## STEP 4: 📝 Final Delivery
1.  Yapılan işi özetle.
2.  Kullanıcıya sonraki adımı (Next Step) öner.