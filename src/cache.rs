//! Optional in-memory cache for CrabKv lookups.

use lru::LruCache;
use parking_lot::Mutex;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::SystemTime;

/// Shared cache handle wrapping an `LruCache` guarded by a mutex.
#[derive(Clone, Debug)]
pub struct Cache {
    inner: Arc<Mutex<LruCache<String, CacheEntry>>>,
}

impl Cache {
    /// Constructs a cache with the provided capacity. When capacity is zero, caching is disabled.
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(LruCache::new(capacity))),
        }
    }

    /// Returns the cached entry if present.
    pub fn get(&self, key: &str) -> Option<CacheEntry> {
        let mut guard = self.inner.lock();
        guard.get(key).cloned()
    }

    /// Inserts or updates the cached entry.
    pub fn put(&self, key: String, entry: CacheEntry) {
        let mut guard = self.inner.lock();
        guard.put(key, entry);
    }

    /// Evicts the provided key from the cache.
    pub fn remove(&self, key: &str) {
        let mut guard = self.inner.lock();
        guard.pop(key);
    }
}

/// Value stored in the cache, keeping the decoded payload and optional expiration.
#[derive(Clone, Debug)]
pub struct CacheEntry {
    pub value: String,
    pub expires_at: Option<SystemTime>,
}
