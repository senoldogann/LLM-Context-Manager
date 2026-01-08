# Bilişsel Kod Matrisi (CCM): Sınırsız Bağlam İçin Teorik Temeller

## 1. Giriş: Bağlam Penceresi Paradoksu

Büyük Dil Modelleri (LLM), yazılım mühendisliğinde devrim yaratmış olsa da, temel bir kısıtlamayla karşı karşıyadır: **Bağlam Penceresi (Context Window)**. GPT-4 veya Gemini 1.5 gibi modeller milyonlarca token işleyebilse de, bu kapasite "sonsuz" değildir ve daha önemlisi, "etkin" değildir. 

### 1.1 Sorun Tanımı: "Ortada Kaybolma" (Lost-in-the-Middle)
Araştırmalar (Liu et al., 2023), LLM'lerin bağlam penceresinin ortasında yer alan bilgileri geri getirmede başarısız olduğunu göstermektedir. Kod tabanları lineer metinler değil, karmaşık bağlantılara sahip hiper-yapılardır. Lineer bir bağlam penceresine 100 dosyalık bir projeyi "flat" (düz) metin olarak doldurmak, modelin dikkat mekanizmasını (attention mechanism) boğmakta ve halüsinasyonlara yol açmaktadır.

### 1.2 Çözüm Önerisi: Bilişsel Kod Matrisi (CCM)
CCM, LLM'in bağlam penceresini "genişletmek" yerine, bu pencereye "nein gireceğini" akıllıca yöneten bir ara katmandır. Bu katman, biyolojik bilişsel süreçleri taklit eder:
1.  **Uzun Süreli Bellek (LTM):** Kod tabanının statik analizi (Graph).
2.  **Çalışma Belleği (Working Memory):** O anki görevle ilgili aktif düğümler.
3.  **Depisodik Bellek:** Kullanıcı etkileşimleri ve geçmiş kararlar.

## 2. CCM'in Üç Temel Sütunu

### 2.1 Kod Özellik Çizgeleri (Code Property Graphs - CPG)
Mevcut RAG (Retrieval-Augmented Generation) sistemleri kodu "chunk"lara bölüp vektör veritabanlarına atar. Bu yaklaşım, kodun yapısal bütünlüğünü (syntax ve semantics) yok eder. 

CCM, kodu **CPG** olarak saklar. CPG, üç grafiğin birleşimidir:
*   **AST (Abstract Syntax Tree):** Kodun sözdizimsel yapısı.
*   **CFG (Control Flow Graph):** Kodun çalışma sırası.
*   **PDG (Program Dependence Graph):** Veri ve değişkenlerin bağımlılıkları.

Bu sayede, "Bu değişken nerede tanımlandı?" sorusu, vektör benzerliğiyle değil, deterministik bir grafik sorgusuyla (Graph Traversal) %100 doğrulukla cevaplanır.

### 2.2 Ajan Tabanlı Bellek (Agentic Memory)
İşletim sistemlerinin bellek yönetimi (paging, swapping) prensiplerinden ilham alan bu katman, LLM'in bağlam penceresini bir "RAM" gibi kullanır.
*   **Letta/MemGPT Yaklaşımı:** LLM, kendisi bir "Memory Manager" ajanı gibi davranarak hangi bilgilerin bağlamda kalacağına, hangilerinin diske (Vektör/Graph DB) taşınacağına karar verir.
*   **Öz-Düzenleme:** Sistem, kullanıcının odaklandığı dosyaya göre grafikteki ilgili düğümleri "pre-fetch" (önceden getirme) yapar.

### 2.3 Spekülatif Geri Getirim (Speculative Retrieval)
Kullanıcı bir soru sormadan önce, sistem kullanıcının imlecini (cursor), son değiştirdiği dosyaları ve açık olan sekmeleri izleyerek niyetini tahmin eder.
*   **Öngörü:** Kullanıcı `user_controller.py` dosyasında `login` fonksiyonunu düzenliyorsa, sistem arka planda `auth_service.py` ve `user_model.py` dosyalarını hazırlar.
*   **Düşük Gecikme:** Kullanıcı sorusunu sorduğunda, bağlam çoktan hazırlanmıştır.

## 3. Mimari Bileşenler ve Akış

```mermaid
graph TD
    User[Developer] -->|IDE Action/Prompt| CCM_Client[VS Code Extension]
    CCM_Client -->|gRPC| CCM_Core[CCM Daemon (Rust)]
    
    subgraph "CCM Core Engine"
        Dispatcher --> SpecRet[Speculative Retrieval]
        Dispatcher --> GraphEngine[Graph Engine (Petgraph)]
        Dispatcher --> VectorEngine[Vector Store (LanceDB)]
        
        SpecRet -->|Cursor Context| Predictor[Intent Predictor]
        GraphEngine <-->|Read/Write| Storage[Embedded DB (Sled)]
        
        Predictor -->|Prefetch Hints| MemoryMgr[Agentic Memory Manager]
        MemoryMgr -->|Context Optimization| LLM_Interface[LLM Context Builder]
    end
    
    LLM_Interface -->|Optimized Context| External_LLM[GPT-4o / Claude 3.5]
```

## 4. Sonuç ve Gelecek Vizyonu
CCM, kodlama asistanlarını "otomatik tamamlama" araçlarından "bilişsel ortaklara" dönüştürmeyi hedefler. Sadece kodu değil, kodun *anlamını* ve *ilişkilerini* bilen bir sistem, yazılım mühendisliğinin geleceğidir.
