pub fn sku_normalize(raw: &str) -> String {
    raw.trim().to_ascii_uppercase()
}

pub fn cents_to_display(cents: u64) -> String {
    format!("{:.2}", cents as f64 / 100.0)
}

pub fn is_valid_sku(sku: &str) -> bool {
    !sku.is_empty() && sku.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

pub fn clamp_quantity(value: u32, max: u32) -> u32 {
    value.min(max)
}
