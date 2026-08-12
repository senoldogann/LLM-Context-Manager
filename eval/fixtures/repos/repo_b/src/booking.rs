use crate::availability::DateRange;
use crate::customer::Customer;
use crate::rooms::Room;
use crate::store::Connection;

pub struct Booking {
    pub customer_id: u64,
    pub room_number: u32,
    pub total_cents: u64,
    pub state: BookingState,
}

pub enum BookingState {
    Confirmed,
    CheckedIn,
    CheckedOut,
    Cancelled,
}

pub fn create(
    conn: &Connection,
    customer: &Customer,
    room: Room,
    dates: &DateRange,
    total_cents: u64,
) -> Booking {
    conn.insert_booking(customer.id, room.number, dates.from, dates.to, total_cents)
}

pub fn cancel(conn: &Connection, customer_id: u64, room_number: u32) -> bool {
    conn.remove_booking(customer_id, room_number)
}

pub fn check_in(booking: &mut Booking) {
    booking.state = BookingState::CheckedIn;
}
