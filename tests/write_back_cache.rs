use crabkv::CrabKv;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

struct TempDir {
    path: PathBuf,
}

impl TempDir {
    fn new() -> io::Result<Self> {
        let mut path = std::env::temp_dir();
        let random_suffix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        path.push(format!("crabkv-test-{}", random_suffix));
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

#[test]
fn write_back_cache_with_flush() -> io::Result<()> {
    let dir = TempDir::new()?;

    // Crée une base avec write-back cache activé
    let db = CrabKv::builder(dir.path())
        .cache_capacity(100.try_into().unwrap())
        .write_back_cache(true)
        .build()?;

    // Écrit plusieurs valeurs (elles sont bufferisées en mémoire)
    db.put("key1".into(), "value1".into())?;
    db.put("key2".into(), "value2".into())?;
    db.put("key3".into(), "value3".into())?;

    // Les valeurs doivent être lisibles depuis le cache
    assert_eq!(db.get("key1")?, Some("value1".into()));
    assert_eq!(db.get("key2")?, Some("value2".into()));
    assert_eq!(db.get("key3")?, Some("value3".into()));

    // Drop et réouverture SANS flush - les données ne doivent PAS persister
    drop(db);
    let db = CrabKv::builder(dir.path())
        .cache_capacity(100.try_into().unwrap())
        .write_back_cache(false) // Sans write-back pour lire depuis WAL
        .build()?;
    
    // Les données ne doivent pas exister (elles n'ont jamais été écrites dans le WAL)
    assert_eq!(db.get("key1")?, None);
    assert_eq!(db.get("key2")?, None);
    assert_eq!(db.get("key3")?, None);

    drop(db);

    // Nouvelle session avec flush cette fois
    let db = CrabKv::builder(dir.path())
        .cache_capacity(100.try_into().unwrap())
        .write_back_cache(true)
        .build()?;

    db.put("key4".into(), "value4".into())?;
    db.put("key5".into(), "value5".into())?;

    // Flush explicite vers le WAL
    db.flush()?;

    // Drop et réouverture
    drop(db);
    let db = CrabKv::builder(dir.path())
        .cache_capacity(100.try_into().unwrap())
        .write_back_cache(false)
        .build()?;

    // Les données doivent maintenant persister
    assert_eq!(db.get("key4")?, Some("value4".into()));
    assert_eq!(db.get("key5")?, Some("value5".into()));

    Ok(())
}

#[test]
fn write_back_cache_with_ttl() -> io::Result<()> {
    let dir = TempDir::new()?;

    let db = CrabKv::builder(dir.path())
        .cache_capacity(100.try_into().unwrap())
        .write_back_cache(true)
        .build()?;

    // Écrit avec TTL court
    db.put_with_ttl("temp_key".into(), "temp_value".into(), Some(Duration::from_millis(100)))?;

    // Lisible immédiatement
    assert_eq!(db.get("temp_key")?, Some("temp_value".into()));

    // Attend expiration
    std::thread::sleep(Duration::from_millis(150));

    // Ne doit plus être lisible (expiré dans le cache)
    assert_eq!(db.get("temp_key")?, None);

    Ok(())
}

#[test]
fn write_back_cache_update_overwrites_buffer() -> io::Result<()> {
    let dir = TempDir::new()?;

    let db = CrabKv::builder(dir.path())
        .cache_capacity(100.try_into().unwrap())
        .write_back_cache(true)
        .build()?;

    // Écrit une valeur initiale
    db.put("key".into(), "value1".into())?;
    assert_eq!(db.get("key")?, Some("value1".into()));

    // Écrase avec nouvelle valeur (doit mettre à jour le buffer)
    db.put("key".into(), "value2".into())?;
    assert_eq!(db.get("key")?, Some("value2".into()));

    // Flush et vérifie
    db.flush()?;
    assert_eq!(db.get("key")?, Some("value2".into()));

    Ok(())
}
