use crate::vector::remote::RemoteEmbedder;
use anyhow::{Context, Result};
use arrow_array::{FixedSizeListArray, Float32Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use futures::{StreamExt, TryStreamExt};
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::{connect, Connection};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;

/// NDJSON embedding fixture: doc ve query vektörleri, namespace ile ayrışır.
#[derive(Debug, Default)]
pub struct EmbeddingFixture {
    pub docs: HashMap<String, Vec<f32>>,
    pub queries: HashMap<String, Vec<f32>>,
    pub dim: usize,
}

impl EmbeddingFixture {
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path)
            .with_context(|| format!("embedding fixture okunamadı: {}", path.display()))?;
        let mut fixture = EmbeddingFixture::default();
        for (line_number, line) in content.lines().enumerate() {
            let value: serde_json::Value = serde_json::from_str(line).with_context(|| {
                format!(
                    "embedding fixture satırı ayrıştırılamadı: {}:{}",
                    path.display(),
                    line_number + 1
                )
            })?;
            let kind = value.get("kind").and_then(|k| k.as_str()).unwrap_or("");
            if kind == "meta" {
                if let Some(dim) = value.get("dim").and_then(|d| d.as_u64()) {
                    fixture.dim = dim as usize;
                }
                continue;
            }
            let ns = value
                .get("ns")
                .and_then(|k| k.as_str())
                .unwrap_or("default");
            let id = value
                .get("id")
                .and_then(|k| k.as_str())
                .context("fixture satırında id eksik")?;
            let vector: Vec<f32> = value
                .get("vector")
                .and_then(|v| v.as_array())
                .and_then(|items| {
                    items
                        .iter()
                        .map(|item| item.as_f64().map(|f| f as f32))
                        .collect::<Option<Vec<f32>>>()
                })
                .context("fixture satırında geçersiz vector")?;
            let key = format!("{}|{}", ns, id);
            if kind == "doc" {
                fixture.docs.insert(key, vector);
            } else if kind == "query" {
                fixture.queries.insert(key, vector);
            }
        }
        if fixture.dim == 0 {
            fixture.dim = fixture
                .docs
                .values()
                .next()
                .map(|vector| vector.len())
                .unwrap_or(crate::vector::hash_embed::HASH_EMBED_DIM);
        }
        Ok(fixture)
    }

    pub fn doc_vector(&self, ns: &str, chunk_id: &str) -> Result<Vec<f32>> {
        self.docs
            .get(&format!("{}|{}", ns, chunk_id))
            .cloned()
            .with_context(|| {
                format!(
                    "embedding fixture'da eksik doc chunk: ns={} id={}",
                    ns, chunk_id
                )
            })
    }

    pub fn query_vector(&self, ns: &str, query: &str) -> Result<Vec<f32>> {
        let key = format!("{}|{}", ns, query);
        if let Some(vector) = self.queries.get(&key) {
            return Ok(vector.clone());
        }
        // Dinamik sorgular (örn. predict_context "related to X") fixture'da
        // olamaz; aynı deterministik hash algoritmasıyla anında üretilir.
        Ok(crate::vector::hash_embed::embed_text(query, self.dim))
    }
}

fn fixture_cache() -> &'static Mutex<HashMap<PathBuf, Arc<EmbeddingFixture>>> {
    static CACHE: std::sync::OnceLock<Mutex<HashMap<PathBuf, Arc<EmbeddingFixture>>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn load_fixture_cached(path: &Path) -> Result<Arc<EmbeddingFixture>> {
    let cache = fixture_cache();
    if let Some(cached) = cache.lock().unwrap().get(path) {
        return Ok(Arc::clone(cached));
    }
    let fixture = Arc::new(EmbeddingFixture::load(path)?);
    cache
        .lock()
        .unwrap()
        .insert(path.to_path_buf(), Arc::clone(&fixture));
    Ok(fixture)
}

/// DB uri'sinden namespace çıkarır: `<repo>/data/ccm_db` → repo dizin adı.
fn namespace_for_uri(uri: &str) -> String {
    let path = Path::new(uri);
    if let Some(generations_root) = path.ancestors().find(|ancestor| {
        ancestor
            .file_name()
            .is_some_and(|name| name == ".ccm-generations")
    }) {
        if let Some(project_name) = generations_root
            .parent()
            .and_then(Path::parent)
            .and_then(Path::file_name)
        {
            return project_name.to_string_lossy().to_string();
        }
    }
    path.parent()
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "default".to_string())
}

