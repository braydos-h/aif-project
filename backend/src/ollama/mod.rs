//! Ollama Cloud backend: POST the image (base64) + prompt, retry once on
//! transient failures, parse the reply.
//!
//! Mirrors `CowWeightEstimator._estimate_via_ollama` /
//! `_call_ollama_with_retry` in `aif/estimator.py`:
//! - API key required when the URL host is `ollama.com`, sent as a Bearer token.
//! - Retries once on 5xx and network/timeout errors, after a 1 s backoff.
//! - 4xx errors and non-JSON bodies are not retried.
//!
//! Split into focused submodules:
//! - [`client`] — HTTP POST with retry policy.
//! - [`response`] — model-text extraction and error-body helpers.
//! - [`cache_key`] — request-scoped cache keys and URL host parsing.

pub mod cache_key;
pub mod client;
pub mod response;

use serde_json::{json, Value};

use crate::config::Config;
use crate::fallback::result_with_extras;
use crate::parse::parse_structured_response;
use crate::validate::to_base64_image;

/// Re-exported for backwards compatibility; new code should use
/// [`crate::hash::sha256_hex`].
pub use crate::hash::sha256_hex;

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
    let host = cache_key::url_host(&config.ollama_url);
    if host.as_deref() == Some("ollama.com") && config.ollama_api_key.is_none() {
        return Err(Box::new(OllamaError(
            "Ollama Cloud requires an API key. Set OLLAMA_API_KEY in .env \
             to an API key created at https://ollama.com/settings/keys."
                .to_string(),
        )));
    }

    let image_b64 = to_base64_image(image_reference)?;
    // Runtime prompt/model/URL overrides must not reuse a result produced by
    // a different request configuration.
    let key = cache_key::cache_key(&image_b64, &config.model, &config.ollama_url, prompt);
    if let Some(cached) = cache.get(&key) {
        eprintln!("cache hit for image {}", &key[..12]);
        return Ok(cached);
    }

    let payload = json!({
        "model": config.model,
        "prompt": prompt,
        "images": [image_b64],
        "stream": false,
    });
    let body = client::post_with_retry(
        &config.ollama_url,
        config.ollama_api_key.as_deref(),
        &payload.to_string(),
    )?;

    let parsed: Value = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&body)
            .map_err(|e| Box::new(OllamaError(format!("Ollama returned non-JSON body: {}", e))))?
    };
    let text = response::extract_text(&parsed);
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
    cache.put(&key, result.clone());
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_known_digest() {
        // sha256("abc") == ba7816bf...
        assert!(sha256_hex(b"abc").starts_with("ba7816bf8f01cfea414140de5dae2223"));
    }
}
