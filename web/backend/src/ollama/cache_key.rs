//! Cache-key and URL helpers for the Ollama backend.
//!
//! Runtime prompt/model/URL overrides must not reuse a result produced by a
//! different request configuration, so all four inputs feed the cache key.

use crate::hash::sha256_hex;

pub(crate) fn cache_key(image_b64: &str, model: &str, url: &str, prompt: &str) -> String {
    let mut input = String::new();
    for value in [image_b64, model, url, prompt] {
        input.push_str(value);
        input.push('\0');
    }
    sha256_hex(input.as_bytes())
}

/// Parse the hostname out of a URL string.
///
/// Returns the lowercased host so case variants (`OLLAMA.COM` vs
/// `ollama.com`) cannot bypass host-based checks. Bracketed IPv6 literals
/// (`[::1]:8080`) are unbracketed so distinct hosts never compare equal.
pub(crate) fn url_host(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let authority = rest.split('/').next().unwrap_or(rest);
    let host = if let Some(after_bracket) = authority.strip_prefix('[') {
        after_bracket.split(']').next().unwrap_or("")
    } else {
        authority.split([':', '?']).next().unwrap_or(authority)
    };
    if host.is_empty() {
        None
    } else {
        Some(host.to_ascii_lowercase())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_key_includes_request_configuration() {
        let first = cache_key("image", "model-a", "https://example.com", "prompt-a");
        let second = cache_key("image", "model-b", "https://example.com", "prompt-a");
        let third = cache_key("image", "model-a", "https://example.com", "prompt-b");
        assert_ne!(first, second);
        assert_ne!(first, third);
    }

    #[test]
    fn url_host_parses() {
        assert_eq!(
            url_host("https://ollama.com/api/generate"),
            Some("ollama.com".to_string())
        );
        assert_eq!(
            url_host("http://localhost:11434/api/generate"),
            Some("localhost".to_string())
        );
        assert_eq!(url_host("not-a-url"), None);
    }

    #[test]
    fn url_host_is_case_insensitive() {
        assert_eq!(
            url_host("https://OLLAMA.COM/api/generate"),
            Some("ollama.com".to_string())
        );
        assert_eq!(
            url_host("http://LocalHost:11434/api/generate"),
            Some("localhost".to_string())
        );
    }

    #[test]
    fn url_host_handles_bracketed_ipv6() {
        assert_eq!(
            url_host("http://[::1]:11434/api/generate"),
            Some("::1".to_string())
        );
        assert_eq!(
            url_host("http://[fe80::1]/cow.jpg"),
            Some("fe80::1".to_string())
        );
        // Distinct bracketed hosts must never compare equal.
        assert_ne!(
            url_host("http://[::1]:11434/api/generate"),
            url_host("http://[fe80::1]:11434/api/generate")
        );
    }
}
