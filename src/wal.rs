//! Write-ahead log providing durable storage for CrabKv operations.

use crate::index::ValuePointer;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufReader, BufWriter, ErrorKind, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const HEADER_SIZE: usize = 1 + 4 + 4 + 1 + 8;

#[derive(Clone, Debug, Eq, PartialEq)]
enum WalOp {
    Put = 1,
    Delete = 2,
}

impl WalOp {
    fn from_byte(byte: u8) -> io::Result<Self> {
        match byte {
            1 => Ok(WalOp::Put),
            2 => Ok(WalOp::Delete),
            _ => Err(io::Error::new(ErrorKind::InvalidData, "unknown WAL opcode")),
        }
    }
}

/// Persistent log entry describing either a put or delete operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WalEntry {
    /// Stores the provided UTF-8 value for the key.
    Put {
        key: String,
        value: String,
        expires_at: Option<SystemTime>,
    },
    /// Removes the key from the store.
    Delete { key: String },
}

impl WalEntry {
    fn key_bytes(&self) -> &[u8] {
        match self {
            WalEntry::Put { key, .. } | WalEntry::Delete { key } => key.as_bytes(),
        }
    }

    fn value_bytes(&self) -> &[u8] {
        match self {
            WalEntry::Put { value, .. } => value.as_bytes(),
            WalEntry::Delete { .. } => &[],
        }
    }

    fn expires_at(&self) -> Option<SystemTime> {
        match self {
            WalEntry::Put { expires_at, .. } => *expires_at,
            WalEntry::Delete { .. } => None,
        }
    }
}

/// Decoded record retrieved from the log.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WalRecord {
    /// Entry reconstructed from the log.
    pub entry: WalEntry,
    /// Starting byte offset of the record within the log.
    pub offset: u64,
    /// Total size of the record in bytes.
    pub record_len: u32,
    /// Size of the value payload.
    pub value_len: u32,
}

/// Write-ahead log abstraction responsible for durable persistence.
#[derive(Debug)]
pub struct Wal {
    path: PathBuf,
    writer: Mutex<BufWriter<File>>,
    last_sync: Mutex<Instant>,
    sync_interval: Option<Duration>,
    compression: bool,
}

