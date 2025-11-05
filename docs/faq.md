# CrabKv FAQ

**Why another key-value store?**  
CrabKv is a teaching project that demonstrates the core building blocks behind engines such as Bitcask or early LevelDB: append-only logs, in-memory indexes, compaction, caching, and TTLs—all in a compact codebase.

**How durable are writes?**  
Every `put` and `delete` fsyncs before the call returns. If the process or machine crashes, the engine replays the WAL on startup and rebuilds the index. The worst-case loss is the in-flight operation that had not returned yet.

**What happens when a TTL expires?**  
Reads check the `expires_at` timestamp stored in each record. Expired entries are dropped from the index (and cache) on access, and compaction refuses to copy them into the new log file.

**Can I disable the cache?**  
Yes. The cache is opt-in. Omit `--cache` on the CLI (or skip `cache_capacity` on the builder) and the engine falls back to direct WAL reads.

**Is the TCP server production-ready?**  
It is intentionally minimal. Commands are parsed line by line without authentication or TLS. Treat it as a debugging hook or prototype transport.

**Why do tests fail on Windows with `dlltool` errors?**  
Rust installations that use the GNU toolchain (`stable-x86_64-pc-windows-gnu`) lack a compatible `dlltool`. Install the MSVC toolchain (`rustup default stable-x86_64-pc-windows-msvc`) and rerun `cargo test`.

**How large can values be?**  
The WAL uses `u32` lengths for keys and values, allowing payloads up to 4 GiB. Practical limits depend on disk space and your willingness to hold values in memory during reads.

**Can I plug in compression or custom serialization?**  
Not yet. The WAL stores raw bytes. The roadmap includes hooks for codecs; contributions are welcome.

**Where do I start if I want to contribute?**  
Review `docs/architecture.md` for a system overview, then open an issue or submit a pull request describing the feature or fix you have in mind.
