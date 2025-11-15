//! Configuration helpers for CrabKv.

use std::num::NonZeroUsize;
use std::time::Duration;

/// Tunable parameters for the storage engine.
#[derive(Clone, Debug)]
pub struct EngineConfig {
    /// Maximum number of cached entries kept in memory.
    /// When absent, caching is disabled.
    pub cache_capacity: Option<NonZeroUsize>,
    /// Default time-to-live applied to writes when not explicitly provided.
    pub default_ttl: Option<Duration>,
    /// Interval between WAL syncs; None means sync on every write.
    pub sync_interval: Option<Duration>,
    /// Whether to compress values with Snappy before writing to WAL.
    pub compression: bool,
    /// Whether to enable write-back caching.
    pub write_back_cache: bool,
}

impl EngineConfig {
    /// Returns a configuration with caching disabled and no default TTL.
    pub fn new(
        cache_capacity: Option<NonZeroUsize>,
        default_ttl: Option<Duration>,
        sync_interval: Option<Duration>,
        compression: bool,
        write_back_cache: bool,
    ) -> Self {
        Self {
            cache_capacity,
            default_ttl,
            sync_interval,
            compression,
            write_back_cache,
        }
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            cache_capacity: None,
            default_ttl: None,
            sync_interval: None,
            compression: false,
            write_back_cache: false,
        }
    }
}
