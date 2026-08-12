use crate::booking::Booking;
use crate::store::Connection;

pub fn occupancy_report(conn: &Connection) -> String {
    format!("{} bookings this week", conn.booking_count())
}

pub fn revenue_report(conn: &Connection) -> u64 {
    conn.booking_total_cents()
}

pub fn render_occupancy(report: &str) -> String {
    format!("Occupancy: {report}")
}
