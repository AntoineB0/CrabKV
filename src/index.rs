//! In-memory index pointing to values stored in the write-ahead log.

use std::fmt;

/// Location of a value within the log.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValuePointer {
    /// Byte offset inside the log file where the record begins.
    pub offset: u64,
    /// Length of the stored value payload in bytes.
    pub value_len: u32,
    /// Total size of the log record, including header and key bytes.
    pub record_len: u32,
}

impl ValuePointer {
    /// Creates a pointer describing a record written to the log.
    pub fn new(offset: u64, value_len: u32, record_len: u32) -> Self {
        Self {
            offset,
            value_len,
            record_len,
        }
    }
}

impl fmt::Display for ValuePointer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "offset={}, value_len={}, record_len={}", self.offset, self.value_len, self.record_len)
    }
}
