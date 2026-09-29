# MCP Otomatik Tazelik (P0.1) Uygulama Planı

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** MCP sunucusu kaydedilen dosyaları kendiliğinden yeniden indekslesin, her okuma sonucu tek satırlık tazelik bilgisi taşısın ve tek dosyalık bir düzenleme Django boyutunda (~7k dosya) bir repoda ≤ 2 sn içinde sorgulanabilir olsun.

**Architecture:** `ccm-core` iki hızlandırma (manifest stat önbelleği, parça metnine göre vektör yeniden kullanımı) ve bir watcher filtresi kazanır. `ccm-mcp` içindeki yeni `freshness` modülü proje başına bir `notify` watcher'ı ile tek bir tokio yenileme görevi çalıştırır; yenileme mevcut index worker sürecini (`update_index`) proje kilidi altında kullanır. Okuma araçları en fazla 2 sn bekler, gerekiyorsa yeni generation'ı yükler ve çıktının başına tazelik satırı ekler. Engine önbelleği bir projenin yalnızca en yeni generation'ını tutar.

**Tech Stack:** Rust 2021, tokio 1.53.1 (`sync::watch`, `sync::mpsc`), notify 6.1.1, ignore 0.4.31 (`gitignore::GitignoreBuilder`), lancedb 0.31.0, serde_json.

**Spec:** `docs/superpowers/specs/2026-09-29-mcp-auto-freshness-design.md`

## Global Constraints

- Okuma bekleme bütçesi: `FRESHNESS_WAIT_BUDGET = Duration::from_secs(2)`.
- Debounce: 300 ms sessizlik (`DEBOUNCE = Duration::from_millis(300)`).
- Yenileme denemesi: en fazla 3 deneme; denemeler arasında 1 sn ve 2 sn; her başarısız denemede `warn` log'u; sonra son hata `last_error` olur ve okumalar mevcut generation'dan devam eder.
- `CCM_AUTO_REFRESH`: varsayılan açık; `0`, `false`, `no`, `off` (büyük/küçük harf duyarsız) kapatır.
- İzlenen proje sınırı `CCM_MCP_ENGINE_CACHE_SIZE` (varsayılan 8); aşılınca durum `Unavailable("watcher limit reached")`.
- `notify = "6.1"` (CLI ile aynı; lock'ta 6.1.1).
- `INDEX_SCHEMA_VERSION` 4 kalır; yeni manifest ve `IndexStats` alanları `#[serde(default)]`.
- Tazelik satırı biçimi: `_Index: <parçalar " · " ile birleşik>_`. Parçalar birebir: `fresh`, `auto-refresh on`, `stale`, `N changed file(s) pending`, `refresh running`, `waiting for semantic upgrade`, `last refresh failed: <hata>`, `auto-refresh off`, `auto-refresh unavailable (<sebep>)`, `indexed <yaş> ago`, `semantic search unavailable: <sebep>`.
- Hızlı (quick) indeksin semantik yükseltmesi sürerken otomatik yenileme `update_index` çalıştırmaz (aksi halde eksik vektör tablosunu onarmaya/tam yeniden indekslemeye girip yükseltmenin embedding işini ikinci kez yapar); okumalar beklemez; yükseltme bitince ertelenen değişiklikler tek yenilemede işlenir.
- Araç çıktıları İngilizce, kod yorumları Türkçe (repo stili). Loglar yapılandırılmış alan kullanır.
- Saf fonksiyonlar girdilerini değiştirmez; yalnızca dönüş değeri üretir.
- Ortam değişkeni değiştiren core testleri `ENV_LOCK`'u tutar ve değişkenleri `Drop` ile geri yükler.
- Yeni testler entegrasyon testidir; yeni unit test eklenmez.
- Kapılar: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`; eval kapıları Task 7'de.
- Performans hedefleri (tek dosyalık düzenleme, kaydet → taze): ≤ 2k dosya ≤ 1 sn; Django ≤ 2 sn; değişiklik bulmayan yenileme Django ≤ 0,4 sn.
- Commit mesajları Conventional Commits; her commit şu satırla biter: `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>`.

## Review Focus

1. **Editörlerin atomik kaydı** (geçici dosyaya yaz + `rename`): hedef dosya değişmiş sayılmalı, yeni sembol görünmeli → Task 6 testi `atomic_rename_save_is_picked_up`.
2. **Toplu değişiklik** (branch değişimi, yüzlerce dosya): olaylar tek yenilemede birleşmeli, sonunda hepsi görünmeli, arada okumalar hata vermemeli → Task 6 testi `bulk_change_coalesces_and_settles`.
3. **Embedding servisi kapalıyken otomatik yenileme**: graf tazelenmeli, satır `semantic search unavailable` göstermeli → Task 6 testi `graph_only_refresh_reports_semantic_notice`.
4. **Otomatik yenileme sürerken elle `index_now`**: proje kilidi sayesinde ikisi de hatasız bitmeli ("Index changed concurrently" olmamalı) → Task 6 testi `manual_index_during_auto_refresh_succeeds`.
5. **Boşluklu yollar ve symlink'li proje kökü** (macOS `/var` → `/private/var`): filtre ve proje anahtarı kanonik yolda eşleşmeli → Task 3 testinde `src/my file.rs`; Task 6 testleri proje yolunu kanonik olmayan tempdir yolu olarak verir.

---

### Task 1: Manifest zaman damgası, stat önbelleği ve yüklü grafın staging'de yeniden kullanımı

**Files:**
- Modify: `core/src/lib.rs` — `IndexManifest` (~1527), `build_index_generation` (~370-395 ve ~595), `update_index` (~1076 ve ~1183-1187), `fingerprint_for_path` çevresi (~1607), `build_manifest` (~1675), test modülündeki `IndexManifest` literalleri (~2004, ~2017)
- Test: `core/tests/incremental_filesystem_test.rs`

**Interfaces:**
- Produces:
  - `pub fn read_index_timestamp(manifest_path: &Path) -> anyhow::Result<Option<u64>>` (Task 5 kullanır)
  - `pub fn unix_now_secs() -> u64` (Task 5 ve 6 kullanır)
  - `fn index_artifact_paths(artifact_parent: &Path, requested_db_path: &Path) -> Vec<PathBuf>` (crate içi; Task 3 kullanır)

- [ ] **Step 1: Başarısız testleri yaz**

`core/tests/incremental_filesystem_test.rs` sonuna ekle:

```rust
#[tokio::test]
async fn update_index_detects_same_size_edit_inside_racy_window() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = tempdir()?;
    let file = project.path().join("lib.rs");
    std::fs::write(&file, "fn alpha() {}\n")?;
    ccm_core::index_directory(project.path().to_string_lossy().as_ref(), None).await?;
    let manifest_path = artifacts(project.path(), None)?.manifest_path;
    assert!(ccm_core::read_index_timestamp(&manifest_path)?.is_some());

    // Aynı boyutta içerik değişikliği; mtime geri yüklenerek stat bilgisi
    // birebir korunur. İndeks az önce alındığı için dosya racy penceresindedir
    // ve içerik yeniden hash'lenmelidir.
    let original_mtime = std::fs::metadata(&file)?.modified()?;
    std::fs::write(&file, "fn gamma() {}\n")?;
    std::fs::OpenOptions::new()
        .write(true)
        .open(&file)?
        .set_modified(original_mtime)?;
    ccm_core::update_index(project.path().to_string_lossy().as_ref(), None).await?;

    let paths = artifacts(project.path(), None)?;
    let graph = CodeGraph::from_file(paths.graph_path.to_string_lossy().as_ref())?;
    assert!(graph.graph.node_weights().any(|node| node.name == "gamma"));
    assert!(!graph.graph.node_weights().any(|node| node.name == "alpha"));
    Ok(())
}

#[tokio::test]
async fn update_index_trusts_unchanged_stat_outside_racy_window() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = tempdir()?;
    let file = project.path().join("lib.rs");
    std::fs::write(&file, "fn alpha() {}\n")?;
    let an_hour_ago = std::time::SystemTime::now() - std::time::Duration::from_secs(3_600);
    std::fs::OpenOptions::new()
        .write(true)
        .open(&file)?
        .set_modified(an_hour_ago)?;
    ccm_core::index_directory(project.path().to_string_lossy().as_ref(), None).await?;

    // Git ile aynı ödünleşim: mtime ve boyut birebir korunmuşsa ve dosya racy
    // pencerenin dışındaysa içerik okunmaz. Bu test hızlı yolun devrede
    // olduğunu sabitler; yol kapanırsa performans sessizce geriler.
    std::fs::write(&file, "fn gamma() {}\n")?;
    std::fs::OpenOptions::new()
        .write(true)
        .open(&file)?
        .set_modified(an_hour_ago)?;
    let stats = ccm_core::update_index(project.path().to_string_lossy().as_ref(), None).await?;

    assert_eq!(stats.files_indexed, 0, "unchanged stat must skip re-hashing");
    Ok(())
}
```

- [ ] **Step 2: Testlerin başarısız olduğunu doğrula**

Run: `cargo test -p ccm-core --test incremental_filesystem_test update_index_`
Expected: derleme hatası `cannot find function 'read_index_timestamp' in crate 'ccm_core'`.

- [ ] **Step 3: Manifest alanı, zaman yardımcıları ve okuma fonksiyonu**

`core/src/lib.rs` içinde `IndexManifest`'i şu hale getir ve hemen altına sabit + fonksiyonları ekle:

```rust
#[derive(Debug, Default, Serialize, Deserialize)]
struct IndexManifest {
    #[serde(default)]
    schema_version: u32,
    #[serde(default)]
    indexed_commit: Option<String>,
    /// Taramanın başladığı an (unix saniye). Stat önbelleğinin racy penceresi
    /// ve MCP tazelik satırındaki indeks yaşı bu değere dayanır.
    #[serde(default)]
    indexed_at: Option<u64>,
    files: HashMap<String, FileFingerprint>,
}

/// Kaba zaman çözünürlüklü dosya sistemlerinde (FAT: 2 sn) aynı pencerede
/// yapılan düzenlemeler kaçmasın diye stat önbelleği bu aralığa giren
/// dosyaları yeniden hash'ler (git'in "racy clean" kuralı).
const RACY_WINDOW_SECS: u64 = 2;

/// Şu anki zamanı unix saniye olarak döndürür.
pub fn unix_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}

