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

        const MAX_CHARS: usize = 1500;
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
                    let end = std::cmp::min(start + MAX_CHARS, text.len());
                    let chunk = text[start..end].to_string();

                    all_chunks.push(chunk);
                    all_chunk_ids.push(format!("{}#chunk{}", ids[i], chunk_idx));
                    original_ids_map.push(i);

                    if end == text.len() {
                        break;
                    }
                    start += MAX_CHARS - OVERLAP;
                    chunk_idx += 1;
                }
            }
        }

        // 1. Generate Embeddings one at a time
        let mut embeddings: Vec<Vec<f32>> = Vec::with_capacity(all_chunks.len());
        for (i, chunk) in all_chunks.iter().enumerate() {
            eprintln!("Embedding chunk {}/{}", i + 1, all_chunks.len());
            let single_embedding = embedder.embed(vec![chunk.clone()]).await?;
            if let Some(emb) = single_embedding.into_iter().next() {
                embeddings.push(emb);
            }
        }

        // Use all_chunks and all_chunk_ids for storage
        let texts = all_chunks;
        let ids = all_chunk_ids;

        let dim = embeddings.first().map(|v| v.len()).unwrap_or(1536);

        // Flatten embeddings for Arrow
        let total_records = ids.len();
        let mut flatten_data = Vec::with_capacity(total_records * dim);
        for vec in &embeddings {
            // Resize if dimension mismatch (Ollama might vary?)
            // Just pushing for now, assuming consistency.
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

    /// Performs semantic search and returns (text, distance).
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<(String, f32)>> {
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
            let text_col = batch
                .column_by_name("text")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            let dist_col = batch
                .column_by_name("_distance")
                .unwrap()
                .as_any()
                .downcast_ref::<Float32Array>()
                .unwrap();

            for i in 0..batch.num_rows() {
                hits.push((text_col.value(i).to_string(), dist_col.value(i)));
            }
        }

        Ok(hits)
    }
}
