//! In-memory result cache, keyed by SHA-256 of the base64 image.
//!
//! Mirrors the per-estimator cache in `aif/estimator.py` (`_cache_get` /
//! `_cache_put`): entries expire after a TTL; TTL 0 disables caching
//! entirely.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::Value;

struct Entry {
    expires_at: Instant,
    result: Value,
}

/// TTL cache of estimation results.
pub struct Cache {
    ttl: Duration,
    entries: Mutex<HashMap<String, Entry>>,
}

impl Cache {
    /// Create a cache with the given TTL. A TTL of 0 disables caching.
    pub fn new(cache_ttl: u64) -> Cache {
        Cache {
            ttl: Duration::from_secs(cache_ttl),
            entries: Mutex::new(HashMap::new()),
        }
    }

    fn enabled(&self) -> bool {
        !self.ttl.is_zero()
    }

    /// Return a deep copy of the cached result for `key` if it is still
    /// within its TTL, else None. Expired entries are removed.
    pub fn get(&self, key: &str) -> Option<Value> {
        if !self.enabled() {
            return None;
        }
        let mut entries = self.entries.lock().unwrap();
        let entry = entries.get(key)?;
        if Instant::now() > entry.expires_at {
            entries.remove(key);
            return None;
        }
        Some(entry.result.clone())
    }

    /// Store `result` under `key` with the configured TTL. No-op when
    /// caching is disabled.
    pub fn put(&self, key: &str, result: Value) {
        if !self.enabled() {
            return;
        }
        let mut entries = self.entries.lock().unwrap();
        entries.insert(
            key.to_string(),
            Entry {
                expires_at: Instant::now() + self.ttl,
                result,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn result(n: u64) -> Value {
        serde_json::json!({"estimated_weight_kg": n})
    }

    #[test]
    fn cache_hit_returns_clone() {
        let cache = Cache::new(300);
        cache.put("key", result(1));
        assert_eq!(cache.get("key"), Some(result(1)));
    }

    #[test]
    fn disabled_cache_never_stores() {
        let cache = Cache::new(0);
        cache.put("key", result(1));
        assert_eq!(cache.get("key"), None);
    }

    #[test]
    fn expired_entry_removed() {
        let cache = Cache::new(1);
        cache.put("k", result(2));
        let mut entries = cache.entries.lock().unwrap();
        if let Some(e) = entries.get_mut("k") {
            e.expires_at = Instant::now() - Duration::from_secs(1);
        }
        drop(entries);
        assert_eq!(cache.get("k"), None);
    }
}
