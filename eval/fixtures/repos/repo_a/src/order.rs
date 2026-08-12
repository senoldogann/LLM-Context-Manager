use crate::db::Connection;

pub struct Order {
    pub id: u64,
    pub customer: String,
    pub items: Vec<OrderLine>,
    pub status: OrderStatus,
}

pub struct OrderLine {
    pub sku: String,
    pub quantity: u32,
    pub unit_price_cents: u64,
}

pub enum OrderStatus {
    New,
    Picked,
    Shipped,
    Cancelled,
}

pub fn load_open_orders(conn: &Connection) -> Vec<Order> {
    conn.query_orders("status != 'shipped'")
}

pub fn mark_shipped(order: &mut Order) {
    order.status = OrderStatus::Shipped;
}

pub fn order_subtotal(order: &Order) -> u64 {
    order
        .items
        .iter()
        .map(|line| line.unit_price_cents * line.quantity as u64)
        .sum()
}
