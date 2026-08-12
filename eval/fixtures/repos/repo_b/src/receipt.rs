use crate::customer::Customer;
use crate::rooms::Room;

pub enum PaymentMethod {
    Card,
    Wallet,
}

pub fn generate(customer: &Customer, room: Room, total_cents: u64) -> String {
    format!(
        "Receipt for {} room {}: {} cents",
        customer.email, room.number, total_cents
    )
}

pub fn render_html(receipt: &str) -> String {
    format!("<html><body>{receipt}</body></html>")
}

pub fn render_text(receipt: &str) -> String {
    receipt.to_string()
}
