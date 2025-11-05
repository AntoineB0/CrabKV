# CrabKv

CrabKv is a lightweight key-value engine inspired by LevelDB. It focuses on durable append-only writes, an in-memory index for fast lookups, and log compaction to keep disk usage under control.

## Features

- Append-only write-ahead log with crash-safe rewrites on Windows and Unix.
- In-memory index mapping keys to log offsets for O(1) lookups.
- Manual and automatic compaction when stale bytes exceed configured heuristics.
- Thread-safe API built on `RwLock` for concurrent reads and serialized writes.
- Minimal CLI for manual inspection and CRUD operations.

## Project Structure

```
src/
  main.rs        # CLI entry point
  lib.rs         # Library facade exporting the engine
  engine.rs      # High-level store orchestration (index + WAL + compaction)
  wal.rs         # Binary write-ahead log encoder/decoder
  index.rs       # ValuePointer type describing offsets inside the log
  compaction.rs  # Simple heuristics to decide when to rewrite the log

tests/
  basic.rs       # Integration test covering persistence, overwrites, and compaction
```

## Getting Started

### Prerequisites

- Rust 1.79+ (ed. 2024)

### Build & Run

```powershell
cargo run -- put alpha 42
cargo run -- get alpha
cargo run -- delete alpha
cargo run -- compact
```

Set `CRABKV_DATA_DIR` to store the log somewhere else:

```powershell
$env:CRABKV_DATA_DIR = "D:/storage/crabkv"
cargo run -- put session token
```

### Testing

```powershell
cargo test
```

## CLI Reference

| Command                         | Description                           |
|---------------------------------|---------------------------------------|
| `crabkv put <key> <value>`      | Inserts or overwrites a value.        |
| `crabkv get <key>`              | Prints the stored value or a miss.    |
| `crabkv delete <key>`           | Removes the key if it exists.         |
| `crabkv compact`                | Forces log compaction immediately.    |

## Implementation Notes

- Each operation is appended to `data/wal.log` (or the configured directory).
- The engine rebuilds the in-memory index on startup by replaying the log.
- Stale bytes accumulate when keys are overwritten or deleted. Once the stale ratio crosses roughly one third of the log (and the file is larger than 1 MiB), the engine rewrites live entries into a fresh log file.
- Compaction is conservative on small logs and can also be triggered manually.

## Roadmap

- Background compaction scheduler to avoid pauses during writes.
- Configurable log thresholds and compaction strategy.
- Optional network front-end (HTTP/TCP) for remote access.
- Pluggable codecs for value compression and TTL support.