/// Manifestteki indeksleme zamanını okur; alan yoksa (eski manifest) `None`.
/// Dosya listesi ayrıştırılmadan atlandığı için büyük manifestlerde de ucuzdur.
pub fn read_index_timestamp(manifest_path: &Path) -> Result<Option<u64>> {
    #[derive(Deserialize)]
    struct ManifestTimestamp {
        #[serde(default)]
        indexed_at: Option<u64>,
    }
    let file = std::fs::File::open(manifest_path).map_err(|error| {
        anyhow::anyhow!(
            "Index manifest '{}' could not be opened: {}",
            manifest_path.display(),
            error
        )
    })?;
    let parsed: ManifestTimestamp = serde_json::from_reader(std::io::BufReader::new(file))
        .map_err(|error| {
            anyhow::anyhow!(
                "Index manifest '{}' could not be parsed: {}",
                manifest_path.display(),
                error
            )
        })?;
    Ok(parsed.indexed_at)
}
```

Aynı dosyanın test modülündeki iki `IndexManifest { ... }` literaline (~2004 ve ~2017) `indexed_at: None,` ekle.

- [ ] **Step 4: Stat önbellekli fingerprint**

`fingerprint_for_path` fonksiyonunun hemen altına ekle:

```rust
/// Önceki fingerprint'in stat bilgisi (mtime + boyut) değişmemişse ve dosya
/// racy pencerenin dışındaysa içerik hash'ini dosyayı okumadan yeniden
/// kullanır. `reuse_before_sec` 0 ise (zaman damgasız eski manifest) her dosya
/// yeniden hash'lenir.
fn fingerprint_reusing_previous(
    path: &Path,
    previous: Option<&FileFingerprint>,
    reuse_before_sec: u64,
) -> std::io::Result<FileFingerprint> {
    let Some(previous) = previous else {
        return fingerprint_for_path(path);
    };
    let meta = std::fs::metadata(path)?;
    let modified = meta
        .modified()
        .ok()
        .and_then(|value| value.duration_since(UNIX_EPOCH).ok());
    let modified_sec = modified.as_ref().map(|value| value.as_secs()).unwrap_or(0);
    let modified_nsec = modified
        .as_ref()
        .map(|value| value.subsec_nanos())
        .unwrap_or(0);
    let stat_unchanged = previous.modified_sec == modified_sec
        && previous.modified_nsec == modified_nsec
        && previous.size == meta.len();
    if stat_unchanged && modified_sec < reuse_before_sec {
        return Ok(previous.clone());
    }
    fingerprint_for_path(path)
}
```

- [ ] **Step 5: `build_manifest` önceki manifesti kullansın**

İmzayı ve başlangıcını değiştir:

```rust
fn build_manifest(
    project_root: &Path,
    excluded_paths: &[PathBuf],
    previous: &IndexManifest,
) -> Result<IndexManifest> {
    // Zaman damgası tarama başlamadan alınır; tarama sırasında değişen
    // dosyalar bir sonraki koşuda racy pencereye düşer ve yeniden hash'lenir.
    let indexed_at = unix_now_secs();
    let reuse_before_sec = previous
        .indexed_at
        .map(|value| value.saturating_sub(RACY_WINDOW_SECS))
        .unwrap_or(0);
    let mut manifest = IndexManifest {
        schema_version: INDEX_SCHEMA_VERSION,
        indexed_commit: current_head_oid(project_root),
        indexed_at: Some(indexed_at),
        files: HashMap::new(),
    };
```

Döngü içindeki `fingerprint_for_path(file_path)` çağrısını değiştir:

```rust
        let fingerprint = fingerprint_reusing_previous(
            file_path,
            previous.files.get(&file_id),
            reuse_before_sec,
        )
        .map_err(|error| {
            anyhow::anyhow!(
                "Project snapshot could not read '{}': {}. Existing index was preserved.",
                file_path.display(),
                error
            )
        })?;
```

- [ ] **Step 6: Artefakt listesini tek kaynağa taşı ve `update_index`'i güncelle**

`copy_directory` fonksiyonunun üstüne ekle:

```rust
/// İndeksin kendi yazdığı artefaktlar. Manifest taraması ve dosya izleyici
/// bunları proje dosyası saymaz; aksi halde her yenileme kendini tetikler.
fn index_artifact_paths(artifact_parent: &Path, requested_db_path: &Path) -> Vec<PathBuf> {
    vec![
        requested_db_path.to_path_buf(),
        artifact_parent.join("ccm_graph.json"),
        artifact_parent.join("ccm_manifest.json"),
        artifact_parent.join(CURRENT_GENERATION_FILE),
        artifact_parent.join(GENERATIONS_DIRECTORY),
        artifact_parent.join(ACTIVATION_LOCK_DIRECTORY),
    ]
}
```

`update_index` içindeki `build_manifest(...)` çağrısını değiştir:

```rust
    let new_manifest = build_manifest(
        &project_root,
        &index_artifact_paths(artifact_parent, &requested_db_path),
        &manifest,
    )?;
```

- [ ] **Step 7: Staging'de grafı yeniden ayrıştırma**

`update_index` içindeki staging bloğunda şu satırları:

```rust
        copy_directory(&active.db_path, &staged_db_path)?;
        std::fs::copy(&active.graph_path, &staged_graph_path)?;
        std::fs::copy(&active.manifest_path, &staged_manifest_path)?;

        let staged_graph = CodeGraph::from_file(&staged_graph_path.to_string_lossy())?;
```

şununla değiştir:

```rust
        copy_directory(&active.db_path, &staged_db_path)?;
        // Graf bu fonksiyonun başında aktif generation'dan zaten yüklendi; JSON'u
        // kopyalayıp yeniden ayrıştırmak büyük repolarda ~0,25 sn sürer. Graf ve
        // manifest aşağıda staging'e yeniden yazılır.
        let staged_graph = graph;
```

- [ ] **Step 8: Tam indeks zaman damgasını yazsın**

`build_index_generation` içinde `info!(path = path, db_path = %db_path_str, "Starting directory indexing");` satırının hemen altına:

```rust
    // Manifestin racy penceresi taramadan önceki ana göre hesaplanır.
    let snapshot_started_at = unix_now_secs();
```

Fonksiyon sonunda `manifest.indexed_commit = current_head_oid(&project_root);` satırının altına:

```rust
    manifest.indexed_at = Some(snapshot_started_at);
```

- [ ] **Step 9: Testleri çalıştır**

Run: `cargo test -p ccm-core --test incremental_filesystem_test`
Expected: tüm testler PASS (iki yeni test dahil).

- [ ] **Step 10: Crate kapıları**

Run: `cargo fmt --all -- --check && cargo clippy -p ccm-core --all-targets -- -D warnings && cargo test -p ccm-core`
Expected: temiz çıktı, tüm testler PASS.

- [ ] **Step 11: Commit**

```bash
git add core/src/lib.rs core/tests/incremental_filesystem_test.rs
git commit -m "$(cat <<'EOF'
perf(index): reuse unchanged file hashes and the loaded graph in incremental updates

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 2: Metni değişmeyen parçaların vektörlerini yeniden kullanma

**Files:**
- Modify: `core/src/vector/store.rs` — `add_documents` (~279-446), yeni `embed_in_batches`, yeni `vectors_for_file` (`delete_by_prefix` ~552 yanına), yeni `ChunkEmbeddingCounts`
- Modify: `core/src/engine.rs` — `index_graph` (~152-179), `incremental_index_paths` (~191-389), `index_nodes_in_bounded_batches` (~391-445), `add_documents` çağıran test (~1804)
- Modify: `core/src/lib.rs` — `IndexStats` (~1406), `build_index_generation` içindeki `index_graph` eşlemesi (~556)
- Modify: `mcp/src/tools.rs` — `format_index_stats_result` (~963)
- Modify: `cli/src/main.rs` — "Initial indexing complete" logu (~169)
- Test: `core/tests/incremental_filesystem_test.rs`

**Interfaces:**
- Produces:
  - `pub struct ChunkEmbeddingCounts { pub embedded: usize, pub reused: usize }` (`ccm_core::vector::store`)
  - `LanceDbStore::vectors_for_file(&self, file_id: &str) -> Result<HashMap<String, Vec<f32>>>`
  - `LanceDbStore::add_documents(&self, ids: Vec<String>, texts: Vec<String>, known_vectors: &HashMap<String, Vec<f32>>) -> Result<ChunkEmbeddingCounts>`
  - `RetrievalEngine::index_graph(&self) -> Result<ChunkEmbeddingCounts>`
  - `IndexStats.embedded_chunks: usize`, `IndexStats.reused_chunks: usize`

- [ ] **Step 1: Başarısız testi yaz**

`core/tests/incremental_filesystem_test.rs` sonuna ekle:

```rust
/// Ollama `/api/embed` sözleşmesini konuşan deterministik yerel sunucu. CI'da
/// gerçek embedding servisi olmadığı için yalnızca bu testte kullanılır ve
/// istek başına gelen `input` sayısını toplar.
fn start_counting_embed_server(
) -> Result<(String, std::sync::Arc<std::sync::atomic::AtomicUsize>)> {
    use std::io::{BufRead, BufReader, Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let address = format!("http://{}", listener.local_addr()?);
    let embedded_inputs = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = embedded_inputs.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let Ok(read_half) = stream.try_clone() else { continue };
            let mut reader = BufReader::new(read_half);
            let mut content_length = 0usize;
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    content_length = value.trim().parse().unwrap_or(0);
                }
            }
            let mut body = vec![0u8; content_length];
            if reader.read_exact(&mut body).is_err() {
                continue;
            }
            let request: serde_json::Value = serde_json::from_slice(&body).unwrap_or_default();
            let inputs = request["input"].as_array().cloned().unwrap_or_default();
            counter.fetch_add(inputs.len(), std::sync::atomic::Ordering::SeqCst);
            let embeddings: Vec<Vec<f32>> = inputs
                .iter()
                .map(|input| {
                    let seed = input.as_str().unwrap_or_default().bytes().fold(0u32, |acc, byte| {
                        acc.wrapping_mul(31).wrapping_add(u32::from(byte))
                    });
                    (0..8u32)
                        .map(|offset| (seed.wrapping_add(offset) % 97) as f32 / 97.0 + 0.01)
                        .collect()
                })
                .collect();
            let payload = serde_json::json!({ "embeddings": embeddings }).to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                payload.len(),
                payload
            );
            let _ = stream.write_all(response.as_bytes());
        }
    });
    Ok((address, embedded_inputs))
}

#[tokio::test]
async fn update_index_embeds_only_changed_chunks() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    struct EnvRestore;
    impl Drop for EnvRestore {
        fn drop(&mut self) {
            std::env::remove_var("EMBEDDING_HOST");
            std::env::remove_var("EMBEDDING_MODEL");
            std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
        }
    }
    let _restore = EnvRestore;
    let (host, embedded_inputs) = start_counting_embed_server()?;
    std::env::remove_var("CCM_DISABLE_EMBEDDER");
    std::env::set_var("EMBEDDING_HOST", &host);
    std::env::set_var("EMBEDDING_MODEL", "ccm-test-embed");

    let project = tempdir()?;
    let file = project.path().join("lib.rs");
    std::fs::write(
        &file,
        "fn alpha() { let a = 1; }\nfn beta() { let b = 2; }\nfn gamma() { let c = 3; }\n",
    )?;
    let first = ccm_core::update_index(project.path().to_string_lossy().as_ref(), None).await?;
    assert_eq!(first.embedded_chunks, 3);
    let after_full_index = embedded_inputs.load(std::sync::atomic::Ordering::SeqCst);

    std::fs::write(
        &file,
        "fn alpha() { let a = 1; }\nfn beta() { let b = 2; }\nfn gamma() { let c = 30; }\n",
    )?;
    let second = ccm_core::update_index(project.path().to_string_lossy().as_ref(), None).await?;

    assert_eq!(second.embedded_chunks, 1, "only gamma changed");
    assert_eq!(second.reused_chunks, 2, "alpha and beta keep their vectors");
    assert_eq!(
        embedded_inputs.load(std::sync::atomic::Ordering::SeqCst) - after_full_index,
        1,
        "the embedding service must see only the changed chunk"
    );
    Ok(())
}
```

- [ ] **Step 2: Testin başarısız olduğunu doğrula**

Run: `cargo test -p ccm-core --test incremental_filesystem_test update_index_embeds_only_changed_chunks`
Expected: derleme hatası `no field 'embedded_chunks' on type 'IndexStats'`.

- [ ] **Step 3: `IndexStats` sayaçları**

`core/src/lib.rs` içinde `IndexStats`'a `semantic_unavailable` alanının altına ekle:

```rust
    /// Bu koşuda embedding servisine gönderilen parça sayısı.
    #[serde(default)]
    pub embedded_chunks: usize,
    /// Metni değişmediği için mevcut vektörü yeniden kullanılan parça sayısı.
    #[serde(default)]
    pub reused_chunks: usize,
```

- [ ] **Step 4: Store — sayaç tipi, vektör okuma ve batch embedding**

`core/src/vector/store.rs` içinde `LanceDbStore` tanımının üstüne ekle:

```rust
/// Bir yazma turunda yeni embed edilen ve mevcut vektörü yeniden kullanılan
/// parça sayıları.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ChunkEmbeddingCounts {
    pub embedded: usize,
    pub reused: usize,
}
```

`impl LanceDbStore` içinde `delete_by_prefix`'in hemen üstüne ekle:

```rust
    /// Dosyanın mevcut parçalarını metinden vektöre eşler. Artımlı güncelleme
    /// dosyanın satırlarını silmeden önce bunu okur; metni değişmeyen parçalar
    /// yeniden embed edilmez. Tablo hiç oluşmamışsa (graf-only) boş döner.
    pub async fn vectors_for_file(&self, file_id: &str) -> Result<HashMap<String, Vec<f32>>> {
        let table = match self.table().await {
            Ok(table) => table,
            // `delete_by_prefix` ile aynı sözleşme: tablo yoksa okunacak vektör de yoktur.
            Err(_) => return Ok(HashMap::new()),
        };
        let batches: Vec<RecordBatch> = table
            .query()
            .only_if(file_scoped_delete_predicate(file_id))
            .select(lancedb::query::Select::columns(&["text", "vector"]))
            .execute()
            .await
            .with_context(|| format!("Existing vectors for '{}' could not be queried", file_id))?
            .try_collect()
            .await?;

        let mut vectors = HashMap::new();
        for batch in batches {
            let text_col = batch
                .column_by_name("text")
                .ok_or_else(|| anyhow::anyhow!("Missing 'text' column in stored vectors"))?
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| anyhow::anyhow!("Failed to cast 'text' column to StringArray"))?;
            let vector_col = batch
                .column_by_name("vector")
                .ok_or_else(|| anyhow::anyhow!("Missing 'vector' column in stored vectors"))?
                .as_any()
                .downcast_ref::<FixedSizeListArray>()
                .ok_or_else(|| {
                    anyhow::anyhow!("Failed to cast 'vector' column to FixedSizeListArray")
                })?;
            for row in 0..batch.num_rows() {
                let values = vector_col.value(row);
                let floats = values
                    .as_any()
                    .downcast_ref::<Float32Array>()
                    .ok_or_else(|| anyhow::anyhow!("Stored vector items are not Float32"))?;
                vectors.insert(text_col.value(row).to_string(), floats.values().to_vec());
            }
        }
        Ok(vectors)
    }
```

`add_documents` içindeki embedder döngüsünü ayrı fonksiyona taşı. `impl LanceDbStore` içine ekle:

```rust
    /// Metinleri sınırlı eşzamanlılıkla batch'ler hâlinde embed eder ve girişle
    /// aynı sırada vektör döndürür. Boş girişte servis hiç çağrılmaz.
    async fn embed_in_batches(
        &self,
        texts: Vec<String>,
        batch_size: usize,
    ) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let embedder = self.embedder()?;
        let total_batches = texts.len().div_ceil(batch_size);
        // Sınırlı eşzamanlılık: Ollama varsayılan olarak tek model işçisiyle
        // (`num_parallel=1`) istekleri sıraya alır; eşzamanlı istekler seri
        // işlenir, hızlanma sağlamaz ve yavaş makinelerde 30s timeout'u
        // aşıp tüm full index'i abort edebilir. Bu yüzden varsayılan 1'dir;
        // `OLLAMA_NUM_PARALLEL>1` ortamlarında CCM_EMBED_CONCURRENCY
        // yükseltilerek gerçek paralellik alınabilir.
        let concurrency: usize = std::env::var("CCM_EMBED_CONCURRENCY")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(1)
            .clamp(1, 8);
        let mut collected: Vec<Option<Vec<Vec<f32>>>> = vec![None; total_batches];
        let batch_futures = texts
            .chunks(batch_size)
            .enumerate()
            .map(|(batch_idx, batch)| {
                let batch_texts: Vec<String> = batch.to_vec();
                let embedder = Arc::clone(&embedder);
                async move {
                    let result = embedder.embed(batch_texts).await;
                    (batch_idx, result)
                }
            });
        let mut stream = futures::stream::iter(batch_futures).buffer_unordered(concurrency);
        let mut completed = 0usize;
        while let Some((batch_idx, result)) = stream.next().await {
            let batch_embeddings = result?;
            completed += 1;
            if batch_idx % 20 == 0 || completed == total_batches {
                tracing::info!(
                    batch = batch_idx + 1,
                    total = total_batches,
                    chunks = batch_embeddings.len(),
                    "Embedding batch progress"
                );
            }
            collected[batch_idx] = Some(batch_embeddings);
        }
        let mut embeddings = Vec::with_capacity(texts.len());
        for batch in collected {
            embeddings.extend(batch.expect("embedding batch"));
        }
        Ok(embeddings)
    }
```

- [ ] **Step 5: `add_documents` bilinen vektörleri kullansın**

İmzayı ve erken dönüşü değiştir:

```rust
    pub async fn add_documents(
        &self,
        ids: Vec<String>,
        texts: Vec<String>,
        known_vectors: &HashMap<String, Vec<f32>>,
    ) -> Result<ChunkEmbeddingCounts> {
        if ids.is_empty() {
            return Ok(ChunkEmbeddingCounts::default());
        }
```

`let mut embeddings: Vec<Vec<f32>> = Vec::with_capacity(all_chunks.len());` satırından `// Use all_chunks and all_chunk_ids for storage` yorumuna kadar olan bloğu (fixture dalı + embedder döngüsü) şununla değiştir:

```rust
        let mut embeddings: Vec<Vec<f32>> = Vec::with_capacity(all_chunks.len());
        let counts = if let Some(fixture) = self.fixture.as_ref() {
            for chunk_id in &all_chunk_ids {
                embeddings.push(fixture.doc_vector(&self.fixture_ns, chunk_id)?);
            }
            ChunkEmbeddingCounts {
                embedded: all_chunk_ids.len(),
                reused: 0,
            }
        } else {
            if self.embedder_disabled {
                return Ok(ChunkEmbeddingCounts::default());
            }
            // Metni değişmemiş parçalar mevcut vektörünü korur; embedding metnin
            // saf fonksiyonu olduğundan sonuç aynıdır ve servis çağrısı atlanır.
            let missing_texts: Vec<String> = all_chunks
                .iter()
                .filter(|chunk| !known_vectors.contains_key(chunk.as_str()))
                .cloned()
                .collect();
            let counts = ChunkEmbeddingCounts {
                embedded: missing_texts.len(),
                reused: all_chunks.len() - missing_texts.len(),
            };
            let mut fresh_vectors = self
                .embed_in_batches(missing_texts, batch_size)
                .await?
                .into_iter();
            for chunk in &all_chunks {
                let vector = match known_vectors.get(chunk.as_str()) {
                    Some(known) => known.clone(),
                    None => fresh_vectors.next().ok_or_else(|| {
                        anyhow::anyhow!(
                            "Embedding provider returned fewer vectors than requested chunks"
                        )
                    })?,
                };
                embeddings.push(vector);
            }
            counts
        };
```

Fonksiyonun sonundaki `Ok(())` satırını `Ok(counts)` yap.

- [ ] **Step 6: Engine — sayaçları taşı, eski vektörleri silmeden önce oku**

`core/src/engine.rs` import'larına ekle: `use crate::vector::store::ChunkEmbeddingCounts;`

`index_nodes_in_bounded_batches` imzası ve erken dönüşü:

```rust
    async fn index_nodes_in_bounded_batches(
        &self,
        nodes: &[CodeNode],
        known_vectors: &HashMap<String, Vec<f32>>,
    ) -> Result<ChunkEmbeddingCounts> {
        if nodes.is_empty() {
            return Ok(ChunkEmbeddingCounts::default());
        }
```

Döngüden önce `let mut counts = ChunkEmbeddingCounts::default();` ekle. `self.vector_store.add_documents(ids, texts).await?;` satırını değiştir:

```rust
            let batch_counts = self
                .vector_store
                .add_documents(ids, texts, known_vectors)
                .await?;
            counts.embedded += batch_counts.embedded;
            counts.reused += batch_counts.reused;
```

Fonksiyon sonundaki `Ok(())` satırını `Ok(counts)` yap.

`index_graph` imzası `pub async fn index_graph(&self) -> Result<ChunkEmbeddingCounts>` olur ve son satırı:

```rust
        self.index_nodes_in_bounded_batches(&nodes, &HashMap::new())
            .await
```

`incremental_index_paths` içinde `let mut indexed_node_ids = HashSet::new();` satırının altına ekle:

```rust
        // Silinmeden önce okunan parça vektörleri (metin → vektör).
        let mut known_vectors: HashMap<String, Vec<f32>> = HashMap::new();
```

Var olan dosya için yapılan `self.vector_store.delete_by_prefix(&relative_path)` çağrısının ("Failed to replace vectors" hata mesajlı olan) hemen üstüne ekle:

```rust
            // Metni değişmeyen parçalar yeniden embed edilmesin diye mevcut
            // vektörler silmeden önce alınır.
            let existing_vectors = self
                .vector_store
                .vectors_for_file(&relative_path)
                .await
                .map_err(|error| {
                    anyhow::anyhow!(
                        "Failed to read existing vectors for '{}': {}",
                        relative_path,
                        error
                    )
                })?;
            known_vectors.extend(existing_vectors);
```

Fonksiyon sonundaki embedding bloğunu değiştir:

```rust
        if !nodes_to_index.is_empty() {
            tracing::info!(
                count = nodes_to_index.len(),
                "Incremental: indexing semantic nodes"
            );
            let counts = self
                .index_nodes_in_bounded_batches(&nodes_to_index, &known_vectors)
                .await?;
            stats.embedded_chunks = counts.embedded;
            stats.reused_chunks = counts.reused;
        }
```

Aynı dosyadaki testte (`.add_documents(` ~1804) üçüncü argüman olarak `&std::collections::HashMap::new(),` ekle.

- [ ] **Step 7: Tam indeks sayaçları, MCP ve CLI çıktısı**

`core/src/lib.rs` `build_index_generation` içindeki `Ok(()) => info!(...)` kolunu değiştir:

```rust
                match engine.index_graph().await {
                    Ok(counts) => {
                        stats.embedded_chunks = counts.embedded;
                        stats.reused_chunks = counts.reused;
                        info!(
                            nodes = stats.nodes_created,
                            files = stats.files_indexed,
                            embedded_chunks = counts.embedded,
                            "Indexing completed successfully"
                        )
                    }
```

`mcp/src/tools.rs` `format_index_stats_result` içinde `lines` vektörünün tanımından hemen sonra ekle:

```rust
    if stats.embedded_chunks + stats.reused_chunks > 0 {
        lines.push(format!(
            "- Chunks Embedded: {} (reused: {})",
            stats.embedded_chunks, stats.reused_chunks
        ));
    }
```

`cli/src/main.rs` "Initial indexing complete" logunu değiştir:

```rust
                    tracing::info!(
                        indexed = stats.files_indexed,
                        failed = stats.files_failed,
                        skipped = stats.files_skipped,
                        nodes = stats.nodes_created,
                        embedded_chunks = stats.embedded_chunks,
                        reused_chunks = stats.reused_chunks,
                        "Initial indexing complete"
                    );
```

- [ ] **Step 8: Testleri çalıştır**

Run: `cargo test -p ccm-core --test incremental_filesystem_test && cargo test -p ccm-core --lib`
Expected: tüm testler PASS.

- [ ] **Step 9: Kapılar**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: temiz çıktı, tüm testler PASS.

- [ ] **Step 10: Commit**

```bash
git add core/src/vector/store.rs core/src/engine.rs core/src/lib.rs mcp/src/tools.rs cli/src/main.rs core/tests/incremental_filesystem_test.rs
git commit -m "$(cat <<'EOF'
perf(index): re-embed only chunks whose text changed during incremental updates

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 3: Watcher filtresi

**Files:**
- Create: `core/src/watch_filter.rs`
- Modify: `core/src/lib.rs` — modül bildirimi ve re-export (dosyanın başı)
- Test: `core/tests/incremental_filesystem_test.rs`

**Interfaces:**
- Consumes: `index_artifact_paths` (Task 1), `is_index_relevant_file`, `GENERATIONS_DIRECTORY`, `ACTIVATION_LOCK_DIRECTORY` (mevcut).
- Produces: `pub struct WatchFilter`, `pub fn build_watch_filter(project_root: &Path, db_path: &Path) -> anyhow::Result<WatchFilter>`, `pub fn is_watch_relevant_path(filter: &WatchFilter, path: &Path) -> bool` (hepsi `ccm_core::` altından).

- [ ] **Step 1: Başarısız testi yaz**

`core/tests/incremental_filesystem_test.rs` sonuna ekle:

```rust
#[tokio::test]
async fn watch_filter_skips_ignored_outputs_and_index_artifacts() -> Result<()> {
    let _env_guard = ENV_LOCK.lock().await;
    std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    let project = tempdir()?;
    let root = std::fs::canonicalize(project.path())?;
    std::fs::create_dir_all(root.join("src"))?;
    std::fs::write(root.join("src/lib.rs"), "fn alpha() {}\n")?;
    std::fs::write(root.join(".gitignore"), "generated/\n")?;
    std::fs::write(root.join(".ccmignore"), "fixtures/\n")?;
    ccm_core::index_directory(root.to_string_lossy().as_ref(), None).await?;
    let active = artifacts(&root, None)?;
    let filter = ccm_core::build_watch_filter(&root, &root.join("data/ccm_db"))?;

    for relevant in ["src/lib.rs", "src/removed.rs", "src/my file.rs", "Makefile"] {
        assert!(
            ccm_core::is_watch_relevant_path(&filter, &root.join(relevant)),
            "{relevant} should trigger a refresh"
        );
    }
    let ignored = [
        root.join("generated/out.rs"),
        root.join("fixtures/sample.rs"),
        root.join("target/debug/build.rs"),
        root.join(".git/index"),
        root.join(".ccm/semantic-upgrade.log"),
        root.join("data/ccm_current"),
        active.graph_path.clone(),
        active.db_path.join("code_vectors.lance/data.lance"),
        root.clone(),
        std::path::PathBuf::from("/outside/project.rs"),
    ];
    for path in ignored {
        assert!(
            !ccm_core::is_watch_relevant_path(&filter, &path),
            "{} should not trigger a refresh",
            path.display()
        );
    }
    Ok(())
}
```

- [ ] **Step 2: Testin başarısız olduğunu doğrula**

Run: `cargo test -p ccm-core --test incremental_filesystem_test watch_filter_`
Expected: derleme hatası `cannot find function 'build_watch_filter' in crate 'ccm_core'`.

- [ ] **Step 3: Filtreyi yaz**

`core/src/watch_filter.rs`:

```rust
//! MCP otomatik yenilemesinin dosya olaylarını süzen filtre. Manifest
//! taramasıyla aynı politikayı uygular; indeksin kendi yazdığı dosyalar ve
//! ignore kurallarına takılan build çıktıları yenileme tetiklemez.

use anyhow::Result;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::path::{Path, PathBuf};

/// Bir proje için önceden derlenmiş izleme kuralları.
pub struct WatchFilter {
    root: PathBuf,
    excluded: Vec<PathBuf>,
    ignore: Gitignore,
}

/// Proje kökü ve indeks DB yolundan izleme filtresini kurar. Kök seviyesindeki
/// `.gitignore`, `.ignore`, `.ccmignore` ve `.git/info/exclude` okunur; iç içe
/// ignore dosyaları kapsanmaz (kaçan olay yalnızca değişiklik bulmayan bir
/// yenileme maliyeti yaratır).
pub fn build_watch_filter(project_root: &Path, db_path: &Path) -> Result<WatchFilter> {
    let root = std::fs::canonicalize(project_root).map_err(|error| {
        anyhow::anyhow!(
            "Project root '{}' could not be resolved for watching: {}",
            project_root.display(),
            error
        )
    })?;
    let artifact_parent = db_path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "Invalid DB path '{}': cannot determine parent directory",
            db_path.display()
        )
    })?;
    let excluded = crate::index_artifact_paths(artifact_parent, db_path)
        .into_iter()
        .flat_map(|path| match std::fs::canonicalize(&path) {
            Ok(canonical) if canonical != path => vec![path, canonical],
            _ => vec![path],
        })
        .collect();

    let mut builder = GitignoreBuilder::new(&root);
    for candidate in [
        root.join(".gitignore"),
        root.join(".ignore"),
        root.join(".ccmignore"),
        root.join(".git/info/exclude"),
    ] {
        if !candidate.is_file() {
            continue;
        }
        if let Some(error) = builder.add(&candidate) {
            return Err(anyhow::anyhow!(
                "Ignore file '{}' could not be parsed for watching: {}",
                candidate.display(),
                error
            ));
        }
    }
    let ignore = builder.build().map_err(|error| {
        anyhow::anyhow!(
            "Watch ignore rules could not be built for '{}': {}",
            root.display(),
            error
        )
    })?;
    Ok(WatchFilter {
        root,
        excluded,
        ignore,
    })
}

