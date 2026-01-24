use lru::LruCache;
use std::num::NonZeroUsize;
use std::sync::Mutex;

/// A thread-safe LRU cache for speculative retrieval.
/// Stores the neighbors (adjacency list) of a node to avoid disk hits.
pub struct SpeculativeCache {
    cache: Mutex<LruCache<String, Vec<String>>>,
}

impl SpeculativeCache {
    pub fn new(capacity: usize) -> Self {
        let cap = NonZeroUsize::new(capacity).expect("Capacity must be > 0");
        Self {
            cache: Mutex::new(LruCache::new(cap)),
        }
    }

    /// Caches the neighbors for a given node.
    pub fn put(&self, node_id: String, neighbors: Vec<String>) {
        if let Ok(mut cache) = self.cache.lock() {
            cache.put(node_id, neighbors);
        }
    }

    /// Retrieves neighbors from cache.
    pub fn get(&self, node_id: &str) -> Option<Vec<String>> {
        if let Ok(mut cache) = self.cache.lock() {
            cache.get(node_id).cloned()
        } else {
            None
        }
    }
}
