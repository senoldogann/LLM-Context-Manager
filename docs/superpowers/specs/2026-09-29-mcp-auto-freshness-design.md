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
| Hız | Parça vektörü yeniden kullanımı + manifest stat önbelleği + 300 ms debounce (bkz. Performans) |

## Bileşenler

### `ccm-core`

- `IndexManifest` → `#[serde(default)] indexed_at: Option<u64>` (unix saniye).
  `build_manifest` doldurur. Eski manifestlerde `None` olur; yeniden indeks
  tetiklenmez.
- `pub fn build_watch_filter(project_root: &Path, db_path: &Path) -> Result<WatchFilter>`
  ve `pub fn is_watch_relevant_path(filter: &WatchFilter, path: &Path) -> bool`
  - `is_index_relevant_file` (politika dışlaması, iç indeks dosyaları, binary
    uzantılar), manifest taramasıyla paylaşılan indeks artefakt listesi
    (`ccm_current`, `.ccm-generations`, kilit dizini, DB), araç durum dizinleri
    (`.ccm`, `.agent`) ve kök seviyesindeki `.gitignore`, `.ignore`,
    `.ccmignore`, `.git/info/exclude` eşleştiricisini birleştirir.
    (`is_index_relevant_file` generation yerleşimini tanımadığı için tek başına
    yetmez.)
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
  1. Olay bekle, 300 ms sessizlik olana kadar biriktir (debounce).
  2. `ServerState::project_index_lock` (manuel `index_now` ile aynı mutex) al.
  3. `run_index_worker_process(project, db, Full)` → worker `update_index`
     çalıştırır.
  4. Başarıda `get_engine` ile yeni generation önbelleğe alınır (generation
     değişmediyse önbellekten döner, graf yeniden yüklenmez); kuyrukta olay
     kalmadıysa `pending_paths = 0`, `last_error = None`,
     `semantic_unavailable = stats.semantic_unavailable`.
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
- `get_engine`: `index_job_in_progress` engeli yalnızca henüz hiç generation
  yokken (ilk indeksleme) uygulanır; var olan generation yeniden indeksleme
  sırasında okunmaya devam eder. İndeks hiç yoksa ve iş de yoksa mevcut
  "index missing" hatası aynen kalır.