pub struct LanceDbStore {
    conn: Connection,
    table_name: String,
    embedder: Option<RemoteEmbedder>,
    fixture: Option<Arc<EmbeddingFixture>>,
    fixture_ns: String,
}

impl LanceDbStore {
    pub async fn new(uri: &str, table_name: &str) -> Result<Self> {
        Self::new_with_fixture_namespace(uri, table_name, None).await
    }

    pub(crate) async fn new_with_fixture_namespace(
        uri: &str,
        table_name: &str,
        fixture_namespace: Option<&str>,
    ) -> Result<Self> {
        let conn = connect(uri).execute().await?;

        let embedder_disabled = std::env::var("CCM_DISABLE_EMBEDDER")
            .or_else(|_| std::env::var("EMBEDDING_DISABLED"))
            .map(|v| matches!(v.to_lowercase().as_str(), "1" | "true" | "yes"))
            .unwrap_or(false);

        // Try to initialize Remote embedder from environment.
        // If it fails (no API key), we operate in "no-vector" mode or warn.
        let embedder = if embedder_disabled {
            tracing::warn!("Embedder disabled via environment. Semantic search will be disabled.");
            None
        } else {
            match RemoteEmbedder::from_env() {
                Ok(e) => Some(e),
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "Remote Embedder init failed. Semantic search will be disabled."
                    );
                    None
                }
            }
        };

        let fixture = match std::env::var("CCM_EMBEDDING_FIXTURE").ok() {
            Some(path) if !path.trim().is_empty() => {
                let fixture_path = PathBuf::from(path);
                tracing::info!(
                    path = %fixture_path.display(),
                    "Embedding fixture modu etkin"
                );
                Some(load_fixture_cached(&fixture_path)?)
            }
            _ => None,
        };
        let fixture_ns = fixture_namespace
            .map(str::to_string)
            .unwrap_or_else(|| namespace_for_uri(uri));

        Ok(Self {
            conn,
            table_name: table_name.to_string(),
            embedder,
            fixture,
            fixture_ns,
        })
    }

    /// Drops the existing table (if any) to prevent duplicate vectors on full re-index.
    pub async fn reset_table(&self) -> Result<()> {
        let table_exists = self
            .conn
            .open_table(&self.table_name)
            .execute()
            .await
            .is_ok();

        if table_exists {
            self.conn
                .drop_table(&self.table_name, &[])
                .await
                .with_context(|| {
                    format!(
                        "vector table '{}' could not be reset; rebuild was aborted",
                        self.table_name
                    )
                })?;
        }

        Ok(())
    }

    pub async fn validate_table(&self) -> Result<usize> {
        let table = self
            .conn
            .open_table(&self.table_name)
            .execute()
            .await
            .with_context(|| {
                format!(
                    "vector table '{}' is missing or unreadable",
                    self.table_name
                )
            })?;
        table
            .count_rows(None)
            .await
            .with_context(|| format!("vector table '{}' could not be scanned", self.table_name))
    }

    /// Embeds texts and inserts them into the LanceDB table.
    pub async fn add_documents(&self, ids: Vec<String>, texts: Vec<String>) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }

        let max_chars: usize = std::env::var("CCM_MAX_CHUNK_CHARS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1000)
            .max(1); // chunks(0) and div_ceil(0) both panic
        let overlap: usize = std::env::var("CCM_CHUNK_OVERLAP")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(100)
            .min(max_chars.saturating_sub(1)); // overlap < max_chars guarantees forward progress

        let mut all_chunks = Vec::new();
        let mut all_chunk_ids = Vec::new();

        for (i, text) in texts.iter().enumerate() {
            if text.len() <= max_chars {
                all_chunks.push(text.clone());
                all_chunk_ids.push(ids[i].clone());
            } else {
                for (chunk_idx, chunk) in semantic_chunks(text, max_chars, overlap)
                    .into_iter()
                    .enumerate()
                {
                    all_chunks.push(chunk);
                    all_chunk_ids.push(format!("{}#chunk{}", ids[i], chunk_idx));
                }
            }
        }

        // 1. Generate Embeddings in batches for performance.
        // Fixture modunda vektörler NDJSON'dan alınır (eksik chunk hata üretir);
        // aksi halde embedder yoksa vektör indeksleme atlanır (mevcut davranış).
        let batch_size: usize = std::env::var("CCM_EMBED_BATCH_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(32)
            .max(1); // guard: chunks(0) panics at runtime
        let mut embeddings: Vec<Vec<f32>> = Vec::with_capacity(all_chunks.len());

        if let Some(fixture) = self.fixture.as_ref() {
            for chunk_id in &all_chunk_ids {
                embeddings.push(fixture.doc_vector(&self.fixture_ns, chunk_id)?);
            }
        } else {
            let embedder = match self.embedder.as_ref() {
                Some(e) => e,
                None => return Ok(()),
            };
            let total_batches = all_chunks.len().div_ceil(batch_size);
            // Sınırlı eşzamanlılık: Ollama `num_parallel` işçisiyle eşzamanlı
            // istekleri kuyruğa alıp işleyebilir; seri bekleme yerine
            // CCM_EMBED_CONCURRENCY kadar batch paralel gider. Vektörler
            // batch indeksine göre toplanıp sırayla birleştirilir, böylece
            // id ↔ vektör hizası bozulmaz.
            let concurrency: usize = std::env::var("CCM_EMBED_CONCURRENCY")
                .ok()
                .and_then(|value| value.parse().ok())
                .unwrap_or(2)
                .clamp(1, 8);
            let mut collected: Vec<Option<Vec<Vec<f32>>>> = vec![None; total_batches];
            let batch_futures =
                all_chunks
                    .chunks(batch_size)
                    .enumerate()
                    .map(|(batch_idx, batch)| {
                        let batch_texts: Vec<String> = batch.to_vec();
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
            for batch in collected {
                embeddings.extend(batch.expect("embedding batch"));
            }
        }

        // Use all_chunks and all_chunk_ids for storage
        let texts = all_chunks;
        let ids = all_chunk_ids;

        if embeddings.len() != ids.len() {
            return Err(anyhow::anyhow!(
                "Embedding provider returned {} vectors for {} chunks",
                embeddings.len(),
                ids.len()
            ));
        }

        let dim = embeddings.first().map(|v| v.len()).unwrap_or(1536);

        // Flatten embeddings for Arrow
        let total_records = ids.len();
        let mut flatten_data = Vec::with_capacity(total_records * dim);
        for vec in &embeddings {
            if vec.len() != dim {
                return Err(anyhow::anyhow!(
                    "Embedding dimension mismatch. Expected {}, got {}",
                    dim,
                    vec.len()
                ));
            }
            flatten_data.extend_from_slice(vec);
        }

        // 2. Prepare Arrow Arrays
        let id_array = StringArray::from(ids);
        let text_array = StringArray::from(texts);
        let vector_data = Float32Array::from(flatten_data);

        let vector_array = FixedSizeListArray::new(
            Arc::new(Field::new("item", DataType::Float32, true)),
            dim as i32,
            Arc::new(vector_data),
            None,
        );

        // 3. Define Schema with DYNAMIC dimension
        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("text", DataType::Utf8, false),
            Field::new(
                "vector",
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, true)),
                    dim as i32,
                ),
                true,
            ),
        ]));

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(id_array),
                Arc::new(text_array),
                Arc::new(vector_array),
            ],
        )?;

        let batches = vec![batch];

        let table = self.conn.open_table(&self.table_name).execute().await;
        match table {
            Ok(t) => {
                t.add(batches).execute().await?;
            }
            Err(_) => {
                self.conn
                    .create_table(&self.table_name, batches)
                    .execute()
                    .await?;
            }
        };

        Ok(())
    }

    /// Performs semantic search and returns (id, text, distance).
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<(String, String, f32)>> {
        // 1. Embed Query
        let query_embedding = if let Some(fixture) = self.fixture.as_ref() {
            fixture.query_vector(&self.fixture_ns, query)?
        } else {
            let embedder = self.embedder.as_ref().ok_or_else(|| {
                anyhow::anyhow!("Embedder not initialized. Configure EMBEDDING_PROVIDER/EMBEDDING_HOST/EMBEDDING_MODEL and EMBEDDING_API_KEY (or OPENAI_API_KEY), or disable semantic search with CCM_DISABLE_EMBEDDER=1.")
            })?;
            let query_vecs = embedder.embed(vec![query.to_string()]).await?;
            match query_vecs.into_iter().next() {
                Some(vec) if !vec.is_empty() => vec,
                Some(_) => {
                    tracing::warn!("Embedder returned an empty query vector");
                    return Ok(vec![]);
                }
                None => {
                    tracing::warn!("Embedder returned no query vectors");
                    return Ok(vec![]);
                }
            }
        };

        // 2. Open Table
        let table = self
            .conn
            .open_table(&self.table_name)
            .execute()
            .await
            .with_context(|| {
                format!(
                    "Vector table '{}' is missing or unreadable; rebuild the CCM index",
                    self.table_name
                )
            })?;

        // 3. Vector Search
        let results = table
            .vector_search(query_embedding)?
            .limit(limit)
            .execute()
            .await?;

        // 4. Parse Results
        let mut hits = Vec::new();
        let batches: Vec<RecordBatch> = results.try_collect().await?;

        for batch in batches {
            let id_col = batch
                .column_by_name("id")
                .ok_or_else(|| anyhow::anyhow!("Missing 'id' column in search results"))?
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| anyhow::anyhow!("Failed to cast 'id' column to StringArray"))?;

            let text_col = batch
                .column_by_name("text")
                .ok_or_else(|| anyhow::anyhow!("Missing 'text' column in search results"))?
                .as_any()
                .downcast_ref::<StringArray>()
                .ok_or_else(|| anyhow::anyhow!("Failed to cast 'text' column to StringArray"))?;

            let dist_col = batch
                .column_by_name("_distance")
                .ok_or_else(|| anyhow::anyhow!("Missing '_distance' column in search results"))?
                .as_any()
                .downcast_ref::<Float32Array>()
                .ok_or_else(|| {
                    anyhow::anyhow!("Failed to cast '_distance' column to Float32Array")
                })?;

            for i in 0..batch.num_rows() {
                hits.push((
                    id_col.value(i).to_string(),
                    text_col.value(i).to_string(),
                    dist_col.value(i),
                ));
            }
        }

        // Eşit mesafeli hit'lerde LanceDB sırası garantili değildir; deterministik
        // top-k için (mesafe asc, id asc) sıralanır.
        hits.sort_by(|a, b| {
            a.2.partial_cmp(&b.2)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.0.cmp(&b.0))
        });

        Ok(hits)
    }

    /// Verilen dosyaya ait vektörleri siler (Garbage Collection).
    ///
    /// Dosya ID'sinin kendisini, ':' ile devam eden sembol node ID'lerini ve
    /// '#' ile devam eden chunk ID'lerini kapsar. Düz `starts_with(prefix)`
    /// kullanılmaz; çünkü `a.rs` için `a.rs.bak` gibi aynı öneki paylaşan
    /// kardeş dosyaların vektörlerini de siler (veri kaybı).
    pub async fn delete_by_prefix(&self, prefix: &str) -> Result<()> {
        let table = match self.conn.open_table(&self.table_name).execute().await {
            Ok(t) => t,
            Err(_) => return Ok(()), // Tablo yoksa silinecek bir şey de yok
        };

        let predicate = file_scoped_delete_predicate(prefix);

        if table.delete(&predicate).await.is_ok() {
            return Ok(());
        }

        // Fallback: lexicographical üst sınır kullanan range predicate'leri
        let range_predicate = file_scoped_delete_range_predicate(prefix);

        match table.delete(&range_predicate).await {
            Ok(_) => Ok(()),
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Failed to delete old vectors with range predicate"
                );
                Err(anyhow::anyhow!(
                    "Failed to delete vectors by prefix '{}': {}",
                    prefix,
                    e
                ))
            }
        }
    }
}

