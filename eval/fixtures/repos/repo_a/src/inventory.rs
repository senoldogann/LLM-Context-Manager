use crate::db::Connection;

pub struct InventoryItem {
    pub sku: String,
    pub location: String,
    pub quantity: u32,
}

pub fn reserve_stock(
    conn: &Connection,
    sku: &str,
    quantity: u32,
) -> Result<(), StockError> {
    let item = lookup_item(conn, sku)?;
    if item.quantity < quantity {
        return Err(StockError::Insufficient {
            sku: sku.into(),
            available: item.quantity,
        });
    }
    conn.update_quantity(sku, item.quantity - quantity);
    Ok(())
}

pub fn lookup_item(conn: &Connection, sku: &str) -> Result<InventoryItem, StockError> {
    conn.find_item(sku).ok_or(StockError::Missing(sku.into()))
}

pub fn restock(conn: &Connection, sku: &str, quantity: u32) {
    let item = conn.find_item(sku);
    let current = item.map(|entry| entry.quantity).unwrap_or(0);
    conn.update_quantity(sku, current + quantity);
}

pub enum StockError {
    Missing(String),
    Insufficient { sku: String, available: u32 },
}
