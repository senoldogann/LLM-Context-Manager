use crate::store::Connection;

pub struct Customer {
    pub id: u64,
    pub email: String,
    pub loyalty_points: u64,
}

pub fn find_or_create(conn: &Connection, email: &str) -> Customer {
    conn.find_customer(email)
        .unwrap_or_else(|| Customer {
            id: conn.next_customer_id(),
            email: email.to_string(),
            loyalty_points: 0,
        })
}

pub fn award_points(customer: &mut Customer, amount_cents: u64) {
    customer.loyalty_points += amount_cents / 100;
}

pub fn is_member(customer: &Customer) -> bool {
    customer.loyalty_points > 0
}
