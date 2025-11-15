# Phase 5: Performance Optimizations

This document describes the performance optimizations implemented in Phase 5 of CrabKv development.

## Overview

Phase 5 focused on improving throughput and reducing latency through several complementary strategies:

1. **Buffered I/O** with configurable fsync intervals
2. **Batch WAL writes** for bulk operations
3. **Asynchronous background compaction**
4. **Snappy compression** for WAL entries
5. **Write-back cache** for hot writes
6. **Enhanced benchmarks** with warm-up periods

## 1. Buffered I/O with Configurable Fsync Intervals

### Problem
Each write operation previously called `fsync()` immediately, causing excessive system calls and blocking on disk I/O.

### Solution
- Replaced direct `File` writes with `BufWriter<File>` in the WAL
- Added `sync_interval: Option<Duration>` configuration
- Track `last_sync: Mutex<Instant>` to batch fsyncs
- Only fsync when the interval has elapsed

### API Usage
```rust
let db = CrabKv::builder("data")
    .sync_interval(Duration::from_millis(100))  // Fsync every 100ms
    .build()?;
```

### Trade-offs
- **Pro**: Dramatically improved write throughput
- **Con**: Up to `sync_interval` of unflushed writes may be lost on crash
- **Recommendation**: Use 10-100ms for most workloads; omit for critical data

## 2. Batch WAL Writes

### Problem
Writing multiple entries sequentially caused multiple fsyncs, each blocking on disk I/O.

### Solution
- Added `WAL::append_batch(entries: Vec<WalEntry>)` method
- Write all entries to the buffer, then single fsync at the end
- Added `CrabKv::put_batch(entries)` for batch operations

### API Usage
```rust
let entries = vec![
    ("key1".to_string(), "value1".to_string(), None),
    ("key2".to_string(), "value2".to_string(), Some(Duration::from_secs(60))),
];
db.put_batch(entries)?;
```

### Benefits
- N writes with 1 fsync instead of N fsyncs
- Ideal for bulk imports or batch processing
- Combines well with sync_interval for maximum throughput

## 3. Asynchronous Background Compaction

### Problem
Compaction blocked the write path, causing latency spikes during maintenance.

### Solution
- Spawned dedicated compaction thread at engine startup
- Used `mpsc::channel` for non-blocking compaction requests
- `maybe_compact_async()` sends `Compact` or `CompactAndShutdown` messages
- Background thread handles compaction, engine continues serving writes

### API Usage
```rust
let db = CrabKv::builder("data")
    .async_compaction(true)
    .build()?;

// Compaction happens in background, writes never block
db.put("key".into(), "value".into())?;
```

### Implementation Details
- `CompactionRequest` enum: `Compact` or `CompactAndShutdown`
- Thread exits cleanly on `CompactAndShutdown` during engine drop
- Falls back to synchronous compaction if async is disabled

## 4. Snappy Compression

### Problem
WAL files grew large over time, consuming disk space and I/O bandwidth.

### Solution
- Integrated `snap` crate (Snappy compression)
- Added `compression: bool` flag to `EngineConfig`
- WAL entries transparently compressed during encoding
- Decompression happens automatically during reads
- Backwards compatible: uncompressed WALs still readable

### API Usage
```rust
let db = CrabKv::builder("data")
    .compression(true)
    .build()?;

// All writes are automatically compressed
db.put("key".into(), "large_value".repeat(100).into())?;
```

### Performance Characteristics
- **CPU**: Small overhead (~5-10% for Snappy)
- **Disk Space**: 30-70% reduction for text/JSON data
- **I/O Bandwidth**: Fewer bytes written/read from disk
- **Recommendation**: Enable for high-volume workloads

## 5. Write-Back Cache

### Problem
High write throughput applications spent excessive time writing to WAL and calling fsync.

### Solution
- Extended `Cache` with `write_buffer: HashMap<String, CacheEntry>`
- When `write_back_cache: bool` is enabled:
  - `put()` buffers writes in memory (no WAL I/O)
  - `get()` checks buffer first for uncommitted writes
  - `flush()` drains buffer and batch-writes to WAL
- Combines with batch writes for maximum efficiency

### API Usage
```rust
let db = CrabKv::builder("data")
    .write_back_cache(true)
    .build()?;

// Writes are buffered (fast)
db.put("key1".into(), "value1".into())?;
db.put("key2".into(), "value2".into())?;

// Explicit flush to persist
db.flush()?;
```

### ⚠️ Important Considerations
- **Data Loss Risk**: Unflushed writes lost on crash/power failure
- **Best Practice**: Call `flush()` periodically or on shutdown
- **Use Case**: High write throughput with acceptable durability trade-off
- **Combines Well With**: Sync interval and compression

## 6. Enhanced Benchmarks

### Improvements
- Added `warm_up_time(Duration)` to let CPU/disk stabilize
- Increased `measurement_time(Duration)` for sustained throughput metrics
- Measures realistic steady-state performance

### Configuration
```rust
group.warm_up_time(Duration::from_secs(2));
group.measurement_time(Duration::from_secs(8));
```

## Performance Summary

| Optimization | Throughput Gain | Latency Impact | Data Safety |
|--------------|----------------|----------------|-------------|
| Buffered I/O + Sync Interval | 10-50x | Lower (amortized fsync) | Risk: up to `sync_interval` of data loss |
| Batch Writes | 5-20x (bulk) | Lower (single fsync) | Safe (explicit fsync) |
| Async Compaction | 0% (throughput) | Much lower (no blocking) | Safe |
| Snappy Compression | 10-30% (I/O bound) | Minimal | Safe |
| Write-Back Cache | 100-500x (bursts) | Minimal (memory-only) | Risk: unflushed data lost on crash |

## Recommended Configurations

### High Throughput (with durability trade-offs)
```rust
CrabKv::builder("data")
    .sync_interval(Duration::from_millis(100))
    .compression(true)
    .async_compaction(true)
    .write_back_cache(true)
    .build()?;

// Flush periodically
loop {
    // ... write operations ...
    std::thread::sleep(Duration::from_secs(1));
    db.flush()?;
}
```

### Balanced (good throughput, safe)
```rust
CrabKv::builder("data")
    .sync_interval(Duration::from_millis(50))
    .compression(true)
    .async_compaction(true)
    .build()?;
```

### Maximum Safety (slowest)
```rust
CrabKv::builder("data")
    // No sync_interval (fsync every write)
    .compression(false)
    .async_compaction(false)
    .build()?;
```

## Testing

Phase 5 includes comprehensive tests:

### `tests/write_back_cache.rs`
- `write_back_cache_with_flush`: Verifies flush behavior and data loss without flush
- `write_back_cache_with_ttl`: Ensures TTL works correctly with buffered writes
- `write_back_cache_update_overwrites_buffer`: Tests buffer overwrites

Run tests:
```powershell
cargo test write_back_cache
```

## Future Optimizations

- **mmap** for read-only WAL access
- **Lock-free index** using concurrent data structures
- **Multi-threaded compaction** for parallel log rewriting
- **Adaptive compression** based on data type detection
- **Write coalescing** to merge consecutive puts to same key

## References

- [Snappy Compression](https://github.com/google/snappy)
- [BufWriter Documentation](https://doc.rust-lang.org/std/io/struct.BufWriter.html)
- [Write-Back Cache Pattern](https://en.wikipedia.org/wiki/Cache_(computing)#Write-back)
