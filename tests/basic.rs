use crabkv::CrabKv;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[test]
fn put_get_delete_cycle() -> io::Result<()> {
    let temp = TempDir::new()?;
    let engine = CrabKv::open(temp.path())?;

    engine.put("alpha".into(), "1".into())?;
    assert_eq!(engine.get("alpha")?, Some("1".into()));

    engine.put("alpha".into(), "2".into())?;
    assert_eq!(engine.get("alpha")?, Some("2".into()));

    engine.delete("alpha")?;
    assert_eq!(engine.get("alpha")?, None);

    drop(engine);
    let engine = CrabKv::open(temp.path())?;
    assert_eq!(engine.get("alpha")?, None);

    engine.put("beta".into(), "fresh".into())?;
    assert_eq!(engine.get("beta")?, Some("fresh".into()));

    engine.compact()?;
    assert_eq!(engine.get("beta")?, Some("fresh".into()));

    Ok(())
}

#[test]
fn ttl_expiration() -> io::Result<()> {
    let temp = TempDir::new()?;
    let engine = CrabKv::builder(temp.path())
        .default_ttl(Duration::from_secs(10))
        .build()?;

    engine.put_with_ttl(
        "session".into(),
        "value".into(),
        Some(Duration::from_millis(100)),
    )?;
    assert_eq!(engine.get("session")?, Some("value".into()));

    sleep(Duration::from_millis(150));
    assert_eq!(engine.get("session")?, None);

    // Subsequent reopen should not resurrect the expired key.
    drop(engine);
    let engine = CrabKv::open(temp.path())?;
    assert_eq!(engine.get("session")?, None);
    Ok(())
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
        path.push(format!("crabkv-test-{unique}"));
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
