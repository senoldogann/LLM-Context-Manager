pub struct AppConfig {
    pub warehouse_name: String,
    pub currency: String,
    pub max_orders_per_batch: u32,
}

pub fn default_config() -> AppConfig {
    AppConfig {
        warehouse_name: "main".to_string(),
        currency: "USD".to_string(),
        max_orders_per_batch: 200,
    }
}

pub fn currency_symbol(currency: &str) -> &'static str {
    match currency {
        "USD" => "$",
        "EUR" => "€",
        _ => "?",
    }
}

pub fn with_batch_limit(config: AppConfig, limit: u32) -> AppConfig {
    AppConfig {
        max_orders_per_batch: limit,
        ..config
    }
}