/// Yalnızca verilen dosyanın vektörlerini eşleyen silme predicate'i üretir:
/// tam dosya ID'si, ':' devamı (sembol node'ları) ve '#' devamı (chunk'lar).
fn file_scoped_delete_predicate(file_id: &str) -> String {
    let exact = escape_sql_literal(file_id);
    let colon = escape_sql_literal(&format!("{}:", file_id));
    let hash = escape_sql_literal(&format!("{}#", file_id));
    format!(
        "id = '{}' OR starts_with(id, '{}') OR starts_with(id, '{}')",
        exact, colon, hash
    )
}

/// `starts_with` desteklenmeyen backend'ler için range tabanlı eşdeğer predicate.
fn file_scoped_delete_range_predicate(file_id: &str) -> String {
    let mut clauses = vec![format!("id = '{}'", escape_sql_literal(file_id))];

    for separator in [':', '#'] {
        let lower = format!("{}{}", file_id, separator);
        let clause = match prefix_upper_bound(&lower) {
            Some(upper) => format!(
                "(id >= '{}' AND id < '{}')",
                escape_sql_literal(&lower),
                escape_sql_literal(&upper)
            ),
            None => format!("id >= '{}'", escape_sql_literal(&lower)),
        };
        clauses.push(clause);
    }

    clauses.join(" OR ")
}

