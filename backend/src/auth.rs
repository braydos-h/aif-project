//! Authentication primitives: password hashing, secure tokens, cookies.
//!
//! Uses the maintained `argon2` password hasher (never home-grown crypto)
//! and `rand` for 256-bit session/invite tokens. Only SHA-256 hashes of
//! tokens are stored; raw tokens live only in the operator's delivery
//! channel and the user's cookie.

use argon2::password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString};
use argon2::Argon2;
use rand::RngCore;

use crate::hash::sha256_hex;

pub const SESSION_COOKIE: &str = "aif_session";
/// Minimum password length (NIST-style: length over complexity rules).
pub const MIN_PASSWORD_LEN: usize = 10;
/// Maximum password length (bounds hashing work on hostile input).
pub const MAX_PASSWORD_LEN: usize = 256;
/// Invite/recovery token bytes (256-bit) and CSRF token bytes.
pub const TOKEN_BYTES: usize = 32;
pub const CSRF_BYTES: usize = 16;

/// Hash a password with Argon2id (OS-random salt per password).
pub fn hash_password(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| format!("hash failed: {}", e))
}

/// Verify a password against a stored PHC hash. Returns false (not an
/// error) for unparseable hashes so a corrupt row fails closed.
pub fn verify_password(password: &str, hash: &str) -> bool {
    let parsed = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return false,
    };
    Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok()
}

/// Validate a candidate password against the length policy.
pub fn check_password_policy(password: &str) -> Result<(), String> {
    if password.len() < MIN_PASSWORD_LEN {
        return Err(format!(
            "Password must be at least {} characters.",
            MIN_PASSWORD_LEN
        ));
    }
    if password.len() > MAX_PASSWORD_LEN {
        return Err("Password must be under 256 characters.".to_string());
    }
    Ok(())
}

/// Generate `n` random bytes as lowercase hex.
pub fn random_hex(nbytes: usize) -> String {
    let mut buf = vec![0u8; nbytes];
    rand::thread_rng().fill_bytes(&mut buf);
    let mut out = String::with_capacity(nbytes * 2);
    for b in buf {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

/// Short opaque id for users/invites/animals/estimates (128-bit hex).
pub fn new_id() -> String {
    random_hex(16)
}

/// SHA-256 hash of a token for database storage.
pub fn token_hash(token: &str) -> String {
    sha256_hex(token.as_bytes())
}

/// Normalize an email for storage/comparison (trim + lowercase).
pub fn normalize_email(email: &str) -> String {
    email.trim().to_ascii_lowercase()
}

/// Basic email shape check (full RFC validation is out of scope for two
/// invited users; operator invites go to known addresses).
pub fn valid_email(email: &str) -> bool {
    if email.is_empty() || email.len() > 254 || email.contains(' ') {
        return false;
    }
    let mut parts = email.split('@');
    match (parts.next(), parts.next(), parts.next()) {
        (Some(local), Some(domain), None) => {
            !local.is_empty() && domain.contains('.') && !domain.starts_with('.') && !domain.ends_with('.')
        }
        _ => false,
    }
}

/// Validate a display name (short free text, no control characters).
pub fn valid_display_name(name: &str) -> bool {
    let trimmed = name.trim();
    !trimmed.is_empty()
        && trimmed.len() <= 64
        && !trimmed.bytes().any(|b| b.is_ascii_control())
}

/// Build the `Set-Cookie` header value for a session token.
pub fn session_cookie_value(token: &str, max_age_secs: u64, secure: bool) -> String {
    let mut cookie = format!(
        "{}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}",
        SESSION_COOKIE, token, max_age_secs
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

/// Expired-cookie value used on logout.
pub fn clear_cookie_value(secure: bool) -> String {
    let mut cookie = format!("{}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0", SESSION_COOKIE);
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

/// Extract the session token from a `Cookie` header value.
pub fn session_token_from_cookie(header: &str) -> Option<String> {
    for part in header.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix(&format!("{}=", SESSION_COOKIE)) {
            let value = value.trim();
            if !value.is_empty() && value.len() <= 256 {
                return Some(value.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn password_round_trip_and_wrong_password_fails() {
        let hash = hash_password("correct horse battery staple").unwrap();
        assert!(hash.starts_with("$argon2"));
        assert!(verify_password("correct horse battery staple", &hash));
        assert!(!verify_password("wrong password here!!", &hash));
        assert!(!verify_password("correct horse battery staple", "not-a-hash"));
        assert!(!hash.contains("correct horse"));
    }

    #[test]
    fn password_policy_bounds_length() {
        assert!(check_password_policy("short").is_err());
        assert!(check_password_policy("long enough password").is_ok());
        assert!(check_password_policy(&"x".repeat(257)).is_err());
    }

    #[test]
    fn token_hashes_differ_and_ids_are_unique() {
        assert_ne!(random_hex(32), random_hex(32));
        assert_ne!(new_id(), new_id());
        assert_eq!(token_hash("abc").len(), 64);
    }

    #[test]
    fn email_validation_basics() {
        assert!(valid_email("user@example.com"));
        assert!(!valid_email("not-an-email"));
        assert!(!valid_email("a@b"));
        assert!(!valid_email("a @example.com"));
        assert_eq!(normalize_email("  User@Example.COM "), "user@example.com");
    }

    #[test]
    fn cookie_parse_finds_session_token() {
        assert_eq!(
            session_token_from_cookie("theme=dark; aif_session=abc123; x=1").as_deref(),
            Some("abc123")
        );
        assert!(session_token_from_cookie("theme=dark").is_none());
        let secure = session_cookie_value("t", 60, true);
        assert!(secure.contains("HttpOnly") && secure.contains("Secure") && secure.contains("SameSite=Lax"));
    }
}
