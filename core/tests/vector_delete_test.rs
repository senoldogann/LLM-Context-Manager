use anyhow::Result;
use arrow_array::{FixedSizeListArray, Float32Array, RecordBatch, StringArray};
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

fn build_test_batch(ids: Vec<String>, texts: Vec<String>) -> Result<RecordBatch> {
    let dim = 3;
    let mut flattened = Vec::with_capacity(ids.len() * dim);
    for i in 0..ids.len() {
        flattened.extend_from_slice(&[0.1_f32 * (i as f32 + 1.0), 0.2, 0.3]);
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

    Ok(RecordBatch::try_new(
        schema.clone(),
        vec![
            Arc::new(id_array),
            Arc::new(text_array),
            Arc::new(vector_array),
        ],
    )?)
}

async fn collect_ids(conn: &lancedb::Connection, table_name: &str) -> Result<Vec<String>> {
    let table = conn.open_table(table_name).execute().await?;
    let stream = table.query().execute().await?;
    let batches: Vec<RecordBatch> = stream.try_collect().await?;
    let mut ids = Vec::new();
    for batch in batches {
        let id_col = batch
            .column_by_name("id")
            .expect("missing id column")
            .as_any()
            .downcast_ref::<StringArray>()
            .expect("id column not a StringArray");
        for i in 0..batch.num_rows() {
            ids.push(id_col.value(i).to_string());
        }
    }
    Ok(ids)
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

    let batch = build_test_batch(ids, texts)?;
    conn.create_table("code_vectors", vec![batch])
        .execute()
        .await?;

    let table = conn.open_table("code_vectors").execute().await?;
    let initial_count = table.count_rows(None).await?;
    assert_eq!(initial_count, 3);

    let store = LanceDbStore::new(&db_path, "code_vectors").await?;
    store.delete_by_prefix("./src/a.rs").await?;

    let remaining_ids = collect_ids(&conn, "code_vectors").await?;
    assert_eq!(remaining_ids, vec!["./src/b.rs:func:1:0"]);

    Ok(())
}

#[tokio::test]
async fn test_delete_by_prefix_keeps_sibling_files_sharing_name_prefix() -> Result<()> {
    setup_test_env();

    let dir = tempdir()?;
    let db_path = dir.path().to_string_lossy().to_string();

    let conn = connect(&db_path).execute().await?;

    // "./src/a.rs" ile aynı öneki paylaşan kardeş dosya ID'leri:
    // düz starts_with silme bunları yanlışlıkla yok ederdi.
    let ids = vec![
        "./src/a.rs:function:symbol:0000000000000001:0".to_string(),
        "./src/a.rs#chunk0".to_string(),
        "./src/a.rs".to_string(),
        "./src/a.rs.bak:function:symbol:0000000000000002:0".to_string(),
        "./src/a.rs.old#chunk0".to_string(),
    ];
    let texts = vec![
        "a symbol".to_string(),
        "a chunk".to_string(),
        "a data file".to_string(),
        "bak symbol".to_string(),
        "old chunk".to_string(),
    ];

    let batch = build_test_batch(ids, texts)?;
    conn.create_table("code_vectors", vec![batch])
        .execute()
        .await?;

    let store = LanceDbStore::new(&db_path, "code_vectors").await?;
    store.delete_by_prefix("./src/a.rs").await?;

    let mut remaining_ids = collect_ids(&conn, "code_vectors").await?;
    remaining_ids.sort();

    assert_eq!(
        remaining_ids,
        vec![
            "./src/a.rs.bak:function:symbol:0000000000000002:0",
            "./src/a.rs.old#chunk0",
        ]
    );

    Ok(())
}
