//! High-level storage engine orchestrating the in-memory index and WAL.

use crate::compaction;
use crate::index::ValuePointer;
use crate::wal::{Wal, WalEntry};
use std::collections::HashMap;
use std::io::{self, ErrorKind};
use std::path::Path;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Concurrent key-value store with append-only persistence.
#[derive(Clone)]
pub struct CrabKv {
    inner: Arc<RwLock<EngineState>>,
}
struct EngineState {
    index: HashMap<String, ValuePointer>,
    wal: Wal,
    stale_bytes: u64,
    total_bytes: u64,
}

impl CrabKv {
    /// Opens the engine inside the provided directory, replaying the log if needed.
    pub fn open(directory: impl AsRef<Path>) -> io::Result<Self> {
        let directory = directory.as_ref().to_path_buf();
        std::fs::create_dir_all(&directory)?;
        let wal_path = directory.join("wal.log");
        let wal = Wal::open(&wal_path)?;
        let (index, stale_bytes) = wal.load_index()?;
        let total_bytes = wal.size()?;

        Ok(Self {
            inner: Arc::new(RwLock::new(EngineState {
                index,
                wal,
                stale_bytes,
                total_bytes,
            })),
        })
    }

    /// Stores or updates a value.
    pub fn put(&self, key: String, value: String) -> io::Result<()> {
        let entry = WalEntry::Put {
            key: key.clone(),
            value,
        };
        let mut state = self.write_lock()?;
        let pointer = state.wal.append(&entry)?;
        state.total_bytes += pointer.record_len as u64;
        if let Some(previous) = state.index.insert(key, pointer) {
            state.stale_bytes += previous.record_len as u64;
        }
        Self::maybe_compact(&mut state)
    }

    /// Returns the value stored for the key.
    pub fn get(&self, key: &str) -> io::Result<Option<String>> {
        let state = self.read_lock()?;
        match state.index.get(key) {
            Some(pointer) => state.wal.read_value(*pointer).map(Some),
            None => Ok(None),
        }
    }

    /// Removes the key if present.
    pub fn delete(&self, key: &str) -> io::Result<()> {
        let entry = WalEntry::Delete {
            key: key.to_owned(),
        };
        let mut state = self.write_lock()?;
        let pointer = state.wal.append(&entry)?;
        state.total_bytes += pointer.record_len as u64;
        if let Some(previous) = state.index.remove(key) {
            state.stale_bytes += previous.record_len as u64;
        }
        Self::maybe_compact(&mut state)
    }

    /// Forces a compaction cycle regardless of the current heuristic.
    pub fn compact(&self) -> io::Result<()> {
        let mut state = self.write_lock()?;
        Self::run_compaction(&mut state)
    }

    fn maybe_compact(state: &mut RwLockWriteGuard<'_, EngineState>) -> io::Result<()> {
        if compaction::should_compact(state.total_bytes, state.stale_bytes) {
            Self::run_compaction(state)
        } else {
            Ok(())
        }
    }

    fn run_compaction(state: &mut RwLockWriteGuard<'_, EngineState>) -> io::Result<()> {
        let mut entries = Vec::with_capacity(state.index.len());
        for (key, pointer) in state.index.iter() {
            let value = state.wal.read_value(*pointer)?;
            entries.push((key.clone(), value));
        }
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        let rebuilt = state.wal.rewrite(&entries)?;
        state.index = rebuilt;
        state.total_bytes = state.wal.size()?;
        state.stale_bytes = 0;
        Ok(())
    }

    fn read_lock(&self) -> io::Result<RwLockReadGuard<'_, EngineState>> {
        self.inner
            .read()
            .map_err(|_| io::Error::new(ErrorKind::Other, "engine poisoned"))
    }

    fn write_lock(&self) -> io::Result<RwLockWriteGuard<'_, EngineState>> {
        self.inner
            .write()
            .map_err(|_| io::Error::new(ErrorKind::Other, "engine poisoned"))
    }
}