fn escape_sql_literal(value: &str) -> String {
    value.replace('\'', "''")
}

fn prefix_upper_bound(prefix: &str) -> Option<String> {
    let mut chars: Vec<char> = prefix.chars().collect();
    while let Some(last) = chars.pop() {
        if last < '\u{10FFFF}' {
            if let Some(next) = char::from_u32(last as u32 + 1) {
                chars.push(next);
                return Some(chars.into_iter().collect());
            }
        }
    }
    None
}

pub(crate) fn semantic_chunks(text: &str, max_chars: usize, overlap: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut start = 0;

    while start < text.len() {
        let mut hard_end = (start + max_chars).min(text.len());
        while hard_end > start && !text.is_char_boundary(hard_end) {
            hard_end -= 1;
        }
        if hard_end == start {
            hard_end = text[start..]
                .char_indices()
                .nth(1)
                .map(|(offset, _)| start + offset)
                .unwrap_or(text.len());
        }

        let mut minimum_split = start + (hard_end - start) / 2;
        while minimum_split < hard_end && !text.is_char_boundary(minimum_split) {
            minimum_split += 1;
        }
        let preferred = text[minimum_split..hard_end]
            .rfind("\n\n")
            .map(|offset| minimum_split + offset + 2)
            .or_else(|| {
                text[minimum_split..hard_end]
                    .rfind('\n')
                    .map(|offset| minimum_split + offset + 1)
            });
        let end = preferred.filter(|end| *end > start).unwrap_or(hard_end);
        chunks.push(text[start..end].to_string());
        if end == text.len() {
            break;
        }

        let mut next = end.saturating_sub(overlap);
        while next < end && !text.is_char_boundary(next) {
            next += 1;
        }
        start = if next > start { next } else { end };
    }

    chunks
}

