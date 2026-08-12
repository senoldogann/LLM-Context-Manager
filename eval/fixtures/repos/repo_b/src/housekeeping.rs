use crate::rooms::Room;
use crate::store::Connection;

pub fn rooms_needing_cleaning(conn: &Connection) -> Vec<Room> {
    conn.list_rooms()
        .into_iter()
        .filter(|room| room.floor % 2 == 0)
        .collect()
}

pub fn assign_cleaner(room: &Room) -> String {
    format!("cleaner-floor-{}", room.floor)
}

pub fn mark_clean(conn: &Connection, room_number: u32) {
    conn.mark_room_clean(room_number);
}
