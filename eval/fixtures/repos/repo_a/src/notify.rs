pub fn send_order_confirmation(order_id: u64, email: &str) -> bool {
    let subject = format!("Order {order_id} confirmed");
    dispatch(email, &subject)
}

pub fn send_stock_alert(sku: &str, location: &str) -> bool {
    let subject = format!("Low stock: {sku} at {location}");
    dispatch("ops@warehouse.local", &subject)
}

pub fn send_payment_failure(order_id: u64) -> bool {
    let subject = format!("Payment failed for order {order_id}");
    dispatch("finance@warehouse.local", &subject)
}

fn dispatch(to: &str, subject: &str) -> bool {
    !to.is_empty() && !subject.is_empty()
}
