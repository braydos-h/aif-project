//! In-memory rate limiters for authentication and inference protection.
//!
//! Login, invite acceptance, and recovery attempts are bounded per IP (and
//! per account where applicable) so credential guessing cannot run hot.
//! Inference concurrency is bounded process-wide so one batch cannot starve
//! the server or queue unbounded provider work.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

/// Sliding-window attempt tracker: at most `max` events per `window_secs`.
pub struct AttemptLimiter {
    max: u32,
    window_secs: u64,
    hits: Mutex<HashMap<String, Vec<Instant>>>,
}

impl AttemptLimiter {
    /// New limiter allowing `max` attempts per `window_secs` seconds.
    pub fn new(max: u32, window_secs: u64) -> Self {
        AttemptLimiter {
            max,
            window_secs,
            hits: Mutex::new(HashMap::new()),
        }
    }

    /// Record an attempt for `key`. Returns false (and keeps the record)
    /// when the key is already over budget.
    pub fn check_and_record(&self, key: &str) -> bool {
        let mut hits = self.hits.lock().unwrap_or_else(|p| p.into_inner());
        let now = Instant::now();
        let entry = hits.entry(key.to_string()).or_default();
        entry.retain(|t| now.duration_since(*t).as_secs() < self.window_secs);
        if entry.len() as u32 >= self.max {
            return false;
        }
        entry.push(now);
        // Bound memory: drop keys that went quiet.
        if hits.len() > 4096 {
            hits.retain(|_, v| v.last().is_some_and(|t| now.duration_since(*t).as_secs() < self.window_secs));
        }
        true
    }

    /// Forget a key's history (used after a successful login).
    pub fn reset(&self, key: &str) {
        if let Ok(mut hits) = self.hits.lock() {
            hits.remove(key);
        }
    }
}

/// Process-wide inference concurrency guard.
pub struct InferenceGate {
    max: usize,
    active: Mutex<usize>,
}

impl InferenceGate {
    /// New gate allowing `max` concurrent inferences.
    pub fn new(max: usize) -> Self {
        InferenceGate {
            max: max.max(1),
            active: Mutex::new(0),
        }
    }

    /// Try to claim a slot. `None` means the server is busy (caller should
    /// answer `503 server_busy`); the guard releases the slot on drop.
    pub fn try_acquire(&self) -> Option<InferenceGuard<'_>> {
        let mut active = self.active.lock().unwrap_or_else(|p| p.into_inner());
        if *active >= self.max {
            return None;
        }
        *active += 1;
        Some(InferenceGuard { gate: self })
    }
}

/// RAII release for an inference slot.
pub struct InferenceGuard<'a> {
    gate: &'a InferenceGate,
}

impl Drop for InferenceGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.gate.active.lock() {
            *active = active.saturating_sub(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiter_blocks_after_budget() {
        let limiter = AttemptLimiter::new(2, 60);
        assert!(limiter.check_and_record("ip"));
        assert!(limiter.check_and_record("ip"));
        assert!(!limiter.check_and_record("ip"));
        limiter.reset("ip");
        assert!(limiter.check_and_record("ip"));
    }

    #[test]
    fn gate_caps_concurrency() {
        let gate = InferenceGate::new(1);
        let _held = gate.try_acquire().unwrap();
        assert!(gate.try_acquire().is_none());
    }
}
