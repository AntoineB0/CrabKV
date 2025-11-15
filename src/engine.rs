//! High-level storage engine orchestrating the in-memory index and WAL.

use crate::cache::{Cache, CacheEntry};
use crate::compaction;
use crate::config::EngineConfig;
use crate::index::ValuePointer;
use crate::wal::{Wal, WalEntry};
use std::collections::HashMap;
use std::io::{self, ErrorKind};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, RwLock};
use std::thread;
use std::time::{Duration, SystemTime};

/// Concurrent key-value store with append-only persistence.
#[derive(Clone)]
pub struct CrabKv {
    inner: Arc<RwLock<EngineState>>,
    config: EngineConfig,
    compaction_tx: Option<Sender<CompactionRequest>>,
}

enum CompactionRequest {
    Trigger,
    #[allow(dead_code)]
    Shutdown,
}

/// Builder used to configure the storage engine before opening it.
#[derive(Clone, Debug)]
pub struct CrabKvBuilder {
    directory: PathBuf,
    cache_capacity: Option<NonZeroUsize>,
    default_ttl: Option<Duration>,
    sync_interval: Option<Duration>,
    async_compaction: bool,
    compression: bool,
    write_back_cache: bool,
}

#[derive(Clone, Debug)]
struct IndexEntry {
    pointer: ValuePointer,
    expires_at: Option<SystemTime>,
}

struct EngineState {
    index: HashMap<String, IndexEntry>,
    wal: Wal,
    cache: Option<Cache>,
    stale_bytes: u64,
    total_bytes: u64,
}

impl CrabKv {
    /// Opens the engine inside the provided directory with default configuration.
    pub fn open(directory: impl AsRef<Path>) -> io::Result<Self> {
        CrabKvBuilder::new(directory).build()
    }

    /// Returns a builder to customize caching and TTL behaviour.
    pub fn builder(directory: impl AsRef<Path>) -> CrabKvBuilder {
        CrabKvBuilder::new(directory)
    }

    /// Flushes write-back cache entries to the WAL if enabled.
    pub fn flush(&self) -> io::Result<()> {
        if !self.config.write_back_cache {
            return Ok(());
        }

        let mut state = self
            .inner
            .write()
            .map_err(|_| io::Error::new(ErrorKind::Other, "engine poisoned"))?;

        let cache = match &state.cache {
            Some(cache) => cache,
            None => return Ok(()),
        };

        let buffered = cache.flush_write_buffer();
        if buffered.is_empty() {
            return Ok(());
        }

        let entries: Vec<_> = buffered
            .into_iter()
            .map(|(key, entry)| (key, entry.value, None))
            .collect();

        let wal_entries: Vec<WalEntry> = entries
            .iter()
            .map(|(key, value, ttl)| {
                let expires_at = ttl.and_then(|duration| SystemTime::now().checked_add(duration));
                WalEntry::Put {
                    key: key.clone(),
                    value: value.clone(),
                    expires_at,
                }
            })
            .collect();

        let pointers = state.wal.append_batch(&wal_entries)?;

        for (i, (key, _, _)) in entries.into_iter().enumerate() {
            let pointer = pointers[i];
            state.total_bytes += pointer.record_len as u64;
            if let Some(previous) = state.index.get(&key) {
                state.stale_bytes += previous.pointer.record_len as u64;
            }
        }

        Ok(())
    }

    /// Stores or updates a value, applying the default TTL when configured.
    pub fn put(&self, key: String, value: String) -> io::Result<()> {
        let ttl = self.config.default_ttl;
        self.put_with_ttl(key, value, ttl)
    }

    /// Stores or updates a value using the provided TTL.
    pub fn put_with_ttl(
        &self,
        key: String,
        value: String,
        ttl: Option<Duration>,
    ) -> io::Result<()> {
        let expires_at = ttl.and_then(|duration| SystemTime::now().checked_add(duration));

        // If write-back cache is enabled, buffer in memory
        if self.config.write_back_cache {
            if let Ok(state) = self.inner.read() {
                if let Some(cache) = &state.cache {
                    cache.put(key, CacheEntry { value, expires_at });
                    return Ok(());
                }
            }
        }

        let mut state = self
            .inner
            .write()
            .map_err(|_| io::Error::new(ErrorKind::Other, "engine poisoned"))?;
        let entry = WalEntry::Put {
            key: key.clone(),
            value: value.clone(),
            expires_at,
        };
        let pointer = state.wal.append(&entry)?;
        state.total_bytes += pointer.record_len as u64;

        if let Some(previous) = state.index.insert(
            key.clone(),
            IndexEntry {
                pointer,
                expires_at,
            },
        ) {
            state.stale_bytes += previous.pointer.record_len as u64;
        }

        if let Some(cache) = &state.cache {
            cache.put(key, CacheEntry { value, expires_at });
        }

        self.maybe_compact_async(&mut state)
    }

