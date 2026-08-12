//! Deterministik pseudo-random üretici (SplitMix64).
//!
//! Yeni dependency eklemeden reproducible deneyler için sabit seed'li RNG sağlar.
//! Aynı seed her zaman aynı diziyi üretir; testler ve fixture üretimi buna dayanır.

#[derive(Debug, Clone)]
pub struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// `[0, bound)` aralığında deterministik indeks döndürür.
    pub fn next_below(&mut self, bound: usize) -> usize {
        assert!(bound > 0, "SplitMix64::next_below bound must be positive");
        (self.next_u64() % bound as u64) as usize
    }

    /// Fisher-Yates karıştırma — aynı seed her zaman aynı sırayı üretir.
    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        for i in (1..items.len()).rev() {
            let j = self.next_below(i + 1);
            items.swap(i, j);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::SplitMix64;

    #[test]
    fn same_seed_produces_same_sequence() {
        let mut a = SplitMix64::new(42);
        let mut b = SplitMix64::new(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn different_seed_produces_different_sequence() {
        let mut a = SplitMix64::new(42);
        let mut b = SplitMix64::new(43);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn shuffle_is_deterministic() {
        let mut items_a = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let mut items_b = items_a.clone();
        SplitMix64::new(7).shuffle(&mut items_a);
        SplitMix64::new(7).shuffle(&mut items_b);
        assert_eq!(items_a, items_b);
    }

    #[test]
    fn shuffle_keeps_all_items() {
        let mut items = vec![1, 2, 3, 4, 5, 6, 7, 8];
        SplitMix64::new(9).shuffle(&mut items);
        let mut sorted = items.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, vec![1, 2, 3, 4, 5, 6, 7, 8]);
    }
}
