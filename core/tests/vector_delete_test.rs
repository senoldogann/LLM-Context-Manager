use anyhow::Result;
use arrow_array::{
    FixedSizeListArray, Float32Array, RecordBatch, RecordBatchIterator, StringArray,
};
use arrow_schema::{DataType, Field, Schema};
use ccm_core::vector::store::LanceDbStore;
use futures::TryStreamExt;
use lancedb::connect;
use lancedb::query::ExecutableQuery;
use std::sync::Arc;
use std::sync::Once;
use tempfile::tempdir;

static INIT: Once = Once::new();

fn setup_test_env() {
    INIT.call_once(|| {
        std::env::set_var("CCM_DISABLE_EMBEDDER", "1");
    });
}

#[tokio::test]
async fn test_delete_by_prefix_removes_rows() -> Result<()> {
    setup_test_env();

    let dir = tempdir()?;
    let db_path = dir.path().to_string_lossy().to_string();

    let conn = connect(&db_path).execute().await?;

    let ids = vec![
        "./src/a.rs:func:1:0".to_string(),
        "./src/a.rs:func:2:0".to_string(),
        "./src/b.rs:func:1:0".to_string(),
    ];
    let texts = vec!["a1".to_string(), "a2".to_string(), "b1".to_string()];

    let dim = 3;
    let vectors = vec![
        vec![0.1_f32, 0.2, 0.3],
        vec![0.4_f32, 0.5, 0.6],
        vec![0.7_f32, 0.8, 0.9],
    ];

    let mut flattened = Vec::with_capacity(ids.len() * dim);
    for v in vectors {
        flattened.extend_from_slice(&v);
    }

    let id_array = StringArray::from(ids);
    let text_array = StringArray::from(texts);
    let vector_data = Float32Array::from(flattened);
    let vector_array = FixedSizeListArray::new(
        Arc::new(Field::new("item", DataType::Float32, true)),
        dim as i32,
        Arc::new(vector_data),
        None,
    );

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
    conn.create_table("code_vectors", batches).execute().await?;

    let table = conn.open_table("code_vectors").execute().await?;
    let initial_count = table.count_rows(None).await?;
    assert_eq!(initial_count, 3);

    let store = LanceDbStore::new(&db_path, "code_vectors").await?;
    store.delete_by_prefix("./src/a.rs").await?;

    let table = conn.open_table("code_vectors").execute().await?;
    let remaining = table.count_rows(None).await?;
    assert_eq!(remaining, 1);

    let stream = table.query().execute().await?;
    let batches: Vec<RecordBatch> = stream.try_collect().await?;
    let mut remaining_ids = Vec::new();
    for batch in batches {
        let id_col = batch
            .column_by_name("id")
            .expect("missing id column")
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("id column not a StringArray");
        for i in 0..batch.num_rows() {
            remaining_ids.push(id_col.value(i).to_string());
        }
    }

    assert_eq!(remaining_ids.len(), 1);
    assert_eq!(remaining_ids[0], "./src/b.rs:func:1:0");

    Ok(())
}