    /// Stores multiple key-value pairs in a single batch for improved throughput.
    pub fn put_batch(&self, entries: Vec<(String, String, Option<Duration>)>) -> io::Result<()> {
        if entries.is_empty() {
            return Ok(());
        }

        let mut state = self
            .inner
            .write()
            .map_err(|_| io::Error::new(ErrorKind::Other, "engine poisoned"))?;

        let wal_entries: Vec<WalEntry> = entries
            .iter()
            .map(|(key, value, ttl)| {
                let expires_at = ttl.and_then(|duration| SystemTime::now().checked_add(duration));
                WalEntry::Put {
                    key: key.clone(),
                    value: value.clone(),
                    expires_at,
                }
            })
            .collect();

        let pointers = state.wal.append_batch(&wal_entries)?;

        for (i, (key, value, ttl)) in entries.into_iter().enumerate() {
            let pointer = pointers[i];
            let expires_at = ttl.and_then(|duration| SystemTime::now().checked_add(duration));
            state.total_bytes += pointer.record_len as u64;

            if let Some(previous) = state.index.insert(
                key.clone(),
                IndexEntry {
                    pointer,
                    expires_at,
                },
            ) {
                state.stale_bytes += previous.pointer.record_len as u64;
            }

            if let Some(cache) = &state.cache {
                cache.put(key, CacheEntry { value, expires_at });
            }
        }

        self.maybe_compact_async(&mut state)
    }

    /// Returns the value stored for the key if present and not expired.
    pub fn get(&self, key: &str) -> io::Result<Option<String>> {
        {
            let state = self
                .inner
                .read()
                .map_err(|_| io::Error::new(ErrorKind::Other, "engine poisoned"))?;

            // With write-back cache, check cache first (may contain uncommitted writes)
            if self.config.write_back_cache {
                if let Some(cache) = &state.cache {
                    if let Some(hit) = cache.get(key) {
                        if !Self::is_expired(hit.expires_at) {
                            return Ok(Some(hit.value));
                        } else {
                            // Expired in cache
                            return Ok(None);
                        }
                    }
                }
            }

            if let Some(entry) = state.index.get(key) {
                if Self::is_expired(entry.expires_at) {
                    drop(state);
                    return self.expire_key(key);
                }

                if let Some(cache) = &state.cache {
                    if let Some(hit) = cache.get(key) {
                        if !Self::is_expired(hit.expires_at) {
                            return Ok(Some(hit.value));
                        }
                    }
                }

                let record = state.wal.read_record(entry.pointer)?;
                if let WalEntry::Put { value, .. } = record.entry {
                    if let Some(cache) = &state.cache {
                        cache.put(
                            key.to_owned(),
                            CacheEntry {
                                value: value.clone(),
                                expires_at: entry.expires_at,
                            },
                        );
                    }
                    return Ok(Some(value));
                }
            }
        }

        Ok(None)
    }

    /// Removes the key if present.
    pub fn delete(&self, key: &str) -> io::Result<()> {
        let mut state = self
            .inner
            .write()
            .map_err(|_| io::Error::new(ErrorKind::Other, "engine poisoned"))?;

        let entry = WalEntry::Delete {
            key: key.to_owned(),
        };
        let pointer = state.wal.append(&entry)?;
        state.total_bytes += pointer.record_len as u64;

        if let Some(previous) = state.index.remove(key) {
            state.stale_bytes += previous.pointer.record_len as u64;
        }

        if let Some(cache) = &state.cache {
            cache.remove(key);
        }

        self.maybe_compact_async(&mut state)
    }

    /// Forces a compaction cycle regardless of the current heuristic.
    pub fn compact(&self) -> io::Result<()> {
        let mut state = self
            .inner
            .write()
            .map_err(|_| io::Error::new(ErrorKind::Other, "engine poisoned"))?;
        Self::run_compaction(&mut state)
    }

    fn expire_key(&self, key: &str) -> io::Result<Option<String>> {
        let mut state = self
            .inner
            .write()
            .map_err(|_| io::Error::new(ErrorKind::Other, "engine poisoned"))?;

        if let Some(entry) = state.index.remove(key) {
            state.stale_bytes += entry.pointer.record_len as u64;
            if let Some(cache) = &state.cache {
                cache.remove(key);
            }
            let delete = WalEntry::Delete {
                key: key.to_owned(),
            };
            let pointer = state.wal.append(&delete)?;
            state.total_bytes += pointer.record_len as u64;
        }

        Ok(None)
    }

    #[allow(dead_code)]
    fn maybe_compact(state: &mut EngineState) -> io::Result<()> {
        if compaction::should_compact(state.total_bytes, state.stale_bytes) {
            Self::run_compaction(state)
        } else {
            Ok(())
        }
    }

    fn maybe_compact_async(&self, state: &mut EngineState) -> io::Result<()> {
        if compaction::should_compact(state.total_bytes, state.stale_bytes) {
            if let Some(tx) = &self.compaction_tx {
                let _ = tx.send(CompactionRequest::Trigger);
                Ok(())
            } else {
                Self::run_compaction(state)
            }
        } else {
            Ok(())
        }
    }

