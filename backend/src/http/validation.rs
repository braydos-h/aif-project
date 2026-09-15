//! Runtime option validation for the estimator API.
//!
//! Optional request fields (`backend`, `model`, `ollama_url`,
//! `ollama_api_key`, `prompt`) are type/size/allow-list validated before
//! use. A request-local [`Config`] clone carries the overrides; shared
//! server state is never mutated.

use serde_json::Value;

use crate::config::{Config, DEFAULT_OLLAMA_URL};

pub(crate) const MAX_BODY_BYTES: usize = 20 * 1024 * 1024;
pub(crate) const MAX_PROMPT_BYTES: usize = 16 * 1024;
pub(crate) const MAX_MODEL_BYTES: usize = 256;
pub(crate) const MAX_URL_BYTES: usize = 2 * 1024;
pub(crate) const MAX_API_KEY_BYTES: usize = 4 * 1024;

pub(crate) fn optional_string<'a>(
    payload: &'a Value,
    field: &str,
) -> Result<Option<&'a str>, String> {
    match payload.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .map(Some)
            .ok_or_else(|| format!("{} must be a string", field)),
    }
}

/// Optional numeric field: accepts a JSON number only (no strings/bools).
/// Returns `Ok(None)` when absent or null so old clients keep working.
pub(crate) fn optional_number(payload: &Value, field: &str) -> Result<Option<f64>, String> {
    match payload.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n
            .as_f64()
            .map(Some)
            .ok_or_else(|| format!("{} must be a number", field)),
        Some(_) => Err(format!("{} must be a number", field)),
    }
}

pub(crate) fn has_control_or_whitespace(value: &str) -> bool {
    value
        .bytes()
        .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
}

pub(crate) fn valid_http_url(url: &str) -> bool {
    if url.is_empty()
        || url.len() > MAX_URL_BYTES
        || has_control_or_whitespace(url)
        || url.contains('@')
        || url.contains('?')
        || url.contains('#')
    {
        return false;
    }
    let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or("");
    let host = authority.split(':').next().unwrap_or("");
    !host.is_empty()
}

pub(crate) fn validate_runtime_config(config: &Config, prompt: &str) -> Result<(), String> {
    if config.backend != "ollama" && config.backend != "none" {
        return Err("Unsupported backend. Choose ollama or none.".to_string());
    }
    if config.model.is_empty()
        || config.model.len() > MAX_MODEL_BYTES
        || has_control_or_whitespace(&config.model)
    {
        return Err("Model must be a non-empty single-line value under 256 bytes.".to_string());
    }
    if !valid_http_url(&config.ollama_url) {
        return Err(
            "Ollama URL must be an http(s) URL without credentials or query parameters."
                .to_string(),
        );
    }
    if let Some(key) = &config.ollama_api_key {
        if key.len() > MAX_API_KEY_BYTES || key.bytes().any(|byte| byte.is_ascii_control()) {
            return Err("API key is too long or contains invalid control characters.".to_string());
        }
    }
    if prompt.len() > MAX_PROMPT_BYTES {
        return Err("Prompt is too long; keep it under 16 KiB.".to_string());
    }
    Ok(())
}

pub(crate) fn safe_url_for_info(url: &str) -> String {
    if valid_http_url(url) {
        url.to_string()
    } else {
        DEFAULT_OLLAMA_URL.to_string()
    }
}

pub(crate) fn redact_secret(message: &str, configs: [&Config; 2]) -> String {
    let mut redacted = message.to_string();
    for config in configs {
        if let Some(key) = &config.ollama_api_key {
            if !key.is_empty() {
                redacted = redacted.replace(key, "[redacted]");
            }
        }
    }
    redacted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_routes_are_explicit_and_options_are_validated() {
        assert!(valid_http_url("https://ollama.com/api/generate"));
        assert!(valid_http_url("http://127.0.0.1:11434/api/generate"));
        assert!(!valid_http_url("file:///secret"));
        assert!(!valid_http_url("https://user:secret@example.com/api"));
        assert!(!valid_http_url("https://example.com/api?key=secret"));
    }
}
