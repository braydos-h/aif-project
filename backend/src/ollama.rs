//! Ollama Cloud backend: POST the image (base64) + prompt, retry once on
//! transient failures, parse the reply.
//!
//! Mirrors `CowWeightEstimator._estimate_via_ollama` /
//! `_call_ollama_with_retry` in `aif/estimator.py`:
//! - API key required when the URL host is `ollama.com`, sent as a Bearer token.
//! - Retries once on 5xx and network/timeout errors, after a 1 s backoff.
//! - 4xx errors and non-JSON bodies are not retried.

use std::time::Duration;

use serde_json::{Value, json};

use crate::config::OLLAMA_MAX_RETRIES;
use crate::config::OLLAMA_RETRY_BACKOFF_SECS;
use crate::config::Config;
use crate::fallback::result_with_extras;
use crate::parse::parse_structured_response;
use crate::validate::to_base64_image;

/// Error raised when the Ollama call fails.
#[derive(Debug)]
pub struct OllamaError(pub String);

impl std::fmt::Display for OllamaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for OllamaError {}

/// The full estimate via Ollama: validates the image, checks cache, calls the
/// endpoint with retry, parses the reply. Returns the result dict.
pub fn estimate_via_ollama(
    config: &Config,
    cache: &crate::cache::Cache,
    image_reference: &str,
    prompt: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    // Mirror the Python check: host == ollama.com requires an API key.
    let host = url_host(&config.ollama_url);
    if host.as_deref() == Some("ollama.com") && config.ollama_api_key.is_none() {
        return Err(Box::new(OllamaError(
            "Ollama Cloud requires an API key. Set OLLAMA_API_KEY in .env \
             to an API key created at https://ollama.com/settings/keys."
                .to_string(),
        )));
    }

    let image_b64 = to_base64_image(image_reference)?;
    let cache_key = sha256_hex(image_b64.as_bytes());
    if let Some(cached) = cache.get(&cache_key) {
        eprintln!("cache hit for image {}", &cache_key[..12]);
        return Ok(cached);
    }

    let payload = json!({
        "model": config.model,
        "prompt": prompt,
        "images": [image_b64],
        "stream": false,
    });
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .build();

    let mut last_error: Option<OllamaError> = None;
    for attempt in 0..=OLLAMA_MAX_RETRIES {
        let mut request = agent
            .post(&config.ollama_url)
            .set("Content-Type", "application/json");
        if let Some(key) = &config.ollama_api_key {
            request = request.set("Authorization", &format!("Bearer {}", key));
        }
        let result = request.send_string(&payload.to_string());
        match result {
            Ok(response) => {
                let body = match response.into_string() {
                    Ok(b) => b,
                    Err(e) => {
                        last_error = Some(OllamaError(format!("Failed to read Ollama response: {}", e)));
                        if attempt == OLLAMA_MAX_RETRIES {
                            return Err(Box::new(last_error.unwrap()));
                        }
                        eprintln!(
                            "Ollama response read failed ({}), retrying in {}s (attempt {}/{})",
                            e,
                            OLLAMA_RETRY_BACKOFF_SECS,
                            attempt + 1,
                            OLLAMA_MAX_RETRIES
                        );
                        std::thread::sleep(Duration::from_secs(OLLAMA_RETRY_BACKOFF_SECS));
                        continue;
                    }
                };
                let parsed: Value = if body.is_empty() {
                    Value::Null
                } else {
                    serde_json::from_str(&body).map_err(|e| {
                        Box::new(OllamaError(format!("Ollama returned non-JSON body: {}", e)))
                    })?
                };
                let text = extract_text(&parsed);
                let Some(text) = text else {
                    return Err(Box::new(OllamaError(
                        "Ollama response did not contain any text".to_string(),
                    )));
                };
                let (weight_kg, extras) = match parse_structured_response(&text) {
                    Some(pair) => pair,
                    None => {
                        return Err(Box::new(OllamaError(format!(
                            "Could not extract a weight from Ollama response: {:?}",
                            text
                        ))));
                    }
                };
                let mut result = result_with_extras(weight_kg, prompt, &text, &extras);
                result["source"] = Value::from("ollama");
                result["model"] = Value::from(config.model.clone());
                cache.put(&cache_key, result.clone());
                return Ok(result);
            }
            Err(ureq::Error::Status(code, response)) => {
                let error_body = response.into_string().unwrap_or_default();
                let detail = error_detail(&error_body)
                    .unwrap_or_else(|| error_body_or_detail(&error_body, code));
                last_error = Some(OllamaError(format!(
                    "Ollama request failed (HTTP {}): {}",
                    code, detail
                )));
                if code < 500 || attempt == OLLAMA_MAX_RETRIES {
                    return Err(Box::new(last_error.unwrap()));
                }
                eprintln!(
                    "Ollama returned HTTP {}, retrying in {}s (attempt {}/{})",
                    code,
                    OLLAMA_RETRY_BACKOFF_SECS,
                    attempt + 1,
                    OLLAMA_MAX_RETRIES
                );
                std::thread::sleep(Duration::from_secs(OLLAMA_RETRY_BACKOFF_SECS));
            }
            Err(ureq::Error::Transport(t)) => {
                last_error = Some(OllamaError(format!(
                    "Unable to reach Ollama at {}: {}",
                    config.ollama_url, t
                )));
                if attempt == OLLAMA_MAX_RETRIES {
                    return Err(Box::new(last_error.unwrap()));
                }
                eprintln!(
                    "Ollama unreachable ({}), retrying in {}s (attempt {}/{})",
                    t,
                    OLLAMA_RETRY_BACKOFF_SECS,
                    attempt + 1,
                    OLLAMA_MAX_RETRIES
                );
                std::thread::sleep(Duration::from_secs(OLLAMA_RETRY_BACKOFF_SECS));
            }
        }
    }

    Err(Box::new(
        last_error.unwrap_or(OllamaError("Ollama call failed".to_string())),
    ))
}

