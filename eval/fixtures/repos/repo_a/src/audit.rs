use crate::db::Connection;
use crate::order::Order;

pub fn log_order_event(conn: &Connection, order_id: u64, event: &str) {
    conn.insert_audit(order_id, event);
}

pub fn recent_events(conn: &Connection, order_id: u64, limit: u32) -> Vec<String> {
    conn.audit_events(order_id, limit)
}

pub fn summarize_events(events: &[String]) -> String {
    format!("{} events recorded", events.len())
}

impl Order {
    pub fn audit_label(&self) -> String {
        format!("order-{}", self.id)
    }
}
