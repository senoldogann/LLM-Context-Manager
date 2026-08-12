use crate::availability::DateRange;
use crate::booking::Booking;
use crate::customer::Customer;
use crate::rooms::Room;

pub struct Connection {
    path: String,
}

impl Connection {
    pub fn connect(path: &str) -> Connection {
        Connection {
            path: path.to_string(),
        }
    }

    pub fn list_rooms(&self) -> Vec<Room> {
        let _ = self;
        Vec::new()
    }

    pub fn list_bookings(&self, room_number: u32) -> Vec<DateRange> {
        let _ = (self, room_number);
        Vec::new()
    }

    pub fn insert_booking(
        &self,
        customer_id: u64,
        room_number: u32,
        from: u32,
        to: u32,
        total_cents: u64,
    ) -> Booking {
        let _ = (
            self,
            customer_id,
            room_number,
            from,
            to,
            total_cents,
        );
        Booking {
            customer_id,
            room_number,
            total_cents,
            state: crate::booking::BookingState::Confirmed,
        }
    }

    pub fn remove_booking(&self, customer_id: u64, room_number: u32) -> bool {
        let _ = (self, customer_id, room_number);
        true
    }

    pub fn find_customer(&self, email: &str) -> Option<Customer> {
        let _ = (self, email);
        None
    }

    pub fn next_customer_id(&self) -> u64 {
        let _ = self;
        1
    }

    pub fn mark_room_clean(&self, room_number: u32) {
        let _ = (self, room_number);
    }

    pub fn booking_count(&self) -> u64 {
        let _ = self;
        0
    }

    pub fn booking_total_cents(&self) -> u64 {
        let _ = self;
        0
    }
}
