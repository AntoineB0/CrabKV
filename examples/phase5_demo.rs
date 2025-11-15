//! Example demonstrating Phase 5 performance optimizations.
//!
//! Run with:
//!   cargo run --example phase5_demo

use crabkv::CrabKv;
use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

fn main() -> std::io::Result<()> {
    println!("=== CrabKv Phase 5 Performance Optimizations Demo ===\n");

    // Demo 1: Baseline (no optimizations)
    demo_baseline()?;

    // Demo 2: Buffered I/O with sync interval
    demo_buffered_io()?;

    // Demo 3: Batch writes
    demo_batch_writes()?;

    // Demo 4: Async compaction
    demo_async_compaction()?;

    // Demo 5: Compression
    demo_compression()?;

    // Demo 6: Write-back cache
    demo_write_back_cache()?;

    // Demo 7: All optimizations combined
    demo_all_optimizations()?;

    println!("\n=== All demos completed successfully! ===");
    Ok(())
}

fn demo_baseline() -> std::io::Result<()> {
    println!("📊 Demo 1: Baseline (no optimizations)");
    let dir = tempfile::tempdir()?;
    let db = CrabKv::builder(dir.path()).build()?;

    let start = Instant::now();
    for i in 0..1000 {
        db.put(format!("key{}", i), format!("value{}", i))?;
    }
    let elapsed = start.elapsed();

    println!("   ✓ 1000 writes in {:?} ({:.0} writes/sec)\n", elapsed, 1000.0 / elapsed.as_secs_f64());
    Ok(())
}

fn demo_buffered_io() -> std::io::Result<()> {
    println!("🚀 Demo 2: Buffered I/O with sync interval (100ms)");
    let dir = tempfile::tempdir()?;
    let db = CrabKv::builder(dir.path())
        .sync_interval(Duration::from_millis(100))
        .build()?;

    let start = Instant::now();
    for i in 0..1000 {
        db.put(format!("key{}", i), format!("value{}", i))?;
    }
    let elapsed = start.elapsed();

    println!("   ✓ 1000 writes in {:?} ({:.0} writes/sec)", elapsed, 1000.0 / elapsed.as_secs_f64());
    println!("   ℹ  Up to 100ms of unflushed data may be lost on crash\n");
    Ok(())
}

fn demo_batch_writes() -> std::io::Result<()> {
    println!("📦 Demo 3: Batch writes (100 entries per batch)");
    let dir = tempfile::tempdir()?;
    let db = CrabKv::builder(dir.path()).build()?;

    let start = Instant::now();
    for batch in 0..10 {
        let entries: Vec<_> = (0..100)
            .map(|i| {
                let idx = batch * 100 + i;
                (format!("key{}", idx), format!("value{}", idx), None)
            })
            .collect();
        db.put_batch(entries)?;
    }
    let elapsed = start.elapsed();

    println!("   ✓ 1000 writes (10 batches) in {:?} ({:.0} writes/sec)\n", elapsed, 1000.0 / elapsed.as_secs_f64());
    Ok(())
}

fn demo_async_compaction() -> std::io::Result<()> {
    println!("⚡ Demo 4: Async background compaction");
    let dir = tempfile::tempdir()?;
    let db = CrabKv::builder(dir.path())
        .async_compaction(true)
        .build()?;

    // Write enough data to trigger compaction
    for i in 0..5000 {
        db.put(format!("key{}", i % 100), format!("value{}", i))?;
    }

    println!("   ✓ Wrote 5000 updates (overwriting 100 keys)");
    println!("   ℹ  Compaction runs in background, writes never blocked\n");
    Ok(())
}

fn demo_compression() -> std::io::Result<()> {
    println!("📦 Demo 5: Snappy compression");
    let dir = tempfile::tempdir()?;
    let db = CrabKv::builder(dir.path())
        .compression(true)
        .build()?;

    let large_value = "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(10);
    
    let start = Instant::now();
    for i in 0..100 {
        db.put(format!("doc{}", i), large_value.clone())?;
    }
    let elapsed = start.elapsed();

    println!("   ✓ 100 large values in {:?}", elapsed);
    println!("   ℹ  Values transparently compressed with Snappy\n");
    Ok(())
}

fn demo_write_back_cache() -> std::io::Result<()> {
    println!("💾 Demo 6: Write-back cache (must flush!)");
    let dir = tempfile::tempdir()?;
    let db = CrabKv::builder(dir.path())
        .cache_capacity(NonZeroUsize::new(1000).unwrap())
        .write_back_cache(true)
        .build()?;

    let start = Instant::now();
    for i in 0..1000 {
        db.put(format!("key{}", i), format!("value{}", i))?;
    }
    let write_elapsed = start.elapsed();

    println!("   ✓ 1000 buffered writes in {:?} ({:.0} writes/sec)", 
             write_elapsed, 1000.0 / write_elapsed.as_secs_f64());

    let flush_start = Instant::now();
    db.flush()?;
    let flush_elapsed = flush_start.elapsed();

    println!("   ✓ Flush to WAL in {:?}", flush_elapsed);
    println!("   ⚠  Without flush(), data would be lost on crash!\n");
    Ok(())
}

fn demo_all_optimizations() -> std::io::Result<()> {
    println!("🔥 Demo 7: All optimizations combined");
    let dir = tempfile::tempdir()?;
    let db = CrabKv::builder(dir.path())
        .sync_interval(Duration::from_millis(100))
        .compression(true)
        .async_compaction(true)
        .cache_capacity(NonZeroUsize::new(1000).unwrap())
        .write_back_cache(true)
        .build()?;

    let start = Instant::now();
    for i in 0..10000 {
        db.put(format!("key{}", i), format!("value{}", i))?;
    }
    
    // Periodic flush (every 1000 writes)
    if start.elapsed() > Duration::from_millis(100) {
        db.flush()?;
    }
    
    db.flush()?; // Final flush
    let elapsed = start.elapsed();

    println!("   ✓ 10,000 writes in {:?} ({:.0} writes/sec)", elapsed, 10000.0 / elapsed.as_secs_f64());
    println!("   ℹ  Combining all optimizations maximizes throughput\n");
    Ok(())
}
