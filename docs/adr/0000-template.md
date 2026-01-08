# [Kısa Başlık, örn: Veritabanı Olarak PostgreSQL Kullanımı]

* **Durum:** [Öneri | Kabul Edildi | Reddedildi]
* **Tarih:** [YYYY-MM-DD]
* **Bağlam:** [Hangi sorunu çözüyoruz? İhtiyaçlar ne?]

## Seçenekler
1.  **[Seçenek A]** (örn: MongoDB) - Esnek ama Transaction zayıf.
2.  **[Seçenek B]** (örn: PostgreSQL) - Güçlü Transaction, JSONB desteği var.

## Karar
**[Seçenek B]** seçilmiştir.

## Neden?
Çünkü projemiz finansal veriler içeriyor ve ACID uyumluluğu (Transaction güvenliği) NoSQL esnekliğinden daha kritik.