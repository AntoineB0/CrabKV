# CrabKv Architecture

CrabKv keeps durability and simplicity by revolving around three long-lived components: the write-ahead log (WAL), the in-memory index, and the optional cache. This document sketches how they interact and where new features such as TTL and compaction fit.

## High-Level Flow

1. **Mutations** (`put`, `delete`, `put_with_ttl`) are serialized through a writer lock. The engine encodes a binary record into the WAL, flushes it, then updates the in-memory index.
2. **Reads** acquire a read lock, consult the index (and cache when enabled), and hit the WAL only when necessary. Expired entries are lazily evicted as soon as they are observed.
3. **Compaction** runs synchronously today. When the stale-to-live ratio crosses a heuristic threshold, the engine rewrites live records into a fresh WAL file, updates the index, and swaps files atomically.

The design favors durability and correctness ahead of raw throughput. Additional parallelism can be explored once the core feature set stabilizes.

## Modules

- `engine.rs`: Owns the index, WAL, cache, and configuration. Exposes the public API and orchestrates compaction.
- `wal.rs`: Binary format with a header describing record kind, payload sizes, and optional TTL metadata.
- `index.rs`: Defines `ValuePointer` and related helpers for tracking offsets inside the WAL.
- `cache.rs`: Thin wrapper around the `lru` crate that mirrors writes and invalidates deletions.
- `compaction.rs`: Computes stale ratios and provides the routines that rewrite live entries.
- `server.rs`: Binds a `CrabKv` instance to a TCP listener, parsing human-friendly commands.
- `config.rs`: Types powering `CrabKvBuilder`, allowing end users to opt into cache sizes and default TTLs.

## Storage Layout

```
<project root>
  data/
    wal.log        # Active WAL file (append-only)
    wal.log.old    # Previous generation during compaction
```

Each WAL record encodes:

- A fixed-size header with kind, key length, value length, and TTL seconds (0 means no TTL).
- The UTF-8 key bytes.
- For `Put`, the raw value bytes. `Delete` omits the value section.

`CrabKv` keeps no background threads by default. Compaction happens in the caller thread when thresholds are reached or the user explicitly triggers it.

## TTL Semantics

- TTL is stored as an absolute `expires_at` timestamp derived from `SystemTime::now()` plus the provided duration.
- Reads drop entries whose expiration is in the past and remove them from the index and cache.
- Compaction refuses to carry expired entries into the new log, shrinking the file automatically.
- A `default_ttl` can be configured via the builder or environment variable. CLI commands can still override TTL per write.

## Concurrency Model

- The engine wraps interior state in a `parking_lot::RwLock`, allowing multiple readers while forcing writes to queue.
- Cloned `CrabKv` handles share the same `Arc` so TCP workers and the CLI can coexist.
- WAL appends use buffered I/O to minimize syscalls while still flushing on each write for durability.

## Extension Points

- **Background compaction**: move `run_compaction` onto a worker thread with a channel for hints.
- **Compression**: extend the WAL header with codec identifiers and compress values before writing.
- **Replication**: mirror WAL appends to a remote sink. The builder already centralizes configuration wiring.

Understanding these components should make it easier to contribute features or evaluate tradeoffs when integrating CrabKv into a larger system.
