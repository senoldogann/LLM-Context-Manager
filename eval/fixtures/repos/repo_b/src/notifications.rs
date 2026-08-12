use crate::customer::Customer;

pub fn send_booking_confirmation(customer: &Customer, total_cents: u64) -> bool {
    !customer.email.is_empty() && total_cents > 0
}

pub fn send_reminder(email: &str, night: u32) -> bool {
    !email.is_empty() && night > 0
}

pub fn send_checkout_receipt(email: &str) -> bool {
    !email.is_empty()
}
