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
}

impl EngineConfig {
    /// Returns a configuration with caching disabled and no default TTL.
    pub fn new(cache_capacity: Option<NonZeroUsize>, default_ttl: Option<Duration>) -> Self {
        Self {
            cache_capacity,
            default_ttl,
        }
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            cache_capacity: None,
            default_ttl: None,
        }
    }
}
