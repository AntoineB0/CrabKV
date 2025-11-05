//! Compaction heuristics used to decide when to rebuild the log.

/// Returns `true` when the ratio of stale records justifies rewriting the log.
pub fn should_compact(total_bytes: u64, stale_bytes: u64) -> bool {
    if total_bytes == 0 {
        return false;
    }
    let stale_ratio = stale_bytes as f64 / total_bytes as f64;
    stale_ratio >= 0.33 && total_bytes > 1_048_576 || stale_bytes > 8 * 1_048_576
}