- Engine önbelleği bir projenin yalnızca en yeni generation'ını tutar; yeni
  generation eklenince aynı projenin eski kayıtları düşürülür. Aksi halde her
  kayıtta yeni generation üreten otomatik yenileme aynı projenin 8 grafını
  bellekte biriktirir (Django'da her biri tam graf).
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

## Performans

Sistem hızlı olmalı; hedefler ölçülmüş başlangıç değerlerine dayanır.

**Ölçüm** (2026-09-29, Apple Silicon, release build, Django 5.1: 6.815 dosya,
graf JSON 82 MB; embedder kapalı ölçülüp embedding ayrıca ölçüldü):

| Ölçüm | Süre |
|---|---|
| Tam indeks (graf-only) | 4,2 sn |
| Değişiklik yokken `update_index` | ~1,0 sn (neredeyse tamamı tüm dosyaları yeniden hash'leme) |
| Tek dosya düzenleme (`query.py`, 175 semantik düğüm), graf-only | 2,2 sn |
| ↳ graf yükleme + tam ağaç hash + DB kopyası | ~1,25 sn |
| ↳ parse + tüm referans kenarlarını yeniden kurma (226k kenar) | ~0,63 sn |
| ↳ kaydetme + generation geçişi | ~0,26 sn |
| Aynı düzenlemenin embedding'i (`mxbai-embed-large`, Ollama, ~63 ms/parça; batch hızlandırmıyor) | ~11 sn |
| MCP'de yeni generation'ı yükleyen ilk sorgu / önbellekli sorgu | 356 ms / 85 ms |

Bugün Django'da kaydet → taze sonuç: ~3 sn (graf-only), ~14 sn (embedding
açık). Kök neden: değişen dosyanın **bütün** parçaları yeniden embed ediliyor
(`engine.rs` `incremental_index_paths` → `delete_by_prefix` + tüm semantik
düğümler) ve her çalışmada tüm dosyalar yeniden okunup hash'leniyor.

**Hedefler** (tek dosyalık tipik düzenleme, embedder açık, kaydet → taze):

| Repo | Hedef |
|---|---|
| ≤ 2k dosya | ≤ 1 sn |
| Django boyutu (~7k dosya) | ≤ 2 sn (bekleme bütçesinin içinde) |
| Değişiklik bulmayan yenileme, Django | ≤ 0,4 sn |

**P0.1 kapsamına alınan hızlandırmalar**

1. **Parça metnine göre vektör yeniden kullanımı.** Değişen dosyanın mevcut
   satırları (`text`, `vector`) silinmeden önce okunur. Yeni parçalardan
   metni birebir aynı olanlar eski vektörü kullanır; yalnızca değişen parçalar
   embed edilir. Embedding metnin saf fonksiyonu olduğu için sonuç aynıdır.
   Şema değişmez. `add_documents` bilinen vektörleri açık bir parametre olarak
   alır (tam indeks boş harita geçer). Fixture modu değişmez.
   `IndexStats`'a `embedded_chunks` ve `reused_chunks` sayaçları eklenir ve
   indeks çıktısında gösterilir. Beklenen: 175 → 1–3 parça (~11 sn → ~0,1 sn).
2. **Manifest stat önbelleği.** `(modified_sec, modified_nsec, size)` önceki
   manifestle aynıysa içerik hash'i dosya okunmadan önceki manifestten alınır.
   Racy durum için git'in kuralı uygulanır: dosyanın mtime'ı önceki
   `indexed_at`'e 1 sn'den yakın veya sonraysa yeniden hash'lenir;
   `indexed_at` olmayan manifestlerde her dosya hash'lenir.
   Beklenen: ~0,9 sn → ~0,05 sn.
3. **Yüklü grafın staging'de yeniden kullanımı.** `update_index` grafı zaten
   başta aktif generation'dan yüklüyor; staging için JSON'u kopyalayıp ikinci
   kez ayrıştırmak yerine bu graf kullanılır (Django'da ~0,25 sn).
4. **Debounce 300 ms.**

Tahmini Django sonucu ~1,6 sn. Kalan maliyet graf JSON gidiş-dönüşü (worker
yükleme/kaydetme + sunucunun yeniden yüklemesi, ~0,55 sn) ve tüm referans
kenarlarının yeniden kurulması (~0,6 sn); ikisi de P0.4 (grafın bellekte
tutulması) kapsamında.

**Kapsam dışı (P0.2):** embedding modelinin/çalışma zamanının kendisi; model
kimliğinin manifestte tutulup model değişince tam yeniden embed zorlanması
(bugün de değişen ve değişmeyen dosyalar farklı modellerle karışabiliyor;
yeniden kullanım bunu kötüleştirmez).

## Hata yönetimi

- **Watcher açılamazsa** (inotify limiti, izin, silinmiş kök): yapılandırılmış
  alanlarla `warn` log'u, durum `Unavailable(sebep)`; her okuma sonucunda
  görünür. Sessizce yutulmaz.
- **Worker hatası** (embedder, "Index changed concurrently", zaman aşımı):
  en fazla 3 deneme; denemeler arasında 1 sn ve 2 sn; her başarısız denemede
  `warn` log'u. Sonra son hata `last_error`'a yazılır ve sonuçta gösterilir;
  okumalar mevcut generation'dan devam eder. Sonraki dosya olayı yeniden dener.
- **Watcher çalışırken hata olayı**: `warn` log'u ve olay kaybı ihtimaline
  karşı tam karşılaştırmalı yenileme; yenileme başarılıysa indeks tutarlıdır.
- **Eşzamanlı yazarlar** (CLI `--watch`, detached semantic upgrade): mevcut
  activation lock + pointer CAS korur; CAS hatası yukarıdaki deneme yolundan
  geçer. İndeks çıktı dizini filtrelendiği için bu yazarlar döngü tetiklemez.
- **Hızlı indeksin semantik yükseltmesi sürerken** otomatik yenileme
  `update_index` çalıştırmaz: yükseltme sürerken vektör tablosu eksik
  göründüğü için `update_index` onarıma ya da tam yeniden indekslemeye girer
  ve aynı embedding işini ikinci kez yapar. Yenileme ertelenir, okumalar
  beklemez, satır `waiting for semantic upgrade` gösterir; yükseltme bitince
  (başarılı ya da değil) ertelenen değişiklikler tek yenilemede işlenir.
  Bilinen sınır: MCP süreci yükseltme sürerken yeniden başlarsa bu kayıt
  kaybolur ve ilk okuma yakalaması onarımı başlatabilir.
- **Bilinen sınır (P0.2 kapatır):** embedding servisi kapalıyken değişiklik
  içeren her yenileme PR #3'teki yoldan tam graf-only yeniden indekslemeye
  gider (Django'da ~4 sn); hedefler embedder açıkken tanımlıdır.
- **Manuel `index_now`**: aynı proje mutex'ini bekler; bittiğinde engine
  yenilenir ve otomatik yenilemeden bir tam karşılaştırma istenir, böylece
  önceki başarısız yenilemeden kalan hata satırı temizlenir.

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
- `mcp/tests/mcp_integration_test.rs`: yeni test, yeniden indeksleme sürerken
  okumanın mevcut generation'dan hatasız döndüğünü doğrular; ilk indekslemede
  "indexing is in progress" bekleyen mevcut test değişmeden geçer.
- `core/tests/incremental_filesystem_test.rs` — vektör yeniden kullanımı: çok
  fonksiyonlu bir dosyada tek fonksiyonu değiştir; `update_index` sonrası
  `embedded_chunks` yalnızca değişen parçaları, `reused_chunks` geri kalanını
  sayar ve arama sonuçları doğru kalır. CI'da Ollama olmadığı için test içinde
  deterministik vektör döndüren küçük bir yerel HTTP embedding sunucusu
  kullanılır (P0.2'deki yerleşik embedder gelince gerçek embedder'a geçer).
- `core/tests/incremental_filesystem_test.rs` — stat önbelleği: aynı boyutta
  içerik değişikliği (racy pencere içinde) algılanır; dokunulmayan dosyalar
  değişmiş sayılmaz.
- Kapılar: `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo fmt --all -- --check`, CI'daki iki eval kapısı.
- **Performans kapısı:** `/tmp` altındaki Django 5.1 klonunda, release build
  ile Performans bölümündeki ölçümler tekrarlanır ve hedeflerle
  karşılaştırılır. Hedef tutmazsa sebep ölçümle raporlanır; kapsam genişletme
  (P0.4'ten parça çekme) kullanıcıya sorulur.
