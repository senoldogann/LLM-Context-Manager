use crate::store::Connection;

pub struct DateRange {
    pub from: u32,
    pub to: u32,
}

impl DateRange {
    pub fn nights(&self) -> u64 {
        self.to.saturating_sub(self.from).max(1) as u64
    }

    pub fn overlaps(&self, other: &DateRange) -> bool {
        self.from < other.to && other.from < self.to
    }
}

pub fn is_available(conn: &Connection, room_number: u32, dates: &DateRange) -> bool {
    !conn
        .list_bookings(room_number)
        .iter()
        .any(|booking| booking.overlaps(dates))
}

pub fn next_free_night(conn: &Connection, room_number: u32, from: u32) -> u32 {
    let mut candidate = from;
    while !is_available(
        conn,
        room_number,
        &DateRange {
            from: candidate,
            to: candidate + 1,
        },
    ) {
        candidate += 1;
    }
    candidate
}
