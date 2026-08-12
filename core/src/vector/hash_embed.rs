//! Deterministik, ağ bağlantısız karakter-trigram hash embedding üretici.
//!
//! Yeni dependency eklemeden offline hybrid evaluation için kararlı vektörler
//! üretir: metinler küçük harfe çevrilip karakter trigramlarına ayrılır, her trigram FNV-1a
//! hash ile sabit boyutlu bir vektöre işaretli olarak eklenir ve L2 normalize
//! edilir. Ortak trigramlar daha yüksek kosinüs benzerliği üretir; aynı giriş
//! her zaman aynı vektörü verir.

use anyhow::Result;

pub const HASH_EMBED_DIM: usize = 64;

#[derive(Debug, Clone)]
pub struct HashEmbedder {
    pub dim: usize,
}

impl Default for HashEmbedder {
    fn default() -> Self {
        Self {
            dim: HASH_EMBED_DIM,
        }
    }
}

impl HashEmbedder {
    pub fn new(dim: usize) -> Self {
        Self { dim }
    }

    pub fn embed(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        Ok(texts
            .into_iter()
            .map(|text| embed_text(&text, self.dim))
            .collect())
    }
}

/// Metni deterministik trigram-hash vektöre çevirir.
pub fn embed_text(text: &str, dim: usize) -> Vec<f32> {
    let mut vector = vec![0.0f32; dim.max(1)];
    for trigram in trigrams(text) {
        let hash = fnv1a(trigram.as_bytes());
        let index = (hash % dim as u64) as usize;
        let sign = if hash & (1 << 63) == 0 { -1.0 } else { 1.0 };
        vector[index] += sign;
    }
    l2_normalize(&mut vector);
    vector
}

/// Küçük harfe çevrilmiş metnin karakter trigramları.
fn trigrams(text: &str) -> Vec<String> {
    let lowered = text.to_ascii_lowercase();
    let chars: Vec<char> = lowered.chars().collect();
    if chars.len() < 3 {
        return vec![lowered];
    }
    chars
        .windows(3)
        .map(|window| window.iter().collect())
        .collect()
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in bytes {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn l2_normalize(vector: &mut [f32]) {
    let norm: f32 = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > f32::EPSILON {
        for value in vector.iter_mut() {
            *value /= norm;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{embed_text, HASH_EMBED_DIM};

    fn cosine(a: &[f32], b: &[f32]) -> f32 {
        let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        dot
    }

    #[test]
    fn embedding_is_deterministic() {
        let a = embed_text("compute_tax implementation", HASH_EMBED_DIM);
        let b = embed_text("compute_tax implementation", HASH_EMBED_DIM);
        assert_eq!(a, b);
    }

    #[test]
    fn embedding_is_l2_normalized() {
        let vector = embed_text("compute_tax implementation", HASH_EMBED_DIM);
        let norm: f32 = vector.iter().map(|v| v * v).sum();
        assert!((norm - 1.0).abs() < 1e-4);
    }

    #[test]
    fn shared_trigrams_rank_higher_than_unrelated() {
        let query = embed_text("find where compute_tax is implemented", HASH_EMBED_DIM);
        let related = embed_text(
            "function compute_tax\nfile: ./src/tax.rs\npub fn compute_tax",
            HASH_EMBED_DIM,
        );
        let unrelated = embed_text(
            "function ship_order\nfile: ./src/shipping.rs\npub fn ship_order",
            HASH_EMBED_DIM,
        );
        assert!(
            cosine(&query, &related) > cosine(&query, &unrelated),
            "ortak trigramlı metin ilgisiz metinden yüksek benzerlik almalı"
        );
    }
}
