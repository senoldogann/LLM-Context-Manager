use crate::availability::{DateRange, is_available};
use crate::store::Connection;

pub struct Room {
    pub number: u32,
    pub floor: u8,
    pub capacity: u8,
}

pub fn pick_room(conn: &Connection, dates: DateRange) -> Option<Room> {
    let rooms = conn.list_rooms();
    rooms
        .into_iter()
        .find(|room| is_available(conn, room.number, &dates))
}

pub fn find_room_by_number(conn: &Connection, number: u32) -> Option<Room> {
    conn.list_rooms().into_iter().find(|room| room.number == number)
}

pub fn rooms_on_floor(conn: &Connection, floor: u8) -> Vec<Room> {
    conn.list_rooms()
        .into_iter()
        .filter(|room| room.floor == floor)
        .collect()
}

impl Default for Room {
    fn default() -> Self {
        Self {
            number: 0,
            floor: 0,
            capacity: 2,
        }
    }
}
