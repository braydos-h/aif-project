//! Shared SHA-256 helpers.
//!
//! Several backends need content hashing (cache keys, deterministic fallback
//! weights, request ids). Centralising it here removes three copies of the
//! same `sha2` boilerplate.

/// Raw 32-byte SHA-256 digest of `data`.
pub fn sha256_bytes(data: &[u8]) -> [u8; 32] {
    let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut hasher, data);
    let digest = sha2::Digest::finalize(hasher);
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    out
}

/// Lowercase hex SHA-256 digest of `data`.
pub fn sha256_hex(data: &[u8]) -> String {
    let digest = sha256_bytes(data);
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

/// First 8 hex chars of the SHA-256 digest — a short unique id.
pub fn sha256_short_id(data: &[u8]) -> String {
    sha256_hex(data).chars().take(8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_known_digest() {
        // sha256("abc") == ba7816bf...
        assert!(sha256_hex(b"abc").starts_with("ba7816bf8f01cfea414140de5dae2223"));
    }

    #[test]
    fn short_id_is_8_hex_chars() {
        let id = sha256_short_id(b"hello");
        assert_eq!(id.len(), 8);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }
}
