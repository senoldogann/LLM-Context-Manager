use crate::availability::DateRange;
use crate::rooms::Room;

pub fn compute_rate(room: Room, dates: &DateRange) -> u64 {
    let nights = dates.nights();
    let base = 12_000 + room.capacity as u64 * 2_500;
    base * nights
}

pub fn apply_weekend_surcharge(total: u64, weekend_nights: u32) -> u64 {
    total + weekend_nights as u64 * 1_500
}

pub fn compute_rate_with_discount(room: Room, dates: &DateRange, percent: u32) -> u64 {
    let base = compute_rate(room, dates);
    base * (100 - percent.min(100)) / 100
}
