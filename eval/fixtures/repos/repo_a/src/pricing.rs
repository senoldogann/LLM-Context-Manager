use crate::db::Connection;
use crate::order::Order;
use crate::tax::compute_tax;

pub fn compute_total(order: &Order) -> u64 {
    let subtotal = crate::order::order_subtotal(order);
    subtotal + compute_tax(subtotal)
}

pub fn apply_discount(total: u64, percent: u32) -> u64 {
    total * (100 - percent.min(100)) / 100
}

pub fn price_for_customer(conn: &Connection, order: &Order, customer_id: u64) -> u64 {
    let base = compute_total(order);
    let tier = conn.customer_tier(customer_id);
    match tier {
        crate::db::CustomerTier::Premium => apply_discount(base, 10),
        crate::db::CustomerTier::Regular => base,
    }
}