impl Wal {
    /// Opens or creates the log at the given path with optional sync interval.
    pub fn open(
        path: impl AsRef<Path>,
        sync_interval: Option<Duration>,
        compression: bool,
    ) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .read(true)
            .append(true)
            .open(&path)?;
        let writer = Mutex::new(BufWriter::new(file));
        let last_sync = Mutex::new(Instant::now());
        Ok(Self {
            path,
            writer,
            last_sync,
            sync_interval,
            compression,
        })
    }

    /// Returns the underlying log path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the current size of the log in bytes.
    pub fn size(&self) -> io::Result<u64> {
        match fs::metadata(&self.path) {
            Ok(meta) => Ok(meta.len()),
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(0),
            Err(err) => Err(err),
        }
    }

    /// Appends an entry to the log and returns a pointer describing it.
    pub fn append(&self, entry: &WalEntry) -> io::Result<ValuePointer> {
        let encoded = self.encode_entry(entry)?;
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| io::Error::new(ErrorKind::Other, "writer poisoned"))?;
        let offset = writer.seek(SeekFrom::End(0))?;
        writer.write_all(&encoded)?;

        // Conditional sync based on interval
        let should_sync = if let Some(interval) = self.sync_interval {
            let mut last = self
                .last_sync
                .lock()
                .map_err(|_| io::Error::new(ErrorKind::Other, "sync lock poisoned"))?;
            if last.elapsed() >= interval {
                *last = Instant::now();
                true
            } else {
                false
            }
        } else {
            true
        };

        if should_sync {
            writer.flush()?;
            writer.get_ref().sync_data()?;
        }

        Ok(ValuePointer::new(
            offset,
            entry.value_bytes().len() as u32,
            encoded.len() as u32,
        ))
    }

    /// Appends multiple entries in a single batch and returns pointers for each.
    pub fn append_batch(&self, entries: &[WalEntry]) -> io::Result<Vec<ValuePointer>> {
        if entries.is_empty() {
            return Ok(Vec::new());
        }

        let mut writer = self
            .writer
            .lock()
            .map_err(|_| io::Error::new(ErrorKind::Other, "writer poisoned"))?;

        let mut pointers = Vec::with_capacity(entries.len());
        let mut offset = writer.seek(SeekFrom::End(0))?;

        for entry in entries {
            let encoded = self.encode_entry(entry)?;
            writer.write_all(&encoded)?;
            pointers.push(ValuePointer::new(
                offset,
                entry.value_bytes().len() as u32,
                encoded.len() as u32,
            ));
            offset += encoded.len() as u64;
        }

        // Always flush and sync after batch
        writer.flush()?;
        writer.get_ref().sync_data()?;
        let mut last_sync = self
            .last_sync
            .lock()
            .map_err(|_| io::Error::new(ErrorKind::Other, "sync lock poisoned"))?;
        *last_sync = Instant::now();

        Ok(pointers)
    }

    /// Reads the record stored at the provided pointer.
    pub fn read_record(&self, pointer: ValuePointer) -> io::Result<WalRecord> {
        self.read_record_at(pointer.offset)
    }

    /// Loads the index by replaying the log from scratch.
    pub fn load_index(
        &self,
    ) -> io::Result<(HashMap<String, (ValuePointer, Option<SystemTime>)>, u64)> {
        let file = match File::open(&self.path) {
            Ok(file) => file,
            Err(err) if err.kind() == ErrorKind::NotFound => return Ok((HashMap::new(), 0)),
            Err(err) => return Err(err),
        };
        let mut reader = BufReader::new(file);
        let mut offset = 0u64;
        let mut index = HashMap::new();
        let mut stale = 0u64;

        while let Some(record) = Self::read_record_internal(&mut reader, self.compression)? {
            let pointer = ValuePointer::new(offset, record.value_len, record.record_len);
            match &record.entry {
                WalEntry::Put {
                    key, expires_at, ..
                } => {
                    if let Some((previous, _)) = index.insert(key.clone(), (pointer, *expires_at)) {
                        stale += previous.record_len as u64;
                    }
                }
                WalEntry::Delete { key } => {
                    if let Some((previous, _)) = index.remove(key) {
                        stale += previous.record_len as u64;
                    }
                }
            }
            offset += record.record_len as u64;
        }

        Ok((index, stale))
    }

    /// Rewrites the log with the provided entries and returns the rebuilt index.
    pub fn rewrite(
        &self,
        entries: &[(String, String, Option<SystemTime>)],
    ) -> io::Result<HashMap<String, (ValuePointer, Option<SystemTime>)>> {
        let mut index = HashMap::new();
        let mut offset = 0u64;
        let temp_path = self.path.with_extension("compact");
        let backup_path = self.path.with_extension("backup");

        {
            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&temp_path)?;
            let mut writer = BufWriter::new(file);

            for (key, value, expires_at) in entries {
                let entry = WalEntry::Put {
                    key: key.clone(),
                    value: value.clone(),
                    expires_at: *expires_at,
                };
                let encoded = self.encode_entry(&entry)?;
                writer.write_all(&encoded)?;
                let pointer =
                    ValuePointer::new(offset, value.as_bytes().len() as u32, encoded.len() as u32);
                index.insert(key.clone(), (pointer, *expires_at));
                offset += encoded.len() as u64;
            }
            writer.flush()?;
            writer.get_ref().sync_all()?;
        }

        if self.path.exists() {
            if backup_path.exists() {
                fs::remove_file(&backup_path)?;
            }
            fs::rename(&self.path, &backup_path)?;
            match fs::rename(&temp_path, &self.path) {
                Ok(()) => {
                    fs::remove_file(&backup_path)?;
                }
                Err(err) => {
                    fs::rename(&backup_path, &self.path)?;
                    let _ = fs::remove_file(&temp_path);
                    return Err(err);
                }
            }
        } else {
            fs::rename(&temp_path, &self.path)?;
        }

        if temp_path.exists() {
            let _ = fs::remove_file(&temp_path);
        }

        Ok(index)
    }

    fn read_record_at(&self, offset: u64) -> io::Result<WalRecord> {
        let mut file = OpenOptions::new().read(true).open(&self.path)?;
        file.seek(SeekFrom::Start(offset))?;
        match Self::read_record_internal(&mut file, self.compression)? {
            Some(mut record) => {
                record.offset = offset;
                Ok(record)
            }
            None => Err(io::Error::new(
                ErrorKind::UnexpectedEof,
                "missing record at offset",
            )),
        }
    }

    fn read_record_internal<R: Read>(reader: &mut R, compression: bool) -> io::Result<Option<WalRecord>> {
        let mut op_buf = [0u8; 1];
        let read = reader.read(&mut op_buf)?;
        if read == 0 {
            return Ok(None);
        }
        let op = WalOp::from_byte(op_buf[0])?;

        let mut len_buf = [0u8; 4];
        reader.read_exact(&mut len_buf)?;
        let key_len = u32::from_le_bytes(len_buf) as usize;
        reader.read_exact(&mut len_buf)?;
        let value_len = u32::from_le_bytes(len_buf) as usize;

        let mut ttl_flag = [0u8; 1];
        reader.read_exact(&mut ttl_flag)?;
        let mut ttl_buf = [0u8; 8];
        reader.read_exact(&mut ttl_buf)?;
        let ttl_secs = u64::from_le_bytes(ttl_buf);

        let mut key_buf = vec![0u8; key_len];
        reader.read_exact(&mut key_buf)?;
        let key = String::from_utf8(key_buf)
            .map_err(|_| io::Error::new(ErrorKind::InvalidData, "invalid utf-8 key"))?;
        let mut value = String::new();

        if matches!(op, WalOp::Put) {
            let mut value_buf = vec![0u8; value_len];
            reader.read_exact(&mut value_buf)?;
            
            let decompressed = if compression && !value_buf.is_empty() {
                snap::raw::Decoder::new()
                    .decompress_vec(&value_buf)
                    .map_err(|e| io::Error::new(ErrorKind::Other, e))?
            } else {
                value_buf
            };
            
            value = String::from_utf8(decompressed)
                .map_err(|_| io::Error::new(ErrorKind::InvalidData, "invalid utf-8 value"))?;
        } else if value_len != 0 {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "delete record has unexpected payload",
            ));
        }

        let record_len = (HEADER_SIZE + key_len + value_len) as u32;
        let expires_at = if ttl_flag[0] == 1 {
            Some(
                UNIX_EPOCH
                    .checked_add(Duration::from_secs(ttl_secs))
                    .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "ttl overflow"))?,
            )
        } else {
            None
        };

        let entry = match op {
            WalOp::Put => WalEntry::Put {
                key,
                value,
                expires_at,
            },
            WalOp::Delete => WalEntry::Delete { key },
        };

        Ok(Some(WalRecord {
            entry,
            offset: 0,
            record_len,
            value_len: value_len as u32,
        }))
    }

    fn encode_entry(&self, entry: &WalEntry) -> io::Result<Vec<u8>> {
        let key = entry.key_bytes();
        let value = entry.value_bytes();

        let compressed;
        let final_value = if self.compression && !value.is_empty() {
            compressed = snap::raw::Encoder::new()
                .compress_vec(value)
                .map_err(|e| io::Error::new(ErrorKind::Other, e))?;
            &compressed[..]
        } else {
            value
        };

        let mut buf = Vec::with_capacity(HEADER_SIZE + key.len() + final_value.len());
        buf.push(match entry {
            WalEntry::Put { .. } => WalOp::Put as u8,
            WalEntry::Delete { .. } => WalOp::Delete as u8,
        });
        buf.extend_from_slice(&(key.len() as u32).to_le_bytes());
        buf.extend_from_slice(&(final_value.len() as u32).to_le_bytes());

        let mut flag = 0u8;
        let mut ttl = 0u64;
        if let Some(expires_at) = entry.expires_at() {
            if let Ok(duration) = expires_at.duration_since(UNIX_EPOCH) {
                flag = 1;
                ttl = duration.as_secs();
            }
        }
        buf.push(flag);
        buf.extend_from_slice(&ttl.to_le_bytes());
        buf.extend_from_slice(key);
        buf.extend_from_slice(final_value);
        Ok(buf)
    }
}
