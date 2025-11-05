# CrabKv

CrabKv is a lightweight Rust key-value store inspired by LevelDB. The engine keeps writes durable via an append-only log, rebuilds its in-memory index on restart, and now ships with caching, TTL, and a small TCP front-end for remote access.

## Features

- Append-only write-ahead log with crash-safe rewrites on Windows and Unix.
- In-memory index mapping keys to WAL offsets for O(1) lookups.
- Optional LRU cache to keep hot keys resident without extra plumbing.
- Per-write TTL metadata encoded in the WAL and honored during reads and compaction.
- Manual or automatic compaction once stale bytes cross configurable heuristics.
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
        .build()
}
```

`CrabKv` implements `Clone`, so handles can be shared between threads. Writes are serialized while reads proceed concurrently.

## Implementation Notes

- Every mutation is appended to `data/wal.log` (or the configured directory) with an optional `expires_at` timestamp.
- Startup replays the log to rebuild the in-memory index and drop expired entries.
- The optional cache sits in front of the WAL to avoid disk reads for hot keys.
- Compaction is triggered when the stale-to-live ratio is roughly ≥ 1/3 and the log exceeds 1 MiB, but can be forced manually.
- Criterion benchmarks exercise writes, hits/misses, and compaction to track regressions.

## Roadmap

- Background compaction to hide maintenance pauses.
- Pluggable compression codecs selectable per-column family.
- Snapshotting or replication hooks for multi-node deployments.
- Additional docs live under `docs/architecture.md`, `docs/integration.md`, and `docs/faq.md`.
