---
description: "High-level System Architecture Design & Blueprinting Protocol."
---

# 🏗️ SYSTEM ARCHITECTURE DESIGN PROTOCOL

**TRIGGER:** Kullanıcı "sistem tasarımı yap", "mimari çiz", "altyapıyı planla" veya "scale etmemiz lazım" dediğinde tetiklenir.

## PHASE 1: REQUIREMENT ANALYSIS (NFRs)
Kod yazmadan veya çizim yapmadan önce şu "Non-Functional Requirements" (Fonksiyonel Olmayan Gereksinimler) setini netleştir:
1.  **Scalability:** Dikey (Vertical) mi Yatay (Horizontal) mı büyüyeceğiz? (K8s vs Serverless).
2.  **Latency:** Gerçek zamanlı mı (Kafka/WebSockets) yoksa Asenkron mu (RabbitMQ/SQS)?
3.  **Consistency:** ACID (SQL) mi yoksa BASE (NoSQL/Redis) mi gerekiyor? CAP teoremindeki yerimiz ne?

## PHASE 2: VISUALIZATION (Mermaid.js)
Mimariyi anlatmak yasaktır, **ÇİZMEK** zorunludur.
* Her zaman **Mermaid.js** sözdizimi kullanarak `graph TD` veya `C4Context` diyagramı üret.
* **Zorunlu Bileşenler:** Diyagramda sadece servisleri değil; **Cache (Redis)**, **Message Broker (Kafka)**, **API Gateway** ve **Database** katmanlarını ayrı ayrı göster.

## PHASE 3: INFRASTRUCTURE AS CODE (IaC)
Sadece teorik konuşma. Bu mimariyi ayağa kaldıracak konfigürasyon taslaklarını sun:
* **Containerization:** `Dockerfile` stratejisi (Multi-stage builds).
* **Orchestration:** `k8s/deployment.yaml` veya `docker-compose.yml` taslağı.
* **Observability:** Prometheus/Grafana veya OpenTelemetry entegrasyon stratejisi.

## PHASE 4: DEFENSE IN DEPTH (Security)
Tasarladığın mimarideki "Single Point of Failure" (Tekil Hata Noktası) risklerini belirle ve "Circuit Breaker" desenini nerede kullanacağını belirt.