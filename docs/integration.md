# Integrating CrabKv

This guide highlights the options available when embedding CrabKv in another Rust application or when orchestrating it via the CLI and TCP server.

## Library Setup

Add the crate to your `Cargo.toml` (the path depends on how you vendor CrabKv):

```toml
[dependencies]
crabkv = { path = "../CrabKV" }
```

Then wire the engine using the builder:

```rust
use crabkv::CrabKv;
use std::num::NonZeroUsize;
use std::time::Duration;

fn init_store() -> std::io::Result<CrabKv> {
    CrabKv::builder("data")
        .cache_capacity(NonZeroUsize::new(4096).unwrap())
        .default_ttl(Duration::from_secs(300))
        .build()
}
```

Clone the returned handle to share it across threads. Reads are cheap, while writes serialize automatically to protect the WAL.

## CLI Workflow

The compiled binary exposes all CRUD operations plus maintenance commands:

```powershell
# Store a value with a custom TTL
crabkv put session abc123 --ttl 45

# Fetch values
crabkv get session

# Delete keys or force compaction
crabkv delete session
crabkv compact
```

Environment variables mirror the builder knobs for quick one-off experiments:

- `CRABKV_DATA_DIR` changes where the WAL lives.
- `CRABKV_CACHE_CAPACITY` enables the LRU cache without passing `--cache`.
- `CRABKV_DEFAULT_TTL_SECS` applies to writes that omit `--ttl`.

## TCP Server

To expose CrabKv over the network:

```powershell
crabkv serve --addr 0.0.0.0:4000 --cache 8192 --default-ttl 120
```

The wire protocol is textual and intentionally simple:

```
PUT key value ttl=30
GET key
DELETE key
COMPACT
HELP
```

Responses include `OK`, `VALUE <payload>`, `NOT_FOUND`, or `ERR <description>`. Because handles are cloned per connection, hundreds of concurrent clients can share the same underlying engine.

## Testing & Benchmarks

- `cargo test` runs end-to-end scenarios, including TTL expiration.
- `cargo bench` executes Criterion benchmarks (`write_heavy`, `read_hit`, `read_miss`, and `compact` scenarios). Run them periodically to watch for regressions when tuning heuristics.

## Operational Tips

- Keep the `data/` directory on fast storage; WAL appends are synchronous.
- Compact proactively if the log keeps growing (the CLI or server `COMPACT` command helps in batch jobs).
- Monitor disk usage by watching the `wal.log` file size; stale ratios are printed in debug logs inside the engine when compaction kicks in.
- When deploying on Windows, install the MSVC toolchain so that both tests and Criterion can link successfully.

These guidelines should help you bootstrap CrabKv inside services, command-line workflows, or automated tests.
