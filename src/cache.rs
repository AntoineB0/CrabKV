//! Optional in-memory cache for CrabKv lookups.

use lru::LruCache;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::time::SystemTime;

/// Shared cache handle wrapping an `LruCache` guarded by a mutex.
#[derive(Clone, Debug)]
pub struct Cache {
    inner: Arc<Mutex<LruCache<String, CacheEntry>>>,
    write_buffer: Arc<Mutex<HashMap<String, CacheEntry>>>,
    write_back: bool,
}

impl Cache {
    /// Constructs a cache with the provided capacity. When capacity is zero, caching is disabled.
    pub fn new(capacity: NonZeroUsize) -> Self {
        Self::with_write_back(capacity, false)
    }

    /// Constructs a cache with optional write-back mode.
    pub fn with_write_back(capacity: NonZeroUsize, write_back: bool) -> Self {
        Self {
            inner: Arc::new(Mutex::new(LruCache::new(capacity))),
            write_buffer: Arc::new(Mutex::new(HashMap::new())),
            write_back,
        }
    }

    /// Returns the cached entry if present, checking write buffer first.
    pub fn get(&self, key: &str) -> Option<CacheEntry> {
        if self.write_back {
            let buffer = self.write_buffer.lock();
            if let Some(entry) = buffer.get(key) {
                return Some(entry.clone());
            }
        }
        let mut guard = self.inner.lock();
        guard.get(key).cloned()
    }

    /// Inserts or updates the cached entry, buffering if write-back is enabled.
    pub fn put(&self, key: String, entry: CacheEntry) {
        if self.write_back {
            let mut buffer = self.write_buffer.lock();
            buffer.insert(key.clone(), entry.clone());
        }
        let mut guard = self.inner.lock();
        guard.put(key, entry);
    }

    /// Evicts the provided key from the cache and write buffer.
    pub fn remove(&self, key: &str) {
        if self.write_back {
            let mut buffer = self.write_buffer.lock();
            buffer.remove(key);
        }
        let mut guard = self.inner.lock();
        guard.pop(key);
    }

    /// Flushes and clears the write buffer, returning buffered entries for WAL persistence.
    pub fn flush_write_buffer(&self) -> Vec<(String, CacheEntry)> {
        if !self.write_back {
            return Vec::new();
        }
        let mut buffer = self.write_buffer.lock();
        let entries = buffer.drain().collect();
        entries
    }
}

/// Value stored in the cache, keeping the decoded payload and optional expiration.
#[derive(Clone, Debug)]
pub struct CacheEntry {
    pub value: String,
    pub expires_at: Option<SystemTime>,
}
