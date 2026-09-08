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

/// Upper bound on cached results; eviction drops expired entries first,
/// then one arbitrary entry. Bounds remote memory growth from distinct
/// per-request prompts while keeping the hot path O(1)-ish.
pub const MAX_CACHE_ENTRIES: usize = 512;
/// Upper bound on TTL (30 days); larger configured values are clamped so
/// `Instant + ttl` can never overflow and poison the mutex.
pub const MAX_CACHE_TTL_SECS: u64 = 30 * 24 * 60 * 60;

/// TTL cache of estimation results.
pub struct Cache {
    ttl: Duration,
    entries: Mutex<HashMap<String, Entry>>,
}

impl Cache {
    /// Create a cache with the given TTL. A TTL of 0 disables caching.
    pub fn new(cache_ttl: u64) -> Cache {
        Cache {
            ttl: Duration::from_secs(cache_ttl.min(MAX_CACHE_TTL_SECS)),
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Number of entries currently held, including unexpired ones.
    pub fn len(&self) -> usize {
        self.entries.lock().unwrap().len()
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
        if !entries.contains_key(key) && entries.len() >= MAX_CACHE_ENTRIES {
            entries.retain(|_, e| Instant::now() <= e.expires_at);
            if entries.len() >= MAX_CACHE_ENTRIES {
                if let Some(oldest) = entries.keys().next().cloned() {
                    entries.remove(&oldest);
                }
            }
        }
        let expires_at = Instant::now()
            .checked_add(self.ttl)
            .unwrap_or_else(|| Instant::now() + Duration::from_secs(MAX_CACHE_TTL_SECS));
        entries.insert(key.to_string(), Entry { expires_at, result });
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

    #[test]
    fn huge_ttl_does_not_panic() {
        let cache = Cache::new(u64::MAX);
        cache.put("k", result(1));
        assert_eq!(cache.get("k"), Some(result(1)));
    }

    #[test]
    fn entries_are_capped() {
        let cache = Cache::new(300);
        for i in 0..700 {
            cache.put(&format!("key-{}", i), result(i));
        }
        assert!(cache.len() <= 512);
        // Most recent insert always survives.
        assert_eq!(cache.get("key-699"), Some(result(699)));
    }
}
