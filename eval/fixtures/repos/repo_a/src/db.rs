use crate::inventory::InventoryItem;
use crate::order::Order;

pub struct Connection {
    path: String,
}

pub struct SalesRow {
    pub amount_cents: u64,
}

pub enum CustomerTier {
    Regular,
    Premium,
}

impl Connection {
    pub fn connect(path: &str) -> Connection {
        Connection {
            path: path.to_string(),
        }
    }

    pub fn query_orders(&self, filter: &str) -> Vec<Order> {
        let _ = (self, filter);
        Vec::new()
    }

    pub fn find_item(&self, sku: &str) -> Option<InventoryItem> {
        let _ = (self, sku);
        None
    }

    pub fn update_quantity(&self, sku: &str, quantity: u32) {
        let _ = (self, sku, quantity);
    }

    pub fn query_sales(&self) -> Vec<SalesRow> {
        let _ = self;
        Vec::new()
    }

    pub fn customer_tier(&self, customer_id: u64) -> CustomerTier {
        let _ = (self, customer_id);
        CustomerTier::Regular
    }

    pub fn insert_audit(&self, order_id: u64, event: &str) {
        let _ = (self, order_id, event);
    }

    pub fn audit_events(&self, order_id: u64, limit: u32) -> Vec<String> {
        let _ = (self, order_id, limit);
        Vec::new()
    }
}
