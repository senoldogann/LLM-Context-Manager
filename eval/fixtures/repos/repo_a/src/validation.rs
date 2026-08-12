use crate::order::Order;

pub fn validate_order(order: &Order) -> Vec<String> {
    let mut errors = Vec::new();
    if order.items.is_empty() {
        errors.push("empty order".to_string());
    }
    errors
}

pub fn validate_sku_format(sku: &str) -> bool {
    crate::utils::is_valid_sku(sku)
}

pub fn require_positive_quantity(quantity: u32) -> Result<(), String> {
    if quantity == 0 {
        Err("quantity must be positive".to_string())
    } else {
        Ok(())
    }
}