/// Olay yolunun indeksi değiştirebilecek bir proje dosyası olup olmadığını
/// bildirir. Silinmiş yollar için de çalışır (dosya içeriği okunmaz; yalnızca
/// dizin ayrımı için `is_dir` sorulur).
pub fn is_watch_relevant_path(filter: &WatchFilter, path: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(&filter.root) else {
        return false;
    };
    if relative.as_os_str().is_empty() {
        return false;
    }
    if filter
        .excluded
        .iter()
        .any(|excluded| path.starts_with(excluded))
    {
        return false;
    }
    let tool_state = relative.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        name == ".ccm"
            || name == ".agent"
            || name == crate::GENERATIONS_DIRECTORY
            || name == crate::ACTIVATION_LOCK_DIRECTORY
            || name.starts_with(".ccm-rebuild-")
            || name.starts_with(".ccm-backup-")
    });
    if tool_state || !crate::is_index_relevant_file(&filter.root, path) {
        return false;
    }
    !filter
        .ignore
        .matched_path_or_any_parents(relative, path.is_dir())
        .is_ignore()
}
```

`core/src/lib.rs` başındaki modül listesine `mod watch_filter;` ekle ve `use` bloğunun altına:

```rust
pub use watch_filter::{build_watch_filter, is_watch_relevant_path, WatchFilter};
```

- [ ] **Step 4: Testi çalıştır**

Run: `cargo test -p ccm-core --test incremental_filesystem_test watch_filter_`
Expected: PASS.

- [ ] **Step 5: Kapılar**

Run: `cargo fmt --all -- --check && cargo clippy -p ccm-core --all-targets -- -D warnings && cargo test -p ccm-core`
Expected: temiz çıktı, tüm testler PASS.

- [ ] **Step 6: Commit**

```bash
git add core/src/watch_filter.rs core/src/lib.rs core/tests/incremental_filesystem_test.rs
git commit -m "$(cat <<'EOF'
feat(index): add a watch filter that skips ignored outputs and index artifacts

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 4: Yeniden indeksleme sürerken aktif generation'dan okumaya devam et

