use crabkv::CrabKv;
use criterion::{BatchSize, Criterion, SamplingMode, criterion_group, criterion_main};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

fn bench_put(c: &mut Criterion) {
    let mut group = c.benchmark_group("writes");
    group.sampling_mode(SamplingMode::Auto);
    group.warm_up_time(std::time::Duration::from_secs(3));
    group.measurement_time(std::time::Duration::from_secs(10));
    group.bench_function("sequential_put_1k", |b| {
        b.iter_batched(
            BenchContext::new,
            |ctx| {
                for i in 0..1_000 {
                    let key = format!("k{i}");
                    ctx.engine.put(key, "v".to_string()).unwrap();
                }
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_get(c: &mut Criterion) {
    let mut group = c.benchmark_group("reads");
    group.warm_up_time(std::time::Duration::from_secs(2));
    group.measurement_time(std::time::Duration::from_secs(8));
    group.bench_function("sequential_get_1k", |b| {
        b.iter_batched(
            || {
                let mut ctx = BenchContext::new();
                for i in 0..1_000 {
                    let key = format!("k{i}");
                    ctx.engine.put(key.clone(), "v".to_string()).unwrap();
                    ctx.keys.push(key);
                }
                ctx
            },
            |ctx| {
                for key in &ctx.keys {
                    let _ = ctx.engine.get(key).unwrap();
                }
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

fn bench_compaction(c: &mut Criterion) {
    let mut group = c.benchmark_group("compaction");
    group.warm_up_time(std::time::Duration::from_secs(2));
    group.measurement_time(std::time::Duration::from_secs(10));
    group.bench_function("compaction_cycle", |b| {
        b.iter_batched(
            || {
                let ctx = BenchContext::new();
                for i in 0..2_000 {
                    let key = format!("k{i}");
                    ctx.engine.put(key.clone(), format!("value-{i}")).unwrap();
                    if i % 2 == 0 {
                        ctx.engine.delete(&key).unwrap();
                    }
                }
                ctx
            },
            |ctx| {
                ctx.engine.compact().unwrap();
            },
            BatchSize::SmallInput,
        );
    });
    group.finish();
}

struct BenchContext {
    engine: CrabKv,
    _dir: BenchDir,
    keys: Vec<String>,
}

impl BenchContext {
    fn new() -> Self {
        let dir = BenchDir::new().expect("bench dir");
        let engine = CrabKv::open(dir.path()).expect("engine");
        Self {
            engine,
            _dir: dir,
            keys: Vec::new(),
        }
    }
}

struct BenchDir {
    path: PathBuf,
}

impl BenchDir {
    fn new() -> io::Result<Self> {
        let mut path = std::env::temp_dir();
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!("crabkv-bench-{unique}"));
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

impl Drop for BenchDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

criterion_group!(benches, bench_put, bench_get, bench_compaction);
criterion_main!(benches);
