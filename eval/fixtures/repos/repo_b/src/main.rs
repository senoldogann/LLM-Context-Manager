mod availability;
mod booking;
mod customer;
mod housekeeping;
mod notifications;
mod payment;
mod rates;
mod receipt;
mod reports;
mod rooms;
mod store;
mod utils;

use std::process::ExitCode;

fn main() -> ExitCode {
    let db = store::connect("hotel.db");
    let customer = customer::find_or_create(&db, "guest@example.com");
    let dates = availability::DateRange { from: 5, to: 7 };
    let room = rooms::pick_room(&db, dates).unwrap_or_default();
    let total = rates::compute_rate(room, &dates);
    if payment::charge(&customer, total) {
        booking::create(&db, &customer, room, &dates, total);
        receipt::generate(&customer, room, total);
    }
    ExitCode::SUCCESS
}
