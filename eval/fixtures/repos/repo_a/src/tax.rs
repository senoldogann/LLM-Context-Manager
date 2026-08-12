pub const TAX_RATE_BPS: u64 = 2_000;

pub fn compute_tax(subtotal: u64) -> u64 {
    subtotal * TAX_RATE_BPS / 10_000
}

pub fn tax_rate_label() -> &'static str {
    "standard"
}

pub fn compute_tax_for_region(subtotal: u64, region: &str) -> u64 {
    match region {
        "exempt" => 0,
        _ => compute_tax(subtotal),
    }
}
