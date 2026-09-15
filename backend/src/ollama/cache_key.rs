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
pub(crate) fn url_host(url: &str) -> Option<String> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host = rest.split(['/', ':', '?']).next().unwrap_or(rest);
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
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
}