    fn run_compaction(state: &mut EngineState) -> io::Result<()> {
        let mut entries = Vec::with_capacity(state.index.len());
        let now = SystemTime::now();
        let mut expired = Vec::new();

        for (key, entry) in state.index.iter() {
            if Self::is_expired_at(entry.expires_at, now) {
                expired.push(key.clone());
                continue;
            }
            let record = state.wal.read_record(entry.pointer)?;
            if let WalEntry::Put {
                value, expires_at, ..
            } = record.entry
            {
                entries.push((key.clone(), value, expires_at));
            }
        }

        for key in expired {
            state.index.remove(&key);
            if let Some(cache) = &state.cache {
                cache.remove(&key);
            }
        }

        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let rebuilt = state.wal.rewrite(&entries)?;
        state.index = rebuilt
            .into_iter()
            .map(|(key, (pointer, expires_at))| {
                (
                    key,
                    IndexEntry {
                        pointer,
                        expires_at,
                    },
                )
            })
            .collect();
        state.total_bytes = state.wal.size()?;
        state.stale_bytes = 0;
        Ok(())
    }

    fn is_expired(expires_at: Option<SystemTime>) -> bool {
        Self::is_expired_at(expires_at, SystemTime::now())
    }

    fn is_expired_at(expires_at: Option<SystemTime>, now: SystemTime) -> bool {
        matches!(expires_at, Some(deadline) if now >= deadline)
    }
}

impl CrabKvBuilder {
    /// Creates a builder rooted at the provided directory with caching disabled.
    pub fn new(directory: impl AsRef<Path>) -> Self {
        Self {
            directory: directory.as_ref().to_path_buf(),
            cache_capacity: None,
            default_ttl: None,
            sync_interval: None,
            async_compaction: false,
            compression: false,
            write_back_cache: false,
        }
    }

    /// Enables an LRU cache sized by the provided entry count.
    pub fn cache_capacity(mut self, capacity: NonZeroUsize) -> Self {
        self.cache_capacity = Some(capacity);
        self
    }

    /// Applies a default TTL to future writes.
    pub fn default_ttl(mut self, ttl: Duration) -> Self {
        self.default_ttl = Some(ttl);
        self
    }

    /// Sets a sync interval for periodic WAL flushes instead of fsyncing every write.
    pub fn sync_interval(mut self, interval: Duration) -> Self {
        self.sync_interval = Some(interval);
        self
    }

    /// Enables background compaction in a dedicated thread.
    pub fn async_compaction(mut self, enabled: bool) -> Self {
        self.async_compaction = enabled;
        self
    }

    /// Enables Snappy compression for values written to the WAL.
    pub fn compression(mut self, enabled: bool) -> Self {
        self.compression = enabled;
        self
    }

    /// Enables write-back caching mode that buffers writes in memory before flushing.
    pub fn write_back_cache(mut self, enabled: bool) -> Self {
        self.write_back_cache = enabled;
        self
    }

    /// Builds the engine, loading the WAL contents into memory.
    pub fn build(self) -> io::Result<CrabKv> {
        std::fs::create_dir_all(&self.directory)?;
        let wal_path = self.directory.join("wal.log");
        let wal = Wal::open(&wal_path, self.sync_interval, self.compression)?;
        let (raw_index, stale_bytes) = wal.load_index()?;
        let index = raw_index
            .into_iter()
            .map(|(key, (pointer, expires_at))| {
                (
                    key,
                    IndexEntry {
                        pointer,
                        expires_at,
                    },
                )
            })
            .collect();
        let total_bytes = wal.size()?;
        let cache = if let Some(capacity) = self.cache_capacity {
            Some(Cache::with_write_back(capacity, self.write_back_cache))
        } else {
            None
        };
        let config = EngineConfig {
            cache_capacity: self.cache_capacity,
            default_ttl: self.default_ttl,
            sync_interval: self.sync_interval,
            compression: self.compression,
            write_back_cache: self.write_back_cache,
        };

        let inner = Arc::new(RwLock::new(EngineState {
            index,
            wal,
            cache,
            stale_bytes,
            total_bytes,
        }));

        let compaction_tx = if self.async_compaction {
            let (tx, rx) = mpsc::channel::<CompactionRequest>();
            let inner_clone = Arc::clone(&inner);
            thread::spawn(move || {
                for req in rx {
                    match req {
                        CompactionRequest::Trigger => {
                            if let Ok(mut state) = inner_clone.write() {
                                let _ = CrabKv::run_compaction(&mut state);
                            }
                        }
                        CompactionRequest::Shutdown => break,
                    }
                }
            });
            Some(tx)
        } else {
            None
        };

        Ok(CrabKv {
            inner,
            config,
            compaction_tx,
        })
    }
}
