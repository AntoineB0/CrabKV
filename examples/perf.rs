use crabkv::CrabKv;
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Simple micro-benchmark to gauge CrabKv throughput without Criterion.
///
/// Usage:
///   cargo run --example perf -- <ops> [value_size_bytes]
///
/// Defaults to 10_000 operations and a 16-byte value payload.
fn main() -> io::Result<()> {
    let ops = env::args()
        .nth(1)
        .as_deref()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(10_000usize);
    let value_size = env::args()
        .nth(2)
        .as_deref()
        .and_then(|raw| raw.parse().ok())
        .unwrap_or(16usize);

    if ops == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "number of operations must be greater than zero",
        ));
    }

    let payload = "x".repeat(value_size.max(1));
    let dir = TempDir::new()?;
    let engine = CrabKv::open(dir.path())?;

    let write_start = Instant::now();
    for i in 0..ops {
        let key = format!("put-{i}");
        engine.put(key, payload.clone())?;
    }
    let write_elapsed = write_start.elapsed();

    let read_start = Instant::now();
    for i in 0..ops {
        let key = format!("put-{i}");
        let _ = engine.get(&key)?;
    }
    let read_elapsed = read_start.elapsed();

    let delete_start = Instant::now();
    for i in 0..ops {
        let key = format!("put-{i}");
        engine.delete(&key)?;
    }
    let delete_elapsed = delete_start.elapsed();

    print_report("sequential_put", ops, write_elapsed);
    print_report("sequential_get_hit", ops, read_elapsed);
    print_report("sequential_delete", ops, delete_elapsed);

    println!(
        "\nPayload size: {value_size} bytes | Directory: {}",
        dir.path().display()
    );

    Ok(())
}

fn print_report(label: &str, ops: usize, elapsed: Duration) {
    let seconds = elapsed.as_secs_f64().max(f64::EPSILON);
    let throughput = ops as f64 / seconds;
    println!(
        "{label:>20}: {:>8.3?} | {:>12.0} ops/s",
        elapsed, throughput
    );
}

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> io::Result<Self> {
        let mut path = std::env::temp_dir();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!("crabkv-perf-{unique}"));
        if path.exists() {
            fs::remove_dir_all(&path)?;
        }
        fs::create_dir_all(&path)?;
        Ok(Self { path })
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