/// Extract the model text from a parsed response: `response` field, or
/// `message.content` when a chat-style object is present.
fn extract_text(parsed: &Value) -> Option<String> {
    if let Some(s) = parsed.get("response").and_then(|v| v.as_str()) {
        if !s.is_empty() {
            return Some(s.to_string());
        }
    }
    if let Some(message) = parsed.get("message").and_then(|v| v.as_object()) {
        if let Some(content) = message.get("content").and_then(|v| v.as_str()) {
            if !content.is_empty() {
                return Some(content.to_string());
            }
        }
    }
    None
}

/// Try to pull an `error` field out of an error body, like the Python code.
fn error_detail(body: &str) -> Option<String> {
    if body.is_empty() {
        return None;
    }
    let parsed: Value = serde_json::from_str(body).ok()?;
    match parsed.get("error") {
        Some(Value::String(s)) => Some(s.clone()),
        Some(other) => Some(other.to_string()),
        None => None,
    }
}

/// Fallback detail when the body isn't structured: return it verbatim, or
/// the HTTP status when the body is empty.
fn error_body_or_detail(body: &str, code: u16) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        format!("HTTP {}", code)
    } else {
        trimmed.to_string()
    }
}

/// SHA-256 hex digest of bytes.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut hasher, bytes);
    let digest = sha2::Digest::finalize(hasher);
    let mut out = String::with_capacity(64);
    for b in digest.iter() {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

/// Parse the hostname out of a URL string.
fn url_host(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
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
    fn sha256_matches_known_digest() {
        // sha256("abc") == ba7816bf...
        assert!(sha256_hex(b"abc").starts_with("ba7816bf8f01cfea414140de5dae2223"));
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
    fn extracts_chat_message_content() {
        let parsed = json!({"message": {"role": "assistant", "content": "the cow weighs 400 kg"}});
        assert_eq!(
            extract_text(&parsed).as_deref(),
            Some("the cow weighs 400 kg")
        );
    }

    #[test]
    fn error_detail_prefers_error_field() {
        assert_eq!(error_detail(r#"{"error": "bad key"}"#).as_deref(), Some("bad key"));
        assert_eq!(error_detail("raw text"), None);
    }

    #[test]
    fn error_body_or_detail_falls_back_to_status() {
        assert!(error_body_or_detail("", 500).contains("500"));
        assert_eq!(error_body_or_detail("boom", 500), "boom");
    }
}
