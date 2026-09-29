# MCP Otomatik Tazelik (P0.1) — Tasarım

## Amaç

Ajan kod aradığında indeks diskteki kodu yansıtmalı; geliştirici `index_now`
çağırmak zorunda kalmamalı. Her okuma sonucu indeksin tazeliğini göstermeli.

**Başarı ölçütleri**

- Dosya kaydedildikten birkaç saniye sonra `search_code` / `find_nodes` yeni
  sembolü buluyor.
- Yenileme sürerken okuma araçları hata vermiyor; mevcut generation'dan cevap
  veriyor.
- Her okuma sonucu tek satırlık tazelik bilgisi taşıyor (taze/bayat, bekleyen
  dosya sayısı, yenileme durumu, gerektiğinde indeks yaşı).

**Kapsam dışı**

- CLI `--watch` değişmez.
- Genel eşzamanlı istek işleme (P0.3) ve artımlı güncellemedeki tam ağaç
  hash'leme / LanceDB kopyasının kaldırılması (P0.4).
- İndeksi hiç olmayan projeler için otomatik tam indeksleme (embedding maliyeti
  nedeniyle açık kullanıcı kararı olarak kalır).

## Kararlar

| Konu | Karar |
|---|---|
| Yaklaşım | Proje başına `notify` watcher + arka plan yenileme görevi |
| Bayat sorgu | En fazla 2 sn yenilemenin bitmesini bekle; yetişmezse mevcut generation'dan cevap ver ve bayat olarak işaretle |
| Yenileme | Tek aşamalı: mevcut worker süreci üzerinden `update_index`; embedder kapalıysa PR #3'teki graf-only yol |
| Varsayılan | Açık; `CCM_AUTO_REFRESH=0` ile kapanır |
| Okuma engeli | `get_engine` içindeki "indexing is in progress" engeli kaldırılır (manuel işler dahil) |
| Şema | `IndexManifest.indexed_at` `serde(default)` ile opsiyonel; `schema_version` değişmez |

## Bileşenler

### `ccm-core`

- `IndexManifest` → `#[serde(default)] indexed_at: Option<u64>` (unix saniye).
  `build_manifest` doldurur. Eski manifestlerde `None` olur; yeniden indeks
  tetiklenmez.
- `pub fn is_watch_relevant_path(project_root: &Path, path: &Path) -> bool`
  - `is_index_relevant_file` (politika dışlaması, iç indeks dosyaları, binary
    uzantılar) ile kök seviyesindeki `.gitignore`, `.ignore`, `.ccmignore`,
    `.git/info/exclude` eşleştiricisini birleştirir.
  - Amaç: `target/` gibi build çıktılarının ve indeksin kendi çıktı dizininin
    (`data/`, `.ccm`) yenileme tetiklememesi. İndeks çıktısının filtrelenmesi
    zorunludur; aksi halde yenileme kendi yazdığı dosyalarla döngüye girer.
  - Eşleştirici watcher başlarken bir kez kurulur. İç içe `.gitignore`
    dosyaları kapsanmaz; kaçan bir olay yalnızca değişiklik bulmayan bir
    yenileme maliyeti yaratır (walker bu dosyaları zaten uygular).
- Okuma tarafı için manifestten `indexed_at` okuyan küçük bir public fonksiyon.
  Engine yüklenirken generation başına bir kez okunur ve engine ile birlikte
  cache'lenir; her istekte manifest parse edilmez.

### `ccm-mcp` — yeni `mcp/src/freshness.rs`

- `WatcherStatus`: `Active` | `Disabled` (env ile kapalı) | `Unavailable(String)`.
- `ProjectFreshness` (tek proje için anlık durum, `Clone`):
  - `watcher: WatcherStatus`
  - `pending_paths: usize`
  - `refresh_in_flight: bool`
  - `last_error: Option<String>`
  - `semantic_unavailable: Option<String>`
- `FreshnessHandle`: `tokio::sync::watch::Sender<ProjectFreshness>` + olay
  kanalı. Okumalar `watch::Receiver` ile durum değişimini bekler.
- `start_project_watcher(...)`: `notify::recommended_watcher` açar, kökü
  recursive izler, olayları `is_watch_relevant_path` ile filtreleyip mpsc
  kanalına yollar. `need_rescan` bayraklı olaylar filtre atlanarak yenileme
  tetikler.
- `run_refresh_loop(...)`: proje başına **tek** tokio görevi.
  1. Olay bekle, 500 ms sessizlik olana kadar biriktir (debounce).
  2. `ServerState::project_index_lock` (manuel `index_now` ile aynı mutex) al.
  3. `run_index_worker_process(project, db, Full)` → worker `update_index`
     çalıştırır.
  4. Başarıda `refresh_project_engine`; `pending_paths = 0`,
     `last_error = None`, `semantic_unavailable = stats.semantic_unavailable`.
  5. Çalışma sırasında yeni olay geldiyse 1'e dön.
  Tek görev olduğu için aynı projede yenilemeler sıraya girer.
- `wait_until_fresh(handle, Duration)`: durum temizse (`pending_paths == 0` ve
  `!refresh_in_flight`) hemen döner; değilse süre dolana kadar watch kanalını
  dinler. Sonuç olarak son `ProjectFreshness` döner. Watcher `Disabled` veya
  `Unavailable` ise beklemeden döner (beklenecek bir yenileme yoktur).
- `format_freshness_line(&ProjectFreshness, indexed_at: Option<u64>, now: u64) -> String`:
  saf fonksiyon.

### `ServerState` / araçlar