#[cfg(test)]
mod chunk_tests {
    use super::{namespace_for_uri, semantic_chunks};

    #[test]
    fn generation_database_keeps_the_project_fixture_namespace() {
        assert_eq!(
            namespace_for_uri("/work/repo_a/data/.ccm-generations/123/ccm_db"),
            "repo_a"
        );
        assert_eq!(
            namespace_for_uri("/work/repo_b/.ccm/.ccm-generations/456/ccm_db"),
            "repo_b"
        );
    }

    #[test]
    fn semantic_chunks_prefer_code_boundaries_and_preserve_progress() {
        let text = "fn one() {\n  work();\n}\n\nfn two() {\n  more();\n}\n";
        let chunks = semantic_chunks(text, 32, 4);
        assert!(chunks.len() >= 2);
        assert!(chunks[0].ends_with("\n\n"));
        assert!(chunks.iter().all(|chunk| chunk.len() <= 32));
    }

    #[test]
    fn semantic_chunks_never_slice_inside_utf8_characters() {
        let text = format!(
            "{}… Türkçe: ğüşiöç, Suomi: ääkköset, emoji: 🧠\n{}",
            "a".repeat(499),
            "b".repeat(700)
        );
        let chunks = semantic_chunks(&text, 500, 100);

        assert!(chunks.len() >= 3);
        assert!(chunks.iter().all(|chunk| chunk.len() <= 500));
        assert!(chunks.iter().any(|chunk| chunk.contains('…')));
        assert!(chunks.iter().any(|chunk| chunk.contains('🧠')));
    }

    #[test]
    fn semantic_chunks_make_progress_when_limit_is_smaller_than_a_character() {
        let chunks = semantic_chunks("🧠🧠", 1, 0);
        assert_eq!(chunks, vec!["🧠", "🧠"]);
    }
}
