use crate::customer::Customer;
use crate::receipt::PaymentMethod;

pub fn charge(customer: &Customer, amount_cents: u64) -> bool {
    amount_cents > 0 && !customer.email.is_empty()
}

pub fn refund(customer: &Customer, amount_cents: u64) -> bool {
    charge(customer, amount_cents)
}

pub fn choose_method(amount_cents: u64) -> PaymentMethod {
    if amount_cents > 500_000 {
        PaymentMethod::Card
    } else {
        PaymentMethod::Wallet
    }
}
