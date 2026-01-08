use anyhow::Result;
use lancedb::{connect, Connection};

pub struct LanceDbStore {
    #[allow(dead_code)]
    conn: Connection,
    #[allow(dead_code)]
    table_name: String,
}

impl LanceDbStore {
    pub async fn new(uri: &str, table_name: &str) -> Result<Self> {
        let conn = connect(uri).execute().await?;
        Ok(Self {
            conn,
            table_name: table_name.to_string(),
        })
    }

    pub async fn init_table(&self, _dim: i32) -> Result<()> {
        // Define Schema: id (Utf8), content (Utf8), vector (FixedSizeList<Float32>)
        // Note: Simplification for prototype. Real implementation needs robust schema handling.
        // In LanceDB 0.23, we often CREATE table with initial data or empty schema.
        Ok(())
    }

    // Placeholder for actual insertion logic which requires Arrow array construction.
    // For now, we want to ensure it compiles with lancedb types.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_connection() {
        let _store = LanceDbStore::new("data/test_db", "test_table")
            .await
            .unwrap();
    }
}