**Files:**
- Modify: `mcp/src/server.rs` — `get_engine` (~335-339 ve ~363-371)
- Test: `mcp/tests/mcp_integration_test.rs`

**Interfaces:**
- Consumes: yok. Produces: yok (davranış değişikliği). "Project indexing is in progress" hatası yalnızca henüz hiç generation yokken döner; mevcut `mcp_large_index_returns_before_client_timeout_and_supports_polling` testi bu yüzden değişmeden geçer.

- [ ] **Step 1: Başarısız testi yaz**

`mcp/tests/mcp_integration_test.rs` sonuna ekle:

```rust
#[test]
fn mcp_serves_the_active_generation_while_reindexing() -> Result<(), Box<dyn std::error::Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn stable_symbol() {}\n")?;
    let mut cmd = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
    cmd.env("CCM_DISABLE_EMBEDDER", "1")
        .env("CCM_MCP_DEBUG", "0")
        .env("CCM_AUTO_REFRESH", "0")
        .env("CCM_INDEX_RESPONSE_TIMEOUT_MS", "1")
        .env("CCM_INTERNAL_INDEX_TEST_DELAY_MS", "3000")
        .env("CCM_PROJECT_ROOT", project.path())
        .env("CCM_ALLOWED_ROOTS", project.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn()?;
    let mut stdin = child.stdin.take().unwrap();
    let mut reader = BufReader::new(child.stdout.take().unwrap());
    send_request(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc":"2.0","id":0,"method":"initialize","params":{}}),
    )?;
    let indexed = send_request(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"index_now","arguments":{"project_path": project.path()}}}),
    )?;
    assert!(tool_text(&indexed).contains("Project index refreshed successfully"));

    fs::write(project.path().join("extra.rs"), "fn added_later() {}\n")?;
    let started = send_request(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"index_project","arguments":{"project_path": project.path()}}}),
    )?;
    assert!(tool_text(&started).contains("started in the background"));

    let retrieval = send_request(
        &mut stdin,
        &mut reader,
        json!({"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"find_nodes","arguments":{"query":"stable_symbol"}}}),
    )?;
    assert!(
        retrieval.get("error").is_none(),
        "reads must keep serving the active generation: {retrieval}"
    );
    assert!(tool_text(&retrieval).contains("stable_symbol (Score:"));

    let _ = child.kill();
    Ok(())
}
```

- [ ] **Step 2: Testin başarısız olduğunu doğrula**

Run: `cargo test -p ccm-mcp --test mcp_integration_test mcp_serves_the_active_generation_while_reindexing`
Expected: FAIL — `reads must keep serving the active generation: {... "indexing is in progress" ...}`.

- [ ] **Step 3: Engeli "indeks yok" dalına taşı**

`get_engine` içinde şu bloğu sil:

```rust
        if self.index_job_in_progress(&cache_key) {
            return Err(anyhow::anyhow!(
                "Project indexing is in progress. Retry this tool after index_project reports completion."
            ));
        }
```

"Project index is missing" dönen bloğu şununla değiştir:

```rust
        if !Path::new(&db_path).exists()
            || !Path::new(&graph_path).is_file()
            || !Path::new(&manifest_path).is_file()
        {
            // İlk indeksleme sürerken okunacak generation yoktur; iş bitince aynı
            // çağrı çalışır. Var olan generation ise yeniden indeksleme sırasında
            // okunmaya devam eder (generation geçişi atomiktir).
            if self.index_job_in_progress(&cache_key) {
                return Err(anyhow::anyhow!(
                    "Project indexing is in progress. Retry this tool after index_project reports completion."
                ));
            }
            return Err(anyhow::anyhow!(
                "Project index is missing. Call index_project first; large indexes run in the background."
            ));
        }
```

- [ ] **Step 4: Testleri çalıştır**

Run: `cargo test -p ccm-mcp --test mcp_integration_test`
Expected: tüm testler PASS (yeni test ve `mcp_large_index_returns_before_client_timeout_and_supports_polling` dahil).

- [ ] **Step 5: Kapılar**

Run: `cargo fmt --all -- --check && cargo clippy -p ccm-mcp --all-targets -- -D warnings && cargo test -p ccm-mcp`
Expected: temiz çıktı, tüm testler PASS.

- [ ] **Step 6: Commit**

```bash
git add mcp/src/server.rs mcp/tests/mcp_integration_test.rs
git commit -m "$(cat <<'EOF'
fix(mcp): keep serving the active generation while a re-index runs

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 5: Okuma sonuçlarında tazelik satırı, indeks yaşı ve generation başına tek engine

**Files:**
- Create: `mcp/src/freshness.rs`
- Modify: `mcp/src/main.rs` — `mod freshness;`
- Modify: `mcp/src/server.rs` — `EngineCache` (~66-90), `get_engine` (~304-400), `refresh_project_engine` (~402-427), `handle_call_tool_inner` (~932-1015), yeni `project_key_for_path`, `ServerState::project_key`, yeni `engine_error_response`
- Create: `mcp/tests/auto_refresh_test.rs`

**Interfaces:**
- Consumes: `ccm_core::read_index_timestamp`, `ccm_core::unix_now_secs` (Task 1).
- Produces:
  - `pub struct CachedEngine { pub engine: Arc<RetrievalEngine>, pub indexed_at: Option<u64> }`; `get_engine(...) -> Result<CachedEngine>`
  - `EngineCache::insert(&mut self, project_key: &str, key: String, engine: CachedEngine) -> CachedEngine` (aynı projenin eski generation'larını düşürür)
  - `pub(crate) fn project_key_for_path(path: &str) -> String`; `ServerState::project_key(&self, project_path: Option<&str>) -> Option<String>`
  - `fn engine_error_response(id: Option<Value>, tool_name: &str, error: &anyhow::Error) -> JsonRpcResponse`
  - `freshness::{WatcherStatus, ProjectFreshness, is_settled, disabled_freshness, format_age, format_freshness_line, with_freshness_line}`
  - Test yardımcıları `McpSession`, `found_node`, `poll_find_nodes` (Task 6 kullanır).

- [ ] **Step 1: Başarısız testi ve test yardımcılarını yaz**

`mcp/tests/auto_refresh_test.rs`:

```rust
use serde_json::{json, Value};
use std::error::Error;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::{Duration, Instant};
use tempfile::tempdir;

/// Gerçek `ccm-mcp` sürecini stdio üzerinden süren test bağlayıcısı.
struct McpSession {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    next_id: u64,
}

