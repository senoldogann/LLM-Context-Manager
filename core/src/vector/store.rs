use crate::vector::remote::RemoteEmbedder;
use anyhow::Result;
use arrow_array::{
    FixedSizeListArray, Float32Array, RecordBatch, RecordBatchIterator, StringArray,
};
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

        // Try to initialize Remote embedder from environment.
        // If it fails (no API key), we operate in "no-vector" mode or warn.
        let embedder = match RemoteEmbedder::from_env() {
            Ok(e) => Some(e),
            Err(e) => {
                eprintln!(
                    "Warning: Remote Embedder init failed: {}. Semantic search will be disabled.",
                    e
                );
                None
            }
        };

        Ok(Self {
            conn,
            table_name: table_name.to_string(),
            embedder,
        })
    }

    /// Embeds texts and inserts them into the LanceDB table.
    pub async fn add_documents(&self, ids: Vec<String>, texts: Vec<String>) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }

        let embedder = self
            .embedder
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Embedder not initialized"))?;

        const MAX_CHARS: usize = 1000;
        const OVERLAP: usize = 200;

        let mut all_chunks = Vec::new();
        let mut all_chunk_ids = Vec::new();
        let mut original_ids_map = Vec::new(); // Maps chunk index to original ID index

        for (i, text) in texts.iter().enumerate() {
            if text.len() <= MAX_CHARS {
                all_chunks.push(text.clone());
                all_chunk_ids.push(ids[i].clone());
                original_ids_map.push(i);
            } else {
                // Split large text into overlapping chunks
                let mut start = 0;
                let mut chunk_idx = 0;

                while start < text.len() {
                    // Find a valid char boundary for 'end'
                    let mut end = std::cmp::min(start + MAX_CHARS, text.len());
                    while !text.is_char_boundary(end) && end > start {
                        end -= 1;
                    }

                    // Safety check: if 'end' somehow equals 'start' (single huge char?), force forward to next valid
                    if end == start && start < text.len() {
                        if let Some((next_idx, _)) = text[start..].char_indices().nth(1) {
                            end = start + next_idx;
                        } else {
                            end = text.len();
                        }
                    }

                    let chunk = text[start..end].to_string();

                    all_chunks.push(chunk);
                    all_chunk_ids.push(format!("{}#chunk{}", ids[i], chunk_idx));
                    original_ids_map.push(i);

                    if end == text.len() {
                        break;
                    }

                    // Calculate next start with overlap, ensuring valid boundary
                    let next_target = start + MAX_CHARS - OVERLAP;
                    let mut next_start = std::cmp::min(next_target, text.len());
                    while !text.is_char_boundary(next_start) && next_start < text.len() {
                        next_start += 1;
                    }
                    // Ensure forward progress
                    if next_start <= start {
                        if let Some((idx, _)) = text[start..].char_indices().nth(1) {
                            start += idx;
                        } else {
                            start = text.len();
                        }
                    } else {
                        start = next_start;
                    }

                    chunk_idx += 1;
                }
            }
        }

        // 1. Generate Embeddings in batches for performance
        const BATCH_SIZE: usize = 32;
        let mut embeddings: Vec<Vec<f32>> = Vec::with_capacity(all_chunks.len());

        let total_batches = all_chunks.len().div_ceil(BATCH_SIZE);
        for (batch_idx, batch) in all_chunks.chunks(BATCH_SIZE).enumerate() {
            eprintln!(
                "Embedding batch {}/{} ({} chunks)",
                batch_idx + 1,
                total_batches,
                batch.len()
            );

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

        let batches = RecordBatchIterator::new(vec![Ok(batch)], schema.clone());

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
            .ok_or_else(|| anyhow::anyhow!("Embedder not initialized"))?;

        // 1. Embed Query
        let query_vecs = embedder.embed(vec![query.to_string()]).await?;
        let query_embedding = query_vecs[0].clone();

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
}