- `freshness: std::sync::Mutex<HashMap<String, Arc<FreshnessHandle>>>`
  (anahtar: kanonik proje yolu).
- İzlenen proje sayısı `CCM_MCP_ENGINE_CACHE_SIZE` ile sınırlı (varsayılan 8);
  aşılırsa durum `Unavailable("watcher limit reached")`.
- `get_engine`: `index_job_in_progress` engeli ve `server.rs`'teki buna bağlı
  hata eşlemesi kaldırılır. İndeks hiç yoksa mevcut "index missing" hatası
  aynen kalır.
- Okuma araçları (`get_context`, `search_code`, `find_nodes`, `read_graph`,
  `find_usages`, `trace_call_chain`, `impact_of_change`, `diff_context`):
  `wait_until_fresh` → `get_engine` → çıktının başına tazelik satırı.
- `mcp/Cargo.toml`: `notify = "6.1"` (CLI ile aynı sürüm).

## Veri akışı

1. **İzleme başlangıcı:** Bir proje için `get_engine` ilk kez başarılı
   olduğunda ya da `index_now` / `index_project` başarıyla bittiğinde watcher
   ve yenileme görevi başlar; bir kez yenileme tetiklenir (sunucu kapalıyken
   yapılan değişiklikleri yakalamak için). `CCM_AUTO_REFRESH=0` ise hiçbiri
   başlamaz, durum `Disabled` olur.
2. **Değişiklik:** filtreden geçen olay `pending_paths`'i artırır; debounce
   sonrası worker çalışır. Branch değişimi gibi toplu olaylar tek yenilemede
   birleşir.
3. **Okuma:** `wait_until_fresh(2s)` → `get_engine` (yeni generation
   `ccm_current` üzerinden zaten seçilir) → tazelik satırı + mevcut çıktı. Ana
   döngü bu bekleme boyunca sıradaki isteği okumaz; P0.3 bunu çözecek.
4. **Tazelik satırı** (mevcut çıktılar gibi İngilizce):
   - `_Index: fresh · auto-refresh on_`
   - `_Index: stale · 4 changed files pending · refresh running · indexed 3m ago_`
   - `_Index: stale · last refresh failed: <hata> · indexed 10m ago_`
   - `_Index: auto-refresh unavailable (<sebep>) · indexed 2h ago_`
   - `_Index: auto-refresh off · indexed 5m ago_`
   - Graf-only modda sona `· semantic search unavailable: <sebep>` eklenir.
   - `indexed_at` yoksa yaş parçası yazılmaz.

## Hata yönetimi

- **Watcher açılamazsa** (inotify limiti, izin, silinmiş kök): yapılandırılmış
  alanlarla `warn` log'u, durum `Unavailable(sebep)`; her okuma sonucunda
  görünür. Sessizce yutulmaz.
- **Worker hatası** (embedder, "Index changed concurrently", zaman aşımı):
  1 s / 2 s / 4 s aralıkla en fazla 3 deneme, her denemede `warn` log'u.
  Sonra son hata `last_error`'a yazılır ve sonuçta gösterilir; okumalar
  mevcut generation'dan devam eder. Sonraki dosya olayı yeniden dener.
- **Watcher çalışırken hata olayı**: `last_error`'a yazılır ve olay kaybı
  ihtimaline karşı yenileme tetiklenir.
- **Eşzamanlı yazarlar** (CLI `--watch`, detached semantic upgrade): mevcut
  activation lock + pointer CAS korur; CAS hatası yukarıdaki deneme yolundan
  geçer. İndeks çıktı dizini filtrelendiği için bu yazarlar döngü tetiklemez.
- **Manuel `index_now`**: aynı proje mutex'ini bekler; bittiğinde engine
  yenilenir ve tazelik durumu güncellenir.

## Test

Mevcut strateji korunur; yeni testler gerçek MCP binary'si üzerinden stdio ile
çalışan entegrasyon testleridir (`CCM_DISABLE_EMBEDDER=1`).

- `mcp/tests/auto_refresh_test.rs`:
  1. Geçici projeyi indeksle, dosyaya yeni fonksiyon ekle; `find_nodes` 10 sn
     içinde sembolü bulur ve satır `fresh` içerir.
  2. `CCM_INTERNAL_INDEX_TEST_DELAY_MS=5000` ile dosyayı değiştir; `find_nodes`
     ~2 sn'de hata vermeden döner, satır `stale` ve `refresh running` içerir.
  3. `CCM_AUTO_REFRESH=0`: değişiklik yansımaz, satır `auto-refresh off` içerir.
  4. `.gitignore`'daki dizine (`target/`) yazmak `pending_paths`'i artırmaz
     (satır `fresh` kalır).
- `core/tests/incremental_filesystem_test.rs`: `is_watch_relevant_path` için
  gerçek geçici dizinle senaryo (gitignore'lu yol, `.ccmignore`, indeks çıktı
  dizini, normal kaynak dosya).
- `mcp/tests/mcp_integration_test.rs`: "indexing is in progress" bekleyen test,
  iş sürerken mevcut generation'dan cevap verildiğini doğrulayacak şekilde
  güncellenir.
- Kapılar: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`, CI'daki iki eval kapısı.
- **Ölçüm:** Büyük bir gerçek repoda (≥5k dosya) kaydetme → `fresh` süresi
  ölçülür. Tipik değer 2 sn'yi aşıyorsa P0.4'ün "yalnızca watcher'ın bildirdiği
  yolları hash'le" kısmının bu işe çekilmesi kullanıcıya sorulur.