impl McpSession {
    fn start(project: &Path, extra_env: &[(&str, &str)]) -> Result<Self, Box<dyn Error>> {
        let mut command = Command::new(assert_cmd::cargo::cargo_bin!("ccm-mcp"));
        command
            .env("CCM_DISABLE_EMBEDDER", "1")
            .env("CCM_MCP_DEBUG", "0")
            .env("CCM_PROJECT_ROOT", project)
            .env("CCM_ALLOWED_ROOTS", project)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for (key, value) in extra_env {
            command.env(key, value);
        }
        let mut child = command.spawn()?;
        let stdin = child.stdin.take().ok_or("child stdin missing")?;
        let reader = BufReader::new(child.stdout.take().ok_or("child stdout missing")?);
        let mut session = Self {
            child,
            stdin,
            reader,
            next_id: 0,
        };
        session.request("initialize", json!({}))?;
        Ok(session)
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, Box<dyn Error>> {
        self.next_id += 1;
        let message = json!({"jsonrpc": "2.0", "id": self.next_id, "method": method, "params": params});
        writeln!(self.stdin, "{}", message)?;
        self.stdin.flush()?;
        let mut line = String::new();
        self.reader.read_line(&mut line)?;
        Ok(serde_json::from_str(&line)?)
    }

    fn call_tool(&mut self, name: &str, arguments: Value) -> Result<String, Box<dyn Error>> {
        let response = self.request("tools/call", json!({"name": name, "arguments": arguments}))?;
        if let Some(error) = response.get("error") {
            return Err(format!("{name} returned a JSON-RPC error: {error}").into());
        }
        Ok(response["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string())
    }
}

impl Drop for McpSession {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// `find_nodes` çıktısında sembolün sonuç başlığı olarak bulunup bulunmadığı.
/// "No graph nodes found for query: 'x'" mesajı da sembolü içerdiği için başlık
/// biçimi (`<sembol> (Score:`) aranır.
fn found_node(text: &str, symbol: &str) -> bool {
    text.contains(&format!("{symbol} (Score:"))
}

/// Koşul sağlanana kadar `find_nodes` çağırır; süre dolarsa son çıktıyla hata döner.
fn poll_find_nodes(
    session: &mut McpSession,
    query: &str,
    deadline: Duration,
    accept: impl Fn(&str) -> bool,
) -> Result<String, Box<dyn Error>> {
    let started = Instant::now();
    loop {
        let text = session.call_tool("find_nodes", json!({ "query": query }))?;
        if accept(&text) {
            return Ok(text);
        }
        if started.elapsed() > deadline {
            return Err(format!("condition not met within {deadline:?}; last output: {text}").into());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn freshness_line_reports_disabled_auto_refresh_and_index_age() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn tracked_symbol() {}\n")?;
    let mut session = McpSession::start(project.path(), &[("CCM_AUTO_REFRESH", "0")])?;
    session.call_tool("index_now", json!({ "project_path": project.path() }))?;

    let text = session.call_tool("find_nodes", json!({ "query": "tracked_symbol" }))?;

    assert!(
        text.starts_with("_Index: auto-refresh off · indexed "),
        "unexpected freshness line: {text}"
    );
    assert!(found_node(&text, "tracked_symbol"));
    Ok(())
}
```

- [ ] **Step 2: Testin başarısız olduğunu doğrula**

Run: `cargo test -p ccm-mcp --test auto_refresh_test`
Expected: FAIL — `unexpected freshness line: ## Function: tracked_symbol ...`.

- [ ] **Step 3: `freshness` modülünün saf kısmını yaz**

`mcp/src/freshness.rs`:

```rust
//! Proje indeksinin tazelik durumu ve okuma sonuçlarına eklenen tazelik satırı.

use crate::protocol::{ToolResult, ToolResultContent};

/// Otomatik yenileme için dosya izleyicinin durumu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WatcherStatus {
    /// Watcher çalışıyor; değişiklikler otomatik indekslenir.
    Active,
    /// `CCM_AUTO_REFRESH` ile kapatıldı.
    Disabled,
    /// Watcher açılamadı; sebep her sonuçta gösterilir.
    Unavailable(String),
}

/// Tek bir projenin anlık tazelik durumu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProjectFreshness {
    pub watcher: WatcherStatus,
    pub pending_paths: usize,
    pub refresh_in_flight: bool,
    /// Hızlı indeksin semantik yükseltmesi sürerken yenileme ertelenir;
    /// yükseltme bitince bekleyen değişiklikler tek seferde işlenir.
    pub waiting_for_upgrade: bool,
    pub last_error: Option<String>,
    pub semantic_unavailable: Option<String>,
}

/// Okumanın beklemesine gerek yoksa `true`: bekleyen iş yoktur ya da iş
/// semantik yükseltme bitene kadar ertelenmiştir (beklemek sonucu değiştirmez).
pub(crate) fn is_settled(freshness: &ProjectFreshness) -> bool {
    freshness.waiting_for_upgrade
        || (freshness.pending_paths == 0 && !freshness.refresh_in_flight)
}

/// Otomatik yenilemesi olmayan proje için durum.
pub(crate) fn disabled_freshness() -> ProjectFreshness {
    ProjectFreshness {
        watcher: WatcherStatus::Disabled,
        pending_paths: 0,
        refresh_in_flight: false,
        waiting_for_upgrade: false,
        last_error: None,
        semantic_unavailable: None,
    }
}

/// Saniye cinsinden süreyi kısa biçime çevirir (`42s`, `3m`, `2h`, `5d`).
pub(crate) fn format_age(seconds: u64) -> String {
    match seconds {
        0..=59 => format!("{}s", seconds),
        60..=3_599 => format!("{}m", seconds / 60),
        3_600..=86_399 => format!("{}h", seconds / 3_600),
        _ => format!("{}d", seconds / 86_400),
    }
}

/// Okuma sonuçlarının başına eklenen tek satırlık tazelik özetini üretir.
/// Taze ve izlenen bir indekste yaş gösterilmez; yaş yalnızca indeksin diskten
/// geri kalmış olabileceği durumlarda anlamlıdır.
pub(crate) fn format_freshness_line(
    freshness: &ProjectFreshness,
    indexed_at: Option<u64>,
    now_secs: u64,
) -> String {
    let fresh = freshness.watcher == WatcherStatus::Active
        && freshness.pending_paths == 0
        && !freshness.refresh_in_flight
        && freshness.last_error.is_none();
    let mut parts: Vec<String> = Vec::new();
    match &freshness.watcher {
        WatcherStatus::Active if fresh => {
            parts.push("fresh".to_string());
            parts.push("auto-refresh on".to_string());
        }
        WatcherStatus::Active => {
            parts.push("stale".to_string());
            if freshness.pending_paths > 0 {
                let plural = if freshness.pending_paths == 1 { "" } else { "s" };
                parts.push(format!(
                    "{} changed file{} pending",
                    freshness.pending_paths, plural
                ));
            }
            if freshness.refresh_in_flight {
                parts.push("refresh running".to_string());
            }
            if freshness.waiting_for_upgrade {
                parts.push("waiting for semantic upgrade".to_string());
            }
            if let Some(error) = &freshness.last_error {
                parts.push(format!("last refresh failed: {}", error));
            }
        }
        WatcherStatus::Disabled => parts.push("auto-refresh off".to_string()),
        WatcherStatus::Unavailable(reason) => {
            parts.push(format!("auto-refresh unavailable ({})", reason))
        }
    }
    if !fresh {
        if let Some(indexed_at) = indexed_at {
            parts.push(format!(
                "indexed {} ago",
                format_age(now_secs.saturating_sub(indexed_at))
            ));
        }
    }
    if let Some(reason) = &freshness.semantic_unavailable {
        parts.push(format!("semantic search unavailable: {}", reason));
    }
    format!("_Index: {}_", parts.join(" · "))
}

/// Tazelik satırını ilk metin içeriğinin başına ekler; içerik yoksa tek
/// satırlık metin içeriği oluşturur.
pub(crate) fn with_freshness_line(result: ToolResult, line: &str) -> ToolResult {
    let mut content = result.content;
    match content.first_mut() {
        Some(first) => first.text = format!("{}\n\n{}", line, first.text),
        None => content.push(ToolResultContent {
            content_type: "text".to_string(),
            text: line.to_string(),
        }),
    }
    ToolResult {
        content,
        is_error: result.is_error,
    }
}
```

`mcp/src/main.rs` modül listesine `mod freshness;` ekle.

- [ ] **Step 4: Engine önbelleği indeks zamanını taşısın ve eski generation'ları düşürsün**

`mcp/src/server.rs` içinde `EngineCache`'in üstüne `CachedEngine` ekle ve `EngineCache`'i değiştir:

```rust
/// Önbellekteki engine ve ait olduğu generation'ın indeksleme zamanı.
#[derive(Clone)]
pub struct CachedEngine {
    pub engine: Arc<RetrievalEngine>,
    /// Aktif generation manifestindeki indeksleme zamanı (unix saniye).
    pub indexed_at: Option<u64>,
}

pub struct EngineCache {
    cache: LruCache<String, CachedEngine>,
}

impl EngineCache {
    fn new(max: usize) -> Self {
        let cap = NonZeroUsize::new(max).unwrap_or_else(|| NonZeroUsize::new(1).unwrap());
        Self {
            cache: LruCache::new(cap),
        }
    }

    fn get(&mut self, key: &str) -> Option<CachedEngine> {
        self.cache.get(key).cloned()
    }

    #[allow(dead_code)]
    fn peek(&self, key: &str) -> Option<CachedEngine> {
        self.cache.peek(key).cloned()
    }

    /// Projenin yeni generation'ını ekler ve aynı projenin eski
    /// generation'larını düşürür. Otomatik yenileme her kayıtta yeni generation
    /// ürettiği için aksi halde eski graflar bellekte birikir; eski engine'i
    /// kullanan istekler `Arc` sayesinde bitene kadar onu korur.
    fn insert(&mut self, project_key: &str, key: String, engine: CachedEngine) -> CachedEngine {
        let prefix = format!("{}#", project_key);
        let stale: Vec<String> = self
            .cache
            .iter()
            .map(|(existing, _)| existing.clone())
            .filter(|existing| existing.starts_with(&prefix) && *existing != key)
            .collect();
        for existing in stale {
            self.cache.pop(&existing);
        }
        self.cache.put(key, engine.clone());
        engine
    }
}
```

`get_engine` dönüş tipini `Result<CachedEngine>` yap. `default_engine` dönüşünü değiştir:

```rust
                    return self
                        .default_engine
                        .read()
                        .await
                        .clone()
                        .map(|engine| CachedEngine {
                            engine,
                            indexed_at: None,
                        })
                        .ok_or_else(|| {
                            anyhow::anyhow!(
                                "No project root is available. Pass 'project_path' or set CCM_PROJECT_ROOT."
                            )
                        });
```

`get_engine` sonunda engine oluşturma ve ekleme kısmını değiştir:

```rust
        let indexed_at = ccm_core::read_index_timestamp(&artifacts.manifest_path)?;
        let engine = CachedEngine {
            engine: Arc::new(RetrievalEngine::new_with_active_policy(
                Arc::new(RwLock::new(graph)),
                store,
                policy_path.as_deref(),
            )),
            indexed_at,
        };

        let mut engines = self.engines.write().await;
        if let Some(existing) = engines.get(&engine_cache_key) {
            return Ok(existing);
        }
        Ok(engines.insert(&cache_key, engine_cache_key, engine))
```

`refresh_project_engine` fonksiyonunun tamamını şununla değiştir:

```rust
    pub async fn refresh_project_engine(&self, project_path: &str) -> Result<()> {
        // get_engine ile aynı normalize key kullanılır ki cache tutarlı kalsın.
        let canonical_path = canonicalize_project_path(Path::new(project_path));
        let cache_key = canonical_path.to_string_lossy().to_string();
        let artifacts = self.project_artifacts(&cache_key)?;
        let db_path = artifacts.db_path.to_string_lossy().to_string();
        let graph = CodeGraph::load_from_file(&artifacts.graph_path.to_string_lossy())?;
        let store = LanceDbStore::new(&db_path, "code_vectors").await?;
        let requested_db_path = self.project_db_path(&cache_key)?;
        let policy_path = requested_db_path
            .parent()
            .map(|parent| parent.join("ccm_learn/policies.json"));
        let engine = CachedEngine {
            engine: Arc::new(RetrievalEngine::new_with_active_policy(
                Arc::new(RwLock::new(graph)),
                store,
                policy_path.as_deref(),
            )),
            indexed_at: ccm_core::read_index_timestamp(&artifacts.manifest_path)?,
        };

        let engine_cache_key = format!(
            "{}#{}",
            cache_key,
            artifacts.generation_id.as_deref().unwrap_or("legacy")
        );
        self.engines
            .write()
            .await
            .insert(&cache_key, engine_cache_key, engine);
        Ok(())
    }
```

- [ ] **Step 5: Proje anahtarı**

`canonicalize_project_path` fonksiyonunun altına ekle:

```rust
/// Proje yolundan önbellek ve tazelik durumu için kanonik anahtar üretir.
pub(crate) fn project_key_for_path(path: &str) -> String {
    canonicalize_project_path(Path::new(path))
        .to_string_lossy()
        .to_string()
}
```

`impl ServerState` içine ekle:

```rust
    /// İstekteki `project_path` ya da etkin proje kökü için kanonik anahtar;
    /// ikisi de yoksa `None` (ev dizini deposu; tazelik satırı eklenmez).
    pub(crate) fn project_key(&self, project_path: Option<&str>) -> Option<String> {
        match project_path {
            Some(path) => Some(project_key_for_path(path)),
            None => self
                .effective_project_root()
                .map(|root| project_key_for_path(&root.to_string_lossy())),
        }
    }
```

- [ ] **Step 6: Okuma araçlarına satırı ekle**

`handle_call_tool_inner`'ın altına ekle (mevcut hata eşlemesi buraya taşınır):

```rust
/// Engine yüklenemediğinde istemciye dönen JSON-RPC hata yanıtını üretir.
fn engine_error_response(
    id: Option<Value>,
    tool_name: &str,
    error: &anyhow::Error,
) -> JsonRpcResponse {
    tracing::warn!(error = %error, tool = %tool_name, "Failed to load project context");
    let message = if error.to_string().contains("Project index is missing") {
        "Project index is missing. Call index_project first.".to_string()
    } else if error.to_string().contains("Project indexing is in progress") {
        "Project indexing is in progress. Poll index_project before retrying this tool.".to_string()
    } else if error.to_string().contains("not allowed")
        || error.to_string().contains("No default project root")
    {
        error.to_string()
    } else {
        "Failed to load project context. Check project_path, allowlist, and index state."
            .to_string()
    };
    create_error_response(id, -32603, &message)
}
```

`handle_call_tool_inner` içinde `// Resolve Engine` bloğundan fonksiyon sonuna kadar olan kısmı şununla değiştir:

```rust
    let project_key = state.project_key(project_path);
    let loaded = match state.get_engine(project_path).await {
        Ok(loaded) => loaded,
        Err(error) => return Ok(engine_error_response(id, tool_name, &error)),
    };
    let engine = loaded.engine.clone();

    let result = match tool_name {
        "get_context" => tools::get_context(&engine, &arguments).await?,
        "search_code" => tools::search_code(&engine, &arguments).await?,
        "find_nodes" => tools::find_nodes(&engine, &arguments).await?,
        "read_graph" => tools::read_graph(&engine, &arguments).await?,
        "find_usages" => tools::find_usages(&engine, &arguments).await?,
        "trace_call_chain" => tools::trace_call_chain(&engine, &arguments).await?,
        "impact_of_change" => tools::impact_of_change(&engine, &arguments).await?,
        "diff_context" => tools::diff_context(&engine, &arguments).await?,
        _ => {
            return Ok(create_error_response(
                id,
                -32602,
                &format!("Unknown tool: {}", tool_name),
            ))
        }
    };

    let result = match project_key {
        Some(_) => crate::freshness::with_freshness_line(
            result,
            &crate::freshness::format_freshness_line(
                &crate::freshness::disabled_freshness(),
                loaded.indexed_at,
                ccm_core::unix_now_secs(),
            ),
        ),
        None => result,
    };

    Ok(create_success_response(id, serde_json::to_value(result)?))
}
```

- [ ] **Step 7: Testleri çalıştır**

Run: `cargo test -p ccm-mcp`
Expected: tüm testler PASS (`freshness_line_reports_disabled_auto_refresh_and_index_age` dahil). Mevcut testler araç metnini `contains(...)` ile doğruladığından başa eklenen satır onları etkilemez; `starts_with` ile metin başını doğrulayan bir test çıkarsa beklentiyi `contains` olarak güncelle.

- [ ] **Step 8: Kapılar**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings`
Expected: temiz çıktı.

- [ ] **Step 9: Commit**

```bash
git add mcp/src/freshness.rs mcp/src/main.rs mcp/src/server.rs mcp/tests/auto_refresh_test.rs
git commit -m "$(cat <<'EOF'
feat(mcp): prefix read results with an index freshness line and keep one engine per project

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 6: Watcher ve arka plan yenileme görevi

**Files:**
- Modify: `mcp/Cargo.toml` — `notify = "6.1"`
- Modify: `mcp/src/freshness.rs` — watcher, yenileme döngüsü, bekleme
- Modify: `mcp/src/server.rs` — `freshness` alanı, `ensure_auto_refresh`, `freshness_handle`, `wait_until_fresh`, `FRESHNESS_WAIT_BUDGET`, `handle_call_tool_inner`
- Modify: `mcp/src/tools.rs` — `IndexModeArg` ve `run_index_worker_process` `pub(crate)`; `index_now` ve `run_index_project` başarıda `ensure_auto_refresh` + `request_refresh`; `schedule_semantic_upgrade` (~617) yükseltmeyi kaydeder
- Modify: `README.md` (~197), `npm/README.md` (~147) — `CCM_AUTO_REFRESH`
- Test: `mcp/tests/auto_refresh_test.rs`

**Interfaces:**
- Consumes: `ccm_core::{build_watch_filter, is_watch_relevant_path, WatchFilter}` (Task 3); `freshness::{ProjectFreshness, WatcherStatus, is_settled, disabled_freshness, format_freshness_line, with_freshness_line}`, `engine_error_response`, `ServerState::project_key` (Task 5); `ServerState::{get_engine, project_index_lock, project_db_path}` (mevcut); test yardımcıları `McpSession`, `found_node`, `poll_find_nodes` (Task 5).
- Produces: `freshness::{FreshnessHandle, auto_refresh_enabled, inactive_handle, start_auto_refresh, request_rescan, wait_until_fresh}`; `ServerState::{ensure_auto_refresh, freshness_handle, request_refresh, wait_until_fresh, begin_semantic_upgrade, end_semantic_upgrade, semantic_upgrade_running}`.

- [ ] **Step 1: Başarısız testleri yaz**

`mcp/tests/auto_refresh_test.rs` sonuna ekle:

```rust
#[test]
fn saved_change_becomes_searchable_without_manual_index() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn existing_symbol() {}\n")?;
    let mut session = McpSession::start(project.path(), &[])?;
    session.call_tool("index_now", json!({ "project_path": project.path() }))?;

    fs::write(project.path().join("added.rs"), "fn freshly_saved_symbol() {}\n")?;
    let text = poll_find_nodes(&mut session, "freshly_saved_symbol", Duration::from_secs(10), |text| {
        found_node(text, "freshly_saved_symbol")
    })?;

    assert!(
        text.starts_with("_Index: fresh · auto-refresh on_"),
        "unexpected freshness line: {text}"
    );
    Ok(())
}

#[test]
fn slow_refresh_returns_stale_result_within_budget() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn existing_symbol() {}\n")?;
    let mut session =
        McpSession::start(project.path(), &[("CCM_INTERNAL_INDEX_TEST_DELAY_MS", "5000")])?;
    session.call_tool("index_now", json!({ "project_path": project.path() }))?;

    // index_now sonrası başlangıç yakalaması da 5 sn sürer; kaydedilen değişiklik beklemede kalır.
    fs::write(project.path().join("added.rs"), "fn pending_symbol() {}\n")?;
    std::thread::sleep(Duration::from_millis(500));
    let started = Instant::now();
    let text = session.call_tool("find_nodes", json!({ "query": "existing_symbol" }))?;

    assert!(
        started.elapsed() < Duration::from_secs(4),
        "read exceeded the 2 s budget: {:?}",
        started.elapsed()
    );
    assert!(text.starts_with("_Index: stale · "), "unexpected freshness line: {text}");
    assert!(text.contains("refresh running"), "unexpected freshness line: {text}");
    assert!(found_node(&text, "existing_symbol"));
    Ok(())
}

#[test]
fn disabled_auto_refresh_keeps_manual_semantics() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn existing_symbol() {}\n")?;
    let mut session = McpSession::start(project.path(), &[("CCM_AUTO_REFRESH", "0")])?;
    session.call_tool("index_now", json!({ "project_path": project.path() }))?;

    fs::write(project.path().join("added.rs"), "fn unindexed_symbol() {}\n")?;
    std::thread::sleep(Duration::from_millis(1_500));
    let text = session.call_tool("find_nodes", json!({ "query": "unindexed_symbol" }))?;

    assert!(!found_node(&text, "unindexed_symbol"), "auto-refresh must stay off: {text}");
    assert!(text.starts_with("_Index: auto-refresh off"), "unexpected freshness line: {text}");
    Ok(())
}

#[test]
fn ignored_and_artifact_writes_do_not_mark_index_stale() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    fs::write(project.path().join(".gitignore"), "generated/\n")?;
    fs::write(project.path().join("main.rs"), "fn existing_symbol() {}\n")?;
    let mut session =
        McpSession::start(project.path(), &[("CCM_INTERNAL_INDEX_TEST_DELAY_MS", "3000")])?;
    session.call_tool("index_now", json!({ "project_path": project.path() }))?;
    poll_find_nodes(&mut session, "existing_symbol", Duration::from_secs(15), |text| {
        text.starts_with("_Index: fresh")
    })?;

    // Gerçek değişiklik yeni generation yazar; indeksin kendi dosyaları ikinci
    // bir (3 sn'lik) yenilemeyi tetiklerse hemen sonraki okuma bayat görünür.
    fs::write(project.path().join("added.rs"), "fn real_change_symbol() {}\n")?;
    poll_find_nodes(&mut session, "real_change_symbol", Duration::from_secs(15), |text| {
        found_node(text, "real_change_symbol") && text.starts_with("_Index: fresh")
    })?;
    std::thread::sleep(Duration::from_millis(800));
    let started = Instant::now();
    let after_refresh = session.call_tool("find_nodes", json!({ "query": "existing_symbol" }))?;
    assert!(after_refresh.starts_with("_Index: fresh"), "artifact writes re-triggered: {after_refresh}");
    assert!(started.elapsed() < Duration::from_secs(1));

    fs::create_dir_all(project.path().join("generated"))?;
    fs::write(project.path().join("generated/out.rs"), "fn generated_symbol() {}\n")?;
    std::thread::sleep(Duration::from_millis(800));
    let started = Instant::now();
    let after_ignored = session.call_tool("find_nodes", json!({ "query": "existing_symbol" }))?;
    assert!(after_ignored.starts_with("_Index: fresh"), "ignored write triggered: {after_ignored}");
    assert!(started.elapsed() < Duration::from_secs(1));
    Ok(())
}

#[test]
fn atomic_rename_save_is_picked_up() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn before_save() {}\n")?;
    let mut session = McpSession::start(project.path(), &[])?;
    session.call_tool("index_now", json!({ "project_path": project.path() }))?;

    let temp = project.path().join(".main.rs.swp-save");
    fs::write(&temp, "fn after_atomic_save() {}\n")?;
    fs::rename(&temp, project.path().join("main.rs"))?;

    poll_find_nodes(&mut session, "after_atomic_save", Duration::from_secs(10), |text| {
        found_node(text, "after_atomic_save") && text.starts_with("_Index: fresh")
    })?;
    let old = session.call_tool("find_nodes", json!({ "query": "before_save" }))?;
    assert!(!found_node(&old, "before_save"), "old symbol must be gone: {old}");
    Ok(())
}

#[test]
fn bulk_change_coalesces_and_settles() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn existing_symbol() {}\n")?;
    let mut session = McpSession::start(project.path(), &[])?;
    session.call_tool("index_now", json!({ "project_path": project.path() }))?;

    for index in 0..200 {
        fs::write(
            project.path().join(format!("bulk_{index}.rs")),
            format!("fn bulk_symbol_{index}() {{}}\n"),
        )?;
    }

    poll_find_nodes(&mut session, "bulk_symbol_199", Duration::from_secs(30), |text| {
        found_node(text, "bulk_symbol_199") && text.starts_with("_Index: fresh")
    })?;
    let first = session.call_tool("find_nodes", json!({ "query": "bulk_symbol_0" }))?;
    assert!(found_node(&first, "bulk_symbol_0"), "{first}");
    Ok(())
}

#[test]
fn graph_only_refresh_reports_semantic_notice() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn existing_symbol() {}\n")?;
    let mut session = McpSession::start(
        project.path(),
        &[
            ("CCM_DISABLE_EMBEDDER", "0"),
            ("EMBEDDING_HOST", "http://127.0.0.1:9"),
            ("EMBEDDING_TIMEOUT_SECS", "2"),
        ],
    )?;
    session.call_tool("index_now", json!({ "project_path": project.path() }))?;

    fs::write(project.path().join("added.rs"), "fn graph_only_symbol() {}\n")?;
    let text = poll_find_nodes(&mut session, "graph_only_symbol", Duration::from_secs(20), |text| {
        found_node(text, "graph_only_symbol")
    })?;

    assert!(
        text.contains("semantic search unavailable"),
        "unexpected freshness line: {text}"
    );
    Ok(())
}

#[test]
fn quick_index_upgrade_defers_refresh_without_blocking_reads() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn existing_symbol() {}\n")?;
    // Gecikme hem hızlı indeksi hem de ayrık semantik yükseltme sürecini uzatır.
    let mut session =
        McpSession::start(project.path(), &[("CCM_INTERNAL_INDEX_TEST_DELAY_MS", "3000")])?;
    session.call_tool(
        "index_now",
        json!({ "project_path": project.path(), "mode": "quick" }),
    )?;

    fs::write(project.path().join("added.rs"), "fn during_upgrade_symbol() {}\n")?;
    std::thread::sleep(Duration::from_millis(800));
    let started = Instant::now();
    let text = session.call_tool("find_nodes", json!({ "query": "existing_symbol" }))?;
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "reads must not wait while the upgrade defers the refresh: {:?}",
        started.elapsed()
    );
    assert!(
        text.contains("waiting for semantic upgrade"),
        "unexpected freshness line: {text}"
    );

    poll_find_nodes(&mut session, "during_upgrade_symbol", Duration::from_secs(20), |text| {
        found_node(text, "during_upgrade_symbol") && text.starts_with("_Index: fresh")
    })?;
    Ok(())
}

#[test]
fn manual_index_during_auto_refresh_succeeds() -> Result<(), Box<dyn Error>> {
    let project = tempdir()?;
    fs::write(project.path().join("main.rs"), "fn existing_symbol() {}\n")?;
    let mut session = McpSession::start(project.path(), &[])?;
    session.call_tool("index_now", json!({ "project_path": project.path() }))?;

    fs::write(project.path().join("added.rs"), "fn raced_symbol() {}\n")?;
    let manual = session.call_tool("index_now", json!({ "project_path": project.path() }))?;
    assert!(
        manual.contains("Project index refreshed successfully")
            || manual.contains("already up to date"),
        "manual index must succeed next to auto-refresh: {manual}"
    );

    poll_find_nodes(&mut session, "raced_symbol", Duration::from_secs(10), |text| {
        found_node(text, "raced_symbol") && text.starts_with("_Index: fresh")
    })?;
    Ok(())
}
```

- [ ] **Step 2: Testlerin başarısız olduğunu doğrula**

Run: `cargo test -p ccm-mcp --test auto_refresh_test`
Expected: yeni testler FAIL (ör. `condition not met within 10s; last output: _Index: auto-refresh off · indexed 0s ago ...`); Task 5 testi PASS.

- [ ] **Step 3: Bağımlılık**

`mcp/Cargo.toml` `[dependencies]` altına ekle:

```toml
notify = "6.1"
```

- [ ] **Step 4: Worker'ı crate içine aç**

`mcp/src/tools.rs` içinde `enum IndexModeArg` → `pub(crate) enum IndexModeArg`; `async fn run_index_worker_process` → `pub(crate) async fn run_index_worker_process`.

- [ ] **Step 5: `freshness` modülüne watcher ve yenileme döngüsünü ekle**

`mcp/src/freshness.rs` başındaki `use crate::protocol::{ToolResult, ToolResultContent};` satırını şununla değiştir:

```rust
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, watch};

use crate::protocol::{ToolResult, ToolResultContent};
use crate::server::ServerState;

/// Değişiklik olaylarının birleştirildiği sessizlik süresi.
const DEBOUNCE: Duration = Duration::from_millis(300);
/// Başarısız yenileme için toplam deneme sayısı.
const MAX_REFRESH_ATTEMPTS: usize = 3;
/// Denemeler arası bekleme: ilk hatadan sonra 1 sn, ikinciden sonra 2 sn.
const RETRY_DELAYS: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(2)];
/// Tazelik satırına yazılan hata özetinin azami uzunluğu.
const ERROR_SUMMARY_CHARS: usize = 160;
```

Dosyanın sonuna ekle:

```rust
/// Bir projenin yayınlanan tazelik durumu ve varsa çalışan watcher'ı.
pub(crate) struct FreshnessHandle {
    pub(crate) state: watch::Sender<ProjectFreshness>,
    /// Yenileme görevine sinyal kanalı; watcher'ı olmayan handle'da `None`.
    signals: Option<mpsc::UnboundedSender<RefreshSignal>>,
    /// Watcher bu handle yaşadıkça çalışır; bırakılırsa izleme durur.
    _watcher: std::sync::Mutex<Option<notify::RecommendedWatcher>>,
}

/// Elle indeksleme sonrası durumun doğrulanması için tam karşılaştırma ister;
/// böylece önceki başarısız yenilemeden kalan hata satırı temizlenir. Watcher'ı
/// olmayan handle'da yapılacak iş yoktur.
pub(crate) fn request_rescan(handle: &FreshnessHandle) {
    if let Some(signals) = &handle.signals {
        if signals.send(RefreshSignal::Rescan).is_err() {
            tracing::warn!("Refresh loop is not running; rescan request was dropped");
        }
    }
}

/// Yenileme görevine giden sinyaller.
enum RefreshSignal {
    /// Filtreden geçen, değişmiş (eklenmiş/silinmiş/yeniden adlandırılmış) yol.
    Changed(PathBuf),
    /// Olay kaybı ihtimali veya başlangıç yakalaması: tam karşılaştırma gerekir.
    Rescan,
    /// Watcher hata bildirdi; olay kaybolmuş olabileceği için yenileme tetiklenir.
    WatchError(String),
}

/// `CCM_AUTO_REFRESH` yalnızca açıkça `0/false/no/off` ise otomatik yenilemeyi kapatır.
pub(crate) fn auto_refresh_enabled() -> bool {
    match std::env::var("CCM_AUTO_REFRESH") {
        Ok(value) => !matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "0" | "false" | "no" | "off"
        ),
        Err(_) => true,
    }
}

/// Watcher'ı olmayan (kapalı veya kullanılamaz) bir handle üretir.
pub(crate) fn inactive_handle(watcher: WatcherStatus) -> Arc<FreshnessHandle> {
    let (state, _) = watch::channel(ProjectFreshness {
        watcher,
        pending_paths: 0,
        refresh_in_flight: false,
        waiting_for_upgrade: false,
        last_error: None,
        semantic_unavailable: None,
    });
    Arc::new(FreshnessHandle {
        state,
        signals: None,
        _watcher: std::sync::Mutex::new(None),
    })
}

/// Hata metnini tazelik satırı için ilk satırına ve sınırlı uzunluğa indirir.
fn summarize_error(message: &str) -> String {
    let first_line = message.lines().next().unwrap_or_default().trim();
    let mut summary: String = first_line.chars().take(ERROR_SUMMARY_CHARS).collect();
    if first_line.chars().count() > ERROR_SUMMARY_CHARS {
        summary.push('…');
    }
    summary
}

/// Proje için watcher'ı ve tek yenileme görevini başlatır; sunucu kapalıyken
/// yapılan değişiklikleri yakalamak için hemen bir tam karşılaştırma ister.
/// Filtre veya watcher kurulamazsa sebebiyle `Unavailable` handle döner.
pub(crate) fn start_auto_refresh(
    server: Arc<ServerState>,
    project_key: String,
    db_path: PathBuf,
) -> Arc<FreshnessHandle> {
    let root = PathBuf::from(&project_key);
    let filter = match ccm_core::build_watch_filter(&root, &db_path) {
        Ok(filter) => filter,
        Err(error) => {
            tracing::warn!(project = %project_key, error = %error, "Auto-refresh unavailable: watch filter could not be built");
            return inactive_handle(WatcherStatus::Unavailable(summarize_error(
                &error.to_string(),
            )));
        }
    };
    let (signals_tx, signals_rx) = mpsc::unbounded_channel();
    let watcher = match start_watcher(filter, &root, signals_tx.clone()) {
        Ok(watcher) => watcher,
        Err(error) => {
            tracing::warn!(project = %project_key, error = %error, "Auto-refresh unavailable: file watcher could not start");
            return inactive_handle(WatcherStatus::Unavailable(summarize_error(
                &error.to_string(),
            )));
        }
    };
    // Başlangıç yakalaması hemen kuyruğa girer; okumalar onu bekler.
    let (state, _) = watch::channel(ProjectFreshness {
        watcher: WatcherStatus::Active,
        pending_paths: 0,
        refresh_in_flight: true,
        waiting_for_upgrade: false,
        last_error: None,
        semantic_unavailable: None,
    });
    let handle = Arc::new(FreshnessHandle {
        state,
        signals: Some(signals_tx.clone()),
        _watcher: std::sync::Mutex::new(Some(watcher)),
    });
    signals_tx
        .send(RefreshSignal::Rescan)
        .expect("refresh loop receiver is owned by this function");
    tokio::spawn(run_refresh_loop(
        server,
        project_key,
        db_path,
        handle.clone(),
        signals_rx,
    ));
    handle
}

/// `notify` watcher'ını açar; filtreden geçen her yol için sinyal yollar.
fn start_watcher(
    filter: ccm_core::WatchFilter,
    root: &Path,
    signals: mpsc::UnboundedSender<RefreshSignal>,
) -> notify::Result<notify::RecommendedWatcher> {
    use notify::Watcher;
    let mut watcher =
        notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
            for signal in signals_for_event(&filter, result) {
                // Alıcı yalnızca yenileme görevi bittiğinde kapanır: sunucu kapanıyordur.
                if signals.send(signal).is_err() {
                    return;
                }
            }
        })?;
    watcher.watch(root, notify::RecursiveMode::Recursive)?;
    Ok(watcher)
}

/// Tek bir watcher olayını yenileme sinyallerine çevirir.
fn signals_for_event(
    filter: &ccm_core::WatchFilter,
    result: notify::Result<notify::Event>,
) -> Vec<RefreshSignal> {
    match result {
        Ok(event) if event.need_rescan() => vec![RefreshSignal::Rescan],
        // Erişim olayları içerik değiştirmez; yazmalar ayrıca Modify olarak gelir.
        Ok(event) if matches!(event.kind, notify::EventKind::Access(_)) => Vec::new(),
        Ok(event) => event
            .paths
            .into_iter()
            .filter(|path| ccm_core::is_watch_relevant_path(filter, path))
            .map(RefreshSignal::Changed)
            .collect(),
        Err(error) => vec![RefreshSignal::WatchError(error.to_string())],
    }
}

/// Sinyalin bekleyen değişiklik kümesindeki anahtarı; tam karşılaştırma
/// isteyen sinyaller proje kökünü işaretler.
fn pending_path(signal: &RefreshSignal, root: &Path) -> PathBuf {
    match signal {
        RefreshSignal::Changed(path) => path.clone(),
        RefreshSignal::Rescan | RefreshSignal::WatchError(_) => root.to_path_buf(),
    }
}

/// Watcher hatalarını görünür kılar (yenileme ayrıca tetiklenir).
fn log_signal(project_key: &str, signal: &RefreshSignal) {
    if let RefreshSignal::WatchError(message) = signal {
        tracing::warn!(project = %project_key, error = %message, "File watcher reported an error; scheduling a full refresh");
    }
}

/// Proje başına tek yenileme görevi: olayları debounce eder, worker'ı sırayla
/// çalıştırır ve durumu yayınlar. Kuyruk boşalmadan durum "taze" yayınlanmaz.
async fn run_refresh_loop(
    server: Arc<ServerState>,
    project_key: String,
    db_path: PathBuf,
    handle: Arc<FreshnessHandle>,
    mut signals: mpsc::UnboundedReceiver<RefreshSignal>,
) {
    let root = PathBuf::from(&project_key);
    let mut pending: HashSet<PathBuf> = HashSet::new();
    let mut waiting_for_upgrade = false;
    loop {
        // Bekleyen iş yoksa ya da iş yükseltme yüzünden ertelendiyse yeni sinyal
        // beklenir; yükseltme bitince `end_semantic_upgrade` döngüyü uyandırır.
        if pending.is_empty() || waiting_for_upgrade {
            let Some(signal) = signals.recv().await else {
                return;
            };
            log_signal(&project_key, &signal);
            pending.insert(pending_path(&signal, &root));
            let count = pending.len();
            handle
                .state
                .send_modify(|freshness| freshness.pending_paths = count);
        }
        loop {
            match tokio::time::timeout(DEBOUNCE, signals.recv()).await {
                Ok(Some(signal)) => {
                    log_signal(&project_key, &signal);
                    pending.insert(pending_path(&signal, &root));
                    let count = pending.len();
                    handle
                        .state
                        .send_modify(|freshness| freshness.pending_paths = count);
                }
                Ok(None) => return,
                Err(_) => break,
            }
        }
        // Hızlı indeksin semantik yükseltmesi sürerken `update_index` eksik
        // vektör tablosunu onarmaya ya da tam yeniden indekslemeye girip
        // yükseltmenin embedding işini ikinci kez yapar; yenileme ertelenir.
        waiting_for_upgrade = server.semantic_upgrade_running(&project_key);
        let count = pending.len();
        if waiting_for_upgrade {
            handle.state.send_modify(|freshness| {
                freshness.pending_paths = count;
                freshness.refresh_in_flight = false;
                freshness.waiting_for_upgrade = true;
            });
            continue;
        }
        handle.state.send_modify(|freshness| {
            freshness.pending_paths = count;
            freshness.refresh_in_flight = true;
            freshness.waiting_for_upgrade = false;
        });
        pending.clear();
        let outcome = refresh_with_retries(&server, &project_key, &db_path).await;
        // Yenileme sırasında kuyruğa düşen olaylar bir sonraki turu başlatır.
        while let Ok(signal) = signals.try_recv() {
            log_signal(&project_key, &signal);
            pending.insert(pending_path(&signal, &root));
        }
        let queued = pending.len();
        handle.state.send_modify(|freshness| {
            freshness.pending_paths = queued;
            freshness.refresh_in_flight = queued > 0;
            match &outcome {
                Ok(stats) => {
                    freshness.last_error = None;
                    freshness.semantic_unavailable = stats.semantic_unavailable.clone();
                }
                Err(error) => {
                    freshness.last_error = Some(summarize_error(&error.to_string()));
                }
            }
        });
    }
}

/// Worker'ı en fazla üç kez çalıştırır; başarısız denemeleri uyarı olarak
/// log'lar ve 1 sn / 2 sn bekler. Son hata olduğu gibi döner.
async fn refresh_with_retries(
    server: &Arc<ServerState>,
    project_key: &str,
    db_path: &Path,
) -> anyhow::Result<ccm_core::IndexStats> {
    let db_path = db_path.to_string_lossy().to_string();
    let mut attempt = 1;
    loop {
        match refresh_once(server, project_key, &db_path).await {
            Ok(stats) => return Ok(stats),
            Err(error) if attempt < MAX_REFRESH_ATTEMPTS => {
                let delay = RETRY_DELAYS[attempt - 1];
                tracing::warn!(
                    project = %project_key,
                    attempt,
                    retry_in_ms = delay.as_millis() as u64,
                    error = %error,
                    "Auto-refresh failed; retrying"
                );
                tokio::time::sleep(delay).await;
                attempt += 1;
            }
            Err(error) => {
                tracing::warn!(
                    project = %project_key,
                    attempt,
                    error = %error,
                    "Auto-refresh failed; the active generation keeps serving reads"
                );
                return Err(error);
            }
        }
    }
}

/// Proje kilidi altında tek artımlı güncelleme çalıştırır ve yeni generation'ı
/// önbelleğe alır (generation değişmediyse `get_engine` önbellekten döner).
async fn refresh_once(
    server: &Arc<ServerState>,
    project_key: &str,
    db_path: &str,
) -> anyhow::Result<ccm_core::IndexStats> {
    let lock = server.project_index_lock(project_key);
    let _guard = lock.lock().await;
    let stats = crate::tools::run_index_worker_process(
        project_key,
        db_path,
        crate::tools::IndexModeArg::Full,
    )
    .await?;
    server.get_engine(Some(project_key)).await?;
    Ok(stats)
}

/// Süren yenilemenin bitmesini en fazla `budget` kadar bekler; süre dolarsa o
/// anki durumu döndürür (okuma bayat işaretlenir ama hata vermez).
pub(crate) async fn wait_until_fresh(
    handle: &FreshnessHandle,
    budget: Duration,
) -> ProjectFreshness {
    let mut receiver = handle.state.subscribe();
    match tokio::time::timeout(budget, receiver.wait_for(is_settled)).await {
        Ok(Ok(_)) | Err(_) => {}
        Ok(Err(error)) => tracing::warn!(
            error = %error,
            "Freshness channel closed; reporting the last known state"
        ),
    }
    handle.state.borrow().clone()
}
```

- [ ] **Step 6: `ServerState` entegrasyonu**

`mcp/src/server.rs` dosya başına sabit ekle:

```rust
/// Okuma araçlarının süren yenilemeyi bekleyeceği en uzun süre.
const FRESHNESS_WAIT_BUDGET: std::time::Duration = std::time::Duration::from_secs(2);
```

`ServerState` yapısına ekle:

```rust
    /// Proje başına otomatik yenileme durumu (anahtar: kanonik proje yolu).
    freshness: std::sync::Mutex<
        std::collections::HashMap<String, Arc<crate::freshness::FreshnessHandle>>,
    >,
```

`ServerState` yapısına ayrıca ekle:

```rust
    /// Semantik yükseltmesi süren projeler (anahtar: kanonik proje yolu).
    semantic_upgrades: std::sync::Mutex<std::collections::HashSet<String>>,
```

`ServerState::new` sonundaki `Ok(Self { ... })` literaline ekle:

```rust
            freshness: std::sync::Mutex::new(std::collections::HashMap::new()),
            semantic_upgrades: std::sync::Mutex::new(std::collections::HashSet::new()),
```

`impl ServerState` içine ekle:

```rust
    /// Proje için otomatik yenilemeyi bir kez başlatır. Kapalıysa ya da izlenen
    /// proje sınırı doluysa ilgili durumla kaydedilir; sonraki çağrılar bir şey
    /// yapmaz.
    pub(crate) fn ensure_auto_refresh(self: &Arc<Self>, project_key: &str) {
        let mut handles = self.freshness.lock().unwrap();
        if handles.contains_key(project_key) {
            return;
        }
        let handle = if !crate::freshness::auto_refresh_enabled() {
            crate::freshness::inactive_handle(crate::freshness::WatcherStatus::Disabled)
        } else if handles.len() >= engine_cache_size() {
            crate::freshness::inactive_handle(crate::freshness::WatcherStatus::Unavailable(
                "watcher limit reached".to_string(),
            ))
        } else {
            match self.project_db_path(project_key) {
                Ok(db_path) => crate::freshness::start_auto_refresh(
                    self.clone(),
                    project_key.to_string(),
                    db_path,
                ),
                Err(error) => crate::freshness::inactive_handle(
                    crate::freshness::WatcherStatus::Unavailable(format!(
                        "index path could not be resolved: {error}"
                    )),
                ),
            }
        };
        handles.insert(project_key.to_string(), handle);
    }

    pub(crate) fn freshness_handle(
        &self,
        project_key: &str,
    ) -> Option<Arc<crate::freshness::FreshnessHandle>> {
        self.freshness.lock().unwrap().get(project_key).cloned()
    }

    /// Elle indeksleme sonrası otomatik yenilemeden durum doğrulaması ister.
    pub(crate) fn request_refresh(&self, project_key: &str) {
        if let Some(handle) = self.freshness_handle(project_key) {
            crate::freshness::request_rescan(&handle);
        }
    }

    /// Hızlı indeksin semantik yükseltmesi başladı; otomatik yenileme ertelenir.
    pub(crate) fn begin_semantic_upgrade(&self, project_key: &str) {
        self.semantic_upgrades
            .lock()
            .unwrap()
            .insert(project_key.to_string());
    }

    /// Yükseltme bitti (başarılı ya da değil); ertelenen yenileme uyandırılır.
    pub(crate) fn end_semantic_upgrade(&self, project_key: &str) {
        self.semantic_upgrades.lock().unwrap().remove(project_key);
        self.request_refresh(project_key);
    }

    pub(crate) fn semantic_upgrade_running(&self, project_key: &str) -> bool {
        self.semantic_upgrades.lock().unwrap().contains(project_key)
    }

    /// Projenin bekleyen yenilemesini en fazla `budget` kadar bekler; otomatik
    /// yenileme kaydı yoksa kapalı durum döner.
    pub(crate) async fn wait_until_fresh(
        &self,
        project_key: &str,
        budget: std::time::Duration,
    ) -> crate::freshness::ProjectFreshness {
        match self.freshness_handle(project_key) {
            Some(handle) => crate::freshness::wait_until_fresh(&handle, budget).await,
            None => crate::freshness::disabled_freshness(),
        }
    }
```

`handle_call_tool_inner` içinde Task 5'te yazılan engine yükleme satırlarını (`let project_key = ...` ile `let engine = loaded.engine.clone();` arası) şununla değiştir:

```rust
    let project_key = state.project_key(project_path);
    // İlk yükleme izin listesini ve indeksin varlığını doğrular; watcher yalnızca
    // izinli ve indeksi olan projelerde başlar.
    if let Err(error) = state.get_engine(project_path).await {
        return Ok(engine_error_response(id, tool_name, &error));
    }
    let freshness = match &project_key {
        Some(key) => {
            state.ensure_auto_refresh(key);
            Some(state.wait_until_fresh(key, FRESHNESS_WAIT_BUDGET).await)
        }
        None => None,
    };
    // Bekleme sırasında yeni generation aktive edilmiş olabilir.
    let loaded = match state.get_engine(project_path).await {
        Ok(loaded) => loaded,
        Err(error) => return Ok(engine_error_response(id, tool_name, &error)),
    };
    let engine = loaded.engine.clone();
```

Sonuca satır ekleyen bloğu değiştir:

```rust
    let result = match freshness {
        Some(freshness) => crate::freshness::with_freshness_line(
            result,
            &crate::freshness::format_freshness_line(
                &freshness,
                loaded.indexed_at,
                ccm_core::unix_now_secs(),
            ),
        ),
        None => result,
    };
```

- [ ] **Step 7: Manuel indeks sonrası izlemeyi başlat**

`mcp/src/tools.rs` `index_now` içinde `refresh_project_engine` başarılı olduktan sonra (Quick kontrolünden önce) ve `run_index_project` içinde aynı noktada ekle:

```rust
            let project_key = crate::server::project_key_for_path(project_path);
            state.ensure_auto_refresh(&project_key);
            state.request_refresh(&project_key);
```

Yeni başlatılan handle için bu iki Rescan sinyali aynı debounce penceresinde tek yenilemede birleşir.

`schedule_semantic_upgrade` içinde `tokio::spawn`'dan önce yükseltmeyi kaydet, iş bitince (başarılı ya da değil) kaydı kaldır. Fonksiyonu şu hale getir (spawn içindeki mevcut `match` aynen kalır):

```rust
fn schedule_semantic_upgrade(
    state: Arc<crate::server::ServerState>,
    project_path: std::sync::Arc<str>,
    db_path: String,
) {
    // Otomatik yenileme yükseltme bitene kadar ertelenir; aksi halde eksik
    // vektör tablosunu görüp aynı embedding işini ikinci kez başlatır.
    let project_key = crate::server::project_key_for_path(&project_path);
    state.begin_semantic_upgrade(&project_key);
    // Detached worker: MCP çıkışında ölmeyen, kendi process grubunda koşan süreç.
    // Yalnızca iş tamamlandığında (süreç hâlâ yaşıyorsa) engine cache tazelenir.
    tokio::spawn(async move {
        let refresh_state = state.clone();
        let refresh_path = project_path.clone();
        match spawn_detached_upgrade_worker(&project_path, &db_path).await {
            Ok(stats) => {
                tracing::info!(
                    nodes = stats.nodes_created,
                    "Background semantic upgrade completed"
                );
                // Yeni generation graph+vektör içerdiğinden cache'i tazele.
                let _ = refresh_state.refresh_project_engine(&refresh_path).await;
            }
            Err(error) => {
                tracing::warn!(error = %error, "Background semantic upgrade failed");
            }
        }
        refresh_state.end_semantic_upgrade(&project_key);
    });
}
```

`index_now` ve `run_index_project` içinde `ensure_auto_refresh`/`request_refresh` satırları Quick kontrolünden önce kalır: `begin_semantic_upgrade` `schedule_semantic_upgrade` içinde eşzamanlı çağrıldığı için yenileme döngüsü 300 ms'lik debounce'tan sonra yükseltmeyi zaten kayıtlı görür.

- [ ] **Step 8: Belgeleme**

`README.md` ve `npm/README.md` içindeki `# MCP Runtime` bloğunda `CCM_MCP_DEBUG=0` satırının altına ekle:

```text
# Re-index automatically when project files change (0 = manual index_now only)
CCM_AUTO_REFRESH=1
```

- [ ] **Step 9: Testleri çalıştır**

Run: `cargo test -p ccm-mcp --test auto_refresh_test`
Expected: tüm testler PASS.

Run: `cargo test -p ccm-mcp`
Expected: tüm testler PASS. Mevcut bir test, iki manuel indeks çağrısı arasında dosya düzenlediği için otomatik yenilemeyle yarışarak düşerse o testin `Command`'ına `.env("CCM_AUTO_REFRESH", "0")` ekle ve üstüne `// Bu test elle indeksleme sözleşmesini doğrular.` yorumunu yaz. Başka bir sebeple düşen test için bu yolu kullanma; kök nedeni düzelt.

- [ ] **Step 10: Kapılar**

Run: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace`
Expected: temiz çıktı, tüm testler PASS.

- [ ] **Step 11: Commit**

```bash
git add mcp/Cargo.toml Cargo.lock mcp/src/freshness.rs mcp/src/server.rs mcp/src/tools.rs mcp/tests/auto_refresh_test.rs README.md npm/README.md
git commit -m "$(cat <<'EOF'
feat(mcp): refresh the index automatically when project files change

Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

### Task 7: Performans kapısı ve son doğrulama

**Files:**
- Kod değişikliği yok; ölçüm sonuçları PR açıklamasına girer. Ölçüm betikleri `/tmp/ccm-bench` altında kalır, repoya girmez.

- [ ] **Step 1: Release build**

Run: `cargo build --release -p ccm-cli -p ccm-mcp`
Expected: başarılı derleme.

- [ ] **Step 2: Django ölçümleri (graf-only)**

Klon yoksa: `git clone -q --depth 1 --branch 5.1 https://github.com/django/django.git /tmp/ccm-bench/django`. Önceki indeksi temizle: `rm -rf /tmp/ccm-bench/django/data`.

```bash
cd /tmp/ccm-bench
BIN=/Users/dogan/Desktop/LLM-Context-Manager/target/release/ccm-cli
export CCM_DISABLE_EMBEDDER=1
time $BIN index --path django            # tam indeks
time $BIN index --path django            # değişiklik yok (hedef ≤ 0,4 sn)
python3 -c "p='django/django/db/models/query.py'; s=open(p).read(); a='    def count(self):\n'; open(p,'w').write(s.replace(a, a+'        # bench edit\n', 1))"
time $BIN index --path django            # tek dosya düzenleme
git -C django checkout -- django/db/models/query.py
```

Expected: değişiklik bulmayan yenileme ≤ 0,4 sn; tek dosya düzenleme başlangıçtaki 2,2 sn'den belirgin düşük.

- [ ] **Step 3: Uçtan uca kaydet → taze (MCP, Django)**

`/tmp/ccm-bench/save_to_fresh.py`:

```python
import json, os, subprocess, sys, time
binary, project = sys.argv[1], sys.argv[2]
env = dict(os.environ, CCM_PROJECT_ROOT=project, CCM_ALLOWED_ROOTS=project, CCM_DISABLE_EMBEDDER="1")
proc = subprocess.Popen([binary], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, env=env, text=True)
rid = 0
def call(method, params):
    global rid
    rid += 1
    proc.stdin.write(json.dumps({"jsonrpc": "2.0", "id": rid, "method": method, "params": params}) + "\n")
    proc.stdin.flush()
    while True:
        reply = json.loads(proc.stdout.readline())
        if reply.get("id") == rid:
            return reply
def find(query):
    reply = call("tools/call", {"name": "find_nodes", "arguments": {"query": query}})
    return reply["result"]["content"][0]["text"]
call("initialize", {})
call("tools/call", {"name": "index_now", "arguments": {"project_path": project}})
while not find("QuerySet").startswith("_Index: fresh"):
    time.sleep(0.2)
path = os.path.join(project, "django/db/models/query.py")
original = open(path).read()
try:
    start = time.perf_counter()
    open(path, "w").write(original + "\n\ndef bench_saved_symbol():\n    return 1\n")
    while True:
        text = find("bench_saved_symbol")
        if "bench_saved_symbol (Score:" in text and text.startswith("_Index: fresh"):
            break
    print(f"save_to_fresh_ms={(time.perf_counter() - start) * 1000:.0f}")
finally:
    open(path, "w").write(original)
    proc.stdin.close()
    proc.wait(timeout=30)
```

Run (3 kez; medyan kaydedilir): `python3 /tmp/ccm-bench/save_to_fresh.py /Users/dogan/Desktop/LLM-Context-Manager/target/release/ccm-mcp /tmp/ccm-bench/django`
Expected: medyan ≤ 2000 ms.

- [ ] **Step 4: Embedding açıkken tek düzenleme (Ollama, küçük repo)**

```bash
git clone -q --depth 1 --branch 3.0.3 https://github.com/pallets/flask.git /tmp/ccm-bench/flask
cd /tmp/ccm-bench
BIN=/Users/dogan/Desktop/LLM-Context-Manager/target/release/ccm-cli
unset CCM_DISABLE_EMBEDDER
export EMBEDDING_MODEL=mxbai-embed-large RUST_LOG=info
$BIN index --path flask 2>/dev/null          # tam semantik indeks (dakikalar sürebilir)
python3 -c "p='flask/src/flask/app.py'; s=open(p).read(); open(p,'w').write(s.replace('def make_response(', 'def make_response(  ', 1))"
time $BIN index --path flask 2>&1 | grep -E "Initial indexing complete"
git -C flask checkout -- src/flask/app.py
```

Expected: log satırında `embedded_chunks` 1–3 ve `reused_chunks` dosyanın kalan parça sayısı; `real` ≤ 1 sn.

- [ ] **Step 5: Hedef karşılaştırması**

Ölçümleri spec'teki başlangıç tablosuyla yan yana bir tabloya yaz (PR açıklamasında kullanılacak). Hedeflerden biri tutmazsa ölçümü ve darboğaz aşamasını (log zaman damgalarından) kullanıcıya raporla; kapsam genişletmeye (P0.4'ten parça çekme) kendi başına karar verme.

- [ ] **Step 6: Tam kapılar ve eval kapıları**

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --release
CCM_DISABLE_EMBEDDER=1 ./target/release/ccm-cli index --path .
jq '.tasks |= map(select(.query.type != "search_code"))' eval/golden_tasks.v3.ccm.json > eval/golden_tasks.v3.structural.json
CCM_DISABLE_EMBEDDER=1 ./target/release/ccm-cli eval --tasks eval/golden_tasks.v3.structural.json --report eval/report.json --baseline eval/report.phase3_baseline.json --min-pass-rate 100 --max-regression 0
CCM_DISABLE_EMBEDDER=1 CCM_EMBEDDING_FIXTURE=eval/fixtures/embeddings.ndjson ./target/release/ccm-cli eval --tasks eval/fixtures/golden_tasks.synthetic.json --report eval/report.semantic-gate.json --min-pass-rate 100 --max-regression 0
```

Expected: hepsi başarılı; iki eval kapısı %100 geçme oranı, 0 regresyon. Kapıların ürettiği üç dosya git'te izlenmez ve ignore da edilmez; `git status --short` çıktısında yalnızca `?? eval/golden_tasks.v3.structural.json`, `?? eval/report.json`, `?? eval/report.semantic-gate.json` olduğunu doğrula ve bu üç dosyayı sil: `rm eval/golden_tasks.v3.structural.json eval/report.json eval/report.semantic-gate.json`.
