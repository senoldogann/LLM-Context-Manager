use crate::vector::remote::RemoteEmbedder;
use anyhow::Result;
use arrow_array::{FixedSizeListArray, Float32Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::{connect, Connection};
use std::sync::Arc;

pub struct LanceDbStore {
    conn: Connection,
    table_name: String,
    embedder: Option<RemoteEmbedder>,
}

impl LanceDbStore {
    pub async fn new(uri: &str, table_name: &str) -> Result<Self> {
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

        Ok(Self {
            conn,
            table_name: table_name.to_string(),
            embedder,
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
            if let Err(e) = self.conn.drop_table(&self.table_name, &[]).await {
                tracing::warn!(
                    table = %self.table_name,
                    error = %e,
                    "Failed to drop table"
                );
            }
        }

        Ok(())
    }

    /// Embeds texts and inserts them into the LanceDB table.
    pub async fn add_documents(&self, ids: Vec<String>, texts: Vec<String>) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }

        let embedder = match self.embedder.as_ref() {
            Some(e) => e,
            None => {
                // Determine if we should warn or just skip
                // Ideally, we just skip vector indexing if semantic search is disabled.
                return Ok(());
            }
        };

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
        let mut original_ids_map = Vec::new(); // Maps chunk index to original ID index

        for (i, text) in texts.iter().enumerate() {
            if text.len() <= max_chars {
                all_chunks.push(text.clone());
                all_chunk_ids.push(ids[i].clone());
                original_ids_map.push(i);
            } else {
                for (chunk_idx, chunk) in semantic_chunks(text, max_chars, overlap)
                    .into_iter()
                    .enumerate()
                {
                    all_chunks.push(chunk);
                    all_chunk_ids.push(format!("{}#chunk{}", ids[i], chunk_idx));
                    original_ids_map.push(i);
                }
            }
        }

        // 1. Generate Embeddings in batches for performance
        let batch_size: usize = std::env::var("CCM_EMBED_BATCH_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(32)
            .max(1); // guard: chunks(0) panics at runtime
        let mut embeddings: Vec<Vec<f32>> = Vec::with_capacity(all_chunks.len());

        let total_batches = all_chunks.len().div_ceil(batch_size);
        for (batch_idx, batch) in all_chunks.chunks(batch_size).enumerate() {
            if batch_idx % 20 == 0 || (batch_idx + 1) == total_batches {
                tracing::info!(
                    batch = batch_idx + 1,
                    total = total_batches,
                    chunks = batch.len(),
                    "Embedding batch progress"
                );
            }

            let batch_texts: Vec<String> = batch.to_vec();
            let batch_embeddings = embedder.embed(batch_texts).await?;
            embeddings.extend(batch_embeddings);
        }

        // Use all_chunks and all_chunk_ids for storage
        let texts = all_chunks;
        let ids = all_chunk_ids;

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
        let embedder = self
            .embedder
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Embedder not initialized. Configure EMBEDDING_PROVIDER/EMBEDDING_HOST/EMBEDDING_MODEL and EMBEDDING_API_KEY (or OPENAI_API_KEY), or disable semantic search with CCM_DISABLE_EMBEDDER=1."))?;

        // 1. Embed Query
        let query_vecs = embedder.embed(vec![query.to_string()]).await?;
        let query_embedding = match query_vecs.into_iter().next() {
            Some(vec) if !vec.is_empty() => vec,
            Some(_) => {
                tracing::warn!("Embedder returned an empty query vector");
                return Ok(vec![]);
            }
            None => {
                tracing::warn!("Embedder returned no query vectors");
                return Ok(vec![]);
            }
        };

        // 2. Open Table
        let table = match self.conn.open_table(&self.table_name).execute().await {
            Ok(t) => t,
            Err(_) => return Ok(vec![]),
        };

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

        Ok(hits)
    }

    /// Deletes vectors where the ID starts with the given prefix.
    /// This is used for Garbage Collection when re-indexing a file.
    pub async fn delete_by_prefix(&self, prefix: &str) -> Result<()> {
        let table = match self.conn.open_table(&self.table_name).execute().await {
            Ok(t) => t,
            Err(_) => return Ok(()), // Table doesn't exist, nothing to delete
        };

        let escaped_lower = prefix.replace('\'', "''");
        let predicate = format!("starts_with(id, '{}')", escaped_lower);

        if let Ok(_) = table.delete(&predicate).await {
            return Ok(());
        }

        // Fallback: range predicate using lexicographical upper bound
        let range_predicate = if let Some(upper) = prefix_upper_bound(prefix) {
            let escaped_upper = upper.replace('\'', "''");
            format!("id >= '{}' AND id < '{}'", escaped_lower, escaped_upper)
        } else {
            format!("id >= '{}'", escaped_lower)
        };

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

fn semantic_chunks(text: &str, max_chars: usize, overlap: usize) -> Vec<String> {
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
    use super::semantic_chunks;

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
