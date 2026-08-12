pub fn normalize_email(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

pub fn cents_to_display(cents: u64) -> String {
    format!("{:.2}", cents as f64 / 100.0)
}

pub fn is_valid_email(email: &str) -> bool {
    email.contains('@') && email.contains('.')
}

pub fn clamp_nights(value: u32, max: u32) -> u32 {
    value.min(max)
}
