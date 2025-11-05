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
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};

/// Concurrent key-value store with append-only persistence.
#[derive(Clone)]
pub struct CrabKv {
    inner: Arc<RwLock<EngineState>>,
    config: EngineConfig,
}

/// Builder used to configure the storage engine before opening it.
#[derive(Clone, Debug)]
pub struct CrabKvBuilder {
    directory: PathBuf,
    cache_capacity: Option<NonZeroUsize>,
    default_ttl: Option<Duration>,
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

        Self::maybe_compact(&mut state)
    }

    /// Returns the value stored for the key if present and not expired.
    pub fn get(&self, key: &str) -> io::Result<Option<String>> {
        {
            let state = self
                .inner
                .read()
                .map_err(|_| io::Error::new(ErrorKind::Other, "engine poisoned"))?;

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

        Self::maybe_compact(&mut state)
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

    fn maybe_compact(state: &mut EngineState) -> io::Result<()> {
        if compaction::should_compact(state.total_bytes, state.stale_bytes) {
            Self::run_compaction(state)
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

    /// Builds the engine, loading the WAL contents into memory.
    pub fn build(self) -> io::Result<CrabKv> {
        std::fs::create_dir_all(&self.directory)?;
        let wal_path = self.directory.join("wal.log");
        let wal = Wal::open(&wal_path)?;
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
        let cache = self.cache_capacity.map(Cache::new);
        let config = EngineConfig {
            cache_capacity: self.cache_capacity,
            default_ttl: self.default_ttl,
        };

        Ok(CrabKv {
            inner: Arc::new(RwLock::new(EngineState {
                index,
                wal,
                cache,
                stale_bytes,
                total_bytes,
            })),
            config,
        })
    }
}
