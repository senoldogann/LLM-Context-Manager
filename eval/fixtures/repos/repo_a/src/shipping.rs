use crate::order::Order;

pub fn schedule(order: &Order, total: u64) -> ShippingLabel {
    let method = pick_method(total);
    ShippingLabel {
        order_id: order.id,
        method,
    }
}

pub fn pick_method(total: u64) -> ShippingMethod {
    if total > 100_000 {
        ShippingMethod::Express
    } else {
        ShippingMethod::Standard
    }
}

pub fn estimate_days(method: &ShippingMethod) -> u32 {
    match method {
        ShippingMethod::Standard => 5,
        ShippingMethod::Express => 2,
    }
}

pub enum ShippingMethod {
    Standard,
    Express,
}

pub struct ShippingLabel {
    pub order_id: u64,
    pub method: ShippingMethod,
}
