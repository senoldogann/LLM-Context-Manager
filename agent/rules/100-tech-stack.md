# PROJE KURALLARI: Bilişsel Kod Matrisi (CCM)

> **SYSTEM OVERRIDE:** Bu dosya projenin TEKNİK ANAYASASIDIR. Değiştirilmesi için Kıdemli Mimar onayı gerekir.

## 1. ÇEKİRDEK TEKNOLOJİ YIĞINI (CORE STACK)

### 1.1 Programlama Dilleri
*   **Core Engine (CCM Daemon):** **Rust** (2021 edition veya üstü).
    *   *Neden:* "Embedded", "Local" ve "Low-Latency" gereksinimleri için bellek güvenliği ve performans şart. Garbage Collector duraksamaları kabul edilemez.
*   **Glue & Research:** **Python 3.10+**.
    *   *Kullanım:* ML model prototipleme, veri analizi ve karmaşık NLP işlemleri için yan süreçler.
*   **Client/Interface:** **TypeScript** (Node.js/Electron env).
    *   *Kullanım:* VS Code / Cursor editör eklentileri ve arayüzler.

### 1.2 Veri & Bellek Yönetimi (Memory & Storage)
*   **Graph Database (CPG):** **Petgraph** (Rust In-Memory) + **Sled** (Embedded Key-Value Persistence).
    *   *Kural:* Ağır veritabanı sunucuları (Neo4j vb.) KESİNLİKLE yasaktır. Sistem "gömülü" (embedded) olmalıdır.
*   **Vector Store:** **LanceDB** veya **Qdrant (Embedded mode)**.
    *   *Kural:* Vektör aramaları yerel diske dayalı ve bellek dostu olmalıdır.
*   **IPC (Inter-Process Communication):** **gRPC** (Tonic - Rust) veya **Unix Domain Sockets**.

### 1.3 Yapay Zeka & LLM Entegrasyonu
*   **Inference:** **Candle** (HuggingFace Rust ML framework) veya **ONNX Runtime**.
    *   *Hedef:* Mümkünse yerel küçük modelleri (SLM) doğrudan Rust içinden çalıştırmak.
*   **Orchestration:** **LangChain (Python sidecar)** veya Rust içinde özel `Agent` trait'leri.

## 2. YAZILIM MİMARİSİ İLKELERİ

### 2.1 "Panic" Yok (No Panic Policy)
*   Rust kodunda `unwrap()` kullanımı **KESİNLİKLE YASAKTIR**.
*   Her hata `Result<T, AppError>` ile yönetilmeli ve uygun şekilde loglanmalıdır.

### 2.2 Async-First
*   gRPC sunucusu ve dosya I/O işlemleri tamamen asenkron (`Tokio` runtime) olmalıdır.

### 2.3 Tip Güvenliği (Type Safety)
*   Domain modelleri "NewType" deseni ile sarmalanmalıdır (örn: `struct UserId(String)` yerine `struct UserId(Uuid)`).
*   Stringly-typed API'lerden kaçınılmalıdır.

## 3. FILE SYSTEM & PROJECT STRUCTURE
*   `/core`: Rust motoru kodu.
*   `/sdk`: TypeScript ve Python istemci kütüphaneleri.
*   `/proto`: Protobuf tanımları (Single Source of Truth).
*   `/models`: Yerel model ağırlıkları ve konfigürasyonları.

## 4. PERFORMANS HEDEFLERİ
*   **Cold Start:** < 500ms.
*   **Sorgu Cevap Süresi (Retrieval):** < 100ms (P99).
*   **Bellek Ayak İzi (Idle):** < 200MB.
