//! Per-request unique ids (8 hex chars).
//!
//! Mixes wall-clock time, process id, and a per-process counter into a
//! SHA-256 digest so two requests in the same clock tick still differ.

use std::sync::atomic::{AtomicU64, Ordering};

/// Per-process counter mixed into request ids.
static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generate a short unique id for the current request (8 hex chars).
pub(crate) fn new_request_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let counter = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut input = Vec::with_capacity(24);
    input.extend_from_slice(&nanos.to_le_bytes());
    input.extend_from_slice(&std::process::id().to_le_bytes());
    input.extend_from_slice(&counter.to_le_bytes());
    crate::hash::sha256_short_id(&input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_ids_are_unique_under_burst() {
        let mut ids = std::collections::HashSet::new();
        for _ in 0..5000 {
            ids.insert(new_request_id());
        }
        assert_eq!(ids.len(), 5000);
    }
}
