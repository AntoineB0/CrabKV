```
   ___           _     _  __      
  / __\ __ __ _ | |__ | |/ /_   __
 / / | '__/ _` || '_ \| ' /\ \ / /
/ /__| | | (_| || |_) | . \ \ V / 
\____/_|  \__,_||_.__/|_|\_\ \_/  
```

**CrabKv** is a lightweight Rust key-value store inspired by LevelDB. The engine keeps writes durable via an append-only log, rebuilds its in-memory index on restart, and now ships with caching, TTL, and a small TCP front-end for remote access.

## Performance Overview

Phase 5 optimizations deliver dramatic throughput improvements:

| Configuration | Throughput (writes/sec) | Speedup vs Baseline |
|--------------|------------------------|---------------------|
|  Baseline (no optimizations) | 759 | 1x |
|  Buffered I/O (100ms sync) | 115,516 | **152x**  |
|  Batch Writes (100/batch) | 75,875 | **100x**  |
|  Write-Back Cache | 68,374 | **90x**  |
|  **All Optimizations Combined** | **220,970** | **291x**  |

> Run `cargo run --example phase5_demo` to see these optimizations in action!

## Features

- Append-only write-ahead log with crash-safe rewrites on Windows and Unix.
- In-memory index mapping keys to WAL offsets for O(1) lookups.
- Optional LRU cache to keep hot keys resident without extra plumbing.
- Per-write TTL metadata encoded in the WAL and honored during reads and compaction.
- Manual or automatic compaction once stale bytes cross configurable heuristics.
- **Buffered I/O with configurable fsync intervals** for reduced system call overhead.
- **Batch WAL writes** allowing multiple entries with a single fsync.
- **Asynchronous background compaction** to avoid blocking the write path.
- **Optional Snappy compression** for WAL entries (transparent encode/decode).
- **Write-back cache** for hot writes, reducing WAL I/O via explicit flush batching.
- Ergonomic CLI plus a text-based TCP server for experimentation.
- Criterion benchmarks and end-to-end tests covering persistence and TTL expiry.

## Project Structure

```
src/
  main.rs        # CLI entry point and TCP server wiring
  lib.rs         # Library facade exporting CrabKv and CrabKvBuilder
  engine.rs      # Store orchestration (index + WAL + cache + compaction)
  wal.rs         # Binary log encoder/decoder with TTL-aware headers
  index.rs       # ValuePointer describing WAL offsets
  compaction.rs  # Heuristics + rewriting logic
  cache.rs       # Optional LRU cache wrapper
  config.rs      # User-facing configuration types
  server.rs      # Minimal TCP server handling text commands

tests/
  basic.rs       # Persistence, overwrite, and TTL expiration checks

benches/
  engine.rs      # Criterion micro-benchmarks
```

## Getting Started

### Prerequisites

- Rust 1.79+ (edition 2024)
- On Windows prefer the MSVC toolchain (`rustup default stable-x86_64-pc-windows-msvc`) so `cargo test` and Criterion can link.

### Build & Format

```powershell
cargo fmt
cargo check
```

### CLI Experiments

```powershell
# Basic CRUD
cargo run -- put hello world
cargo run -- get hello

# Write with a 30s TTL
cargo run -- put --ttl 30 ephemeral value

# Housekeeping
cargo run -- delete hello
cargo run -- compact
```

Change the data directory or defaults via environment variables:

```powershell
$env:CRABKV_DATA_DIR = "D:/storage/crabkv"
$env:CRABKV_CACHE_CAPACITY = "2048"
$env:CRABKV_DEFAULT_TTL_SECS = "60"
cargo run -- get hello
```

### Testing & Benchmarks

```powershell
cargo test       # Requires the MSVC toolchain on Windows; MinGW lacks dlltool
cargo bench
cargo run --example perf -- 50000 64  # Ad-hoc microbench (ops, value_size_bytes)
```

## TCP Server

Start the server and connect with `nc`, `telnet`, or any TCP client:

```powershell
# Terminal 1
cargo run -- serve --addr 127.0.0.1:4000 --cache 4096 --default-ttl 120

# Terminal 2
nc 127.0.0.1 4000
PUT demo value ttl=45
GET demo
DELETE demo
COMPACT
```

The server speaks a simple, line-oriented protocol. Type `HELP` to list supported commands.

## Configuration Cheatsheet

| Env Var                     | CLI Flag (serve)      | Description                                  |
|-----------------------------|-----------------------|----------------------------------------------|
| `CRABKV_DATA_DIR`           | —                     | Directory where WAL and metadata live.       |
| `CRABKV_CACHE_CAPACITY`     | `--cache <entries>`   | Enables the LRU cache with the provided size.|
| `CRABKV_DEFAULT_TTL_SECS`   | `--default-ttl <s>`   | Applies a TTL to writes that omit `--ttl`.   |

Individual `put` commands can also set `--ttl <seconds>` without touching defaults.

## Library Usage

```rust
use crabkv::CrabKv;
use std::num::NonZeroUsize;
use std::time::Duration;

fn open_engine() -> std::io::Result<CrabKv> {
    CrabKv::builder("data")
        .cache_capacity(NonZeroUsize::new(2048).unwrap())
        .default_ttl(Duration::from_secs(30))
        .sync_interval(Duration::from_millis(100))  // Fsync every 100ms instead of per-write
        .compression(true)                           // Enable Snappy compression
        .async_compaction(true)                      // Background compaction thread
        .write_back_cache(true)                      // Buffer writes in memory
        .build()
}

fn example_write_back() -> std::io::Result<()> {
    let db = CrabKv::builder("data")
        .write_back_cache(true)
        .build()?;
    
    // Writes are buffered in memory (fast)
    db.put("key1".into(), "value1".into())?;
    db.put("key2".into(), "value2".into())?;
    
    // Explicit flush to persist buffered writes to WAL
    db.flush()?;
    
    Ok(())
}

fn example_batch_writes() -> std::io::Result<()> {
    let db = CrabKv::builder("data").build()?;
    
    // Batch multiple writes with a single fsync
    let entries = vec![
        ("key1".to_string(), "value1".to_string(), None),
        ("key2".to_string(), "value2".to_string(), Some(Duration::from_secs(60))),
    ];
    db.put_batch(entries)?;
    
    Ok(())
}
```

`CrabKv` implements `Clone`, so handles can be shared between threads. Writes are serialized while reads proceed concurrently.

### Performance Tuning

- **Sync Interval**: Set `.sync_interval(Duration)` to batch fsyncs and trade durability for throughput. `None` (default) syncs every write.
- **Compression**: Enable `.compression(true)` to reduce WAL size at the cost of CPU. Uses Snappy for fast compression/decompression.
- **Async Compaction**: Enable `.async_compaction(true)` to run compaction in a background thread without blocking writes.
- **Write-Back Cache**: Enable `.write_back_cache(true)` to buffer hot writes in memory. **Important**: Must call `db.flush()` periodically or before shutdown to persist buffered data.
- **Batch Writes**: Use `put_batch()` to write multiple entries with a single fsync, improving throughput for bulk operations.

## Implementation Notes

- Every mutation is appended to `data/wal.log` (or the configured directory) with an optional `expires_at` timestamp.
- Startup replays the log to rebuild the in-memory index and drop expired entries.
- The optional cache sits in front of the WAL to avoid disk reads for hot keys.
- Compaction is triggered when the stale-to-live ratio is roughly ≥ 1/3 and the log exceeds 1 MiB, but can be forced manually.
- Criterion benchmarks exercise writes, hits/misses, and compaction to track regressions.


