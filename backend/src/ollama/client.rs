//! HTTP transport for the Ollama backend with retry policy.
//!
//! Sends the JSON payload via `ureq` and maps transport/HTTP failures to
//! [`OllamaError`]. Retries once on 5xx, network/timeout errors, and
//! response-read failures after a backoff; 4xx errors are not retried.

use std::time::Duration;

use crate::config::{OLLAMA_MAX_RETRIES, OLLAMA_RETRY_BACKOFF_SECS};

use super::response::{error_body_or_detail, error_detail};
use super::OllamaError;

/// POST `payload` to `url` with an optional bearer key, retrying transient
/// failures. Returns the raw response body on success.
pub(crate) fn post_with_retry(
    url: &str,
    api_key: Option<&str>,
    payload: &str,
) -> Result<String, OllamaError> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .build();

    let mut last_error: Option<OllamaError> = None;
    for attempt in 0..=OLLAMA_MAX_RETRIES {
        let mut request = agent.post(url).set("Content-Type", "application/json");
        if let Some(key) = api_key {
            request = request.set("Authorization", &format!("Bearer {}", key));
        }
        match request.send_string(payload) {
            Ok(response) => match response.into_string() {
                Ok(body) => return Ok(body),
                Err(e) => {
                    last_error = Some(OllamaError(format!(
                        "Failed to read Ollama response: {}",
                        e
                    )));
                    if attempt == OLLAMA_MAX_RETRIES {
                        return Err(last_error.unwrap());
                    }
                    eprintln!(
                        "Ollama response read failed ({}), retrying in {}s (attempt {}/{})",
                        e,
                        OLLAMA_RETRY_BACKOFF_SECS,
                        attempt + 1,
                        OLLAMA_MAX_RETRIES
                    );
                    std::thread::sleep(Duration::from_secs(OLLAMA_RETRY_BACKOFF_SECS));
                }
            },
            Err(ureq::Error::Status(code, response)) => {
                let error_body = response.into_string().unwrap_or_default();
                let detail = error_detail(&error_body)
                    .unwrap_or_else(|| error_body_or_detail(&error_body, code));
                last_error = Some(OllamaError(format!(
                    "Ollama request failed (HTTP {}): {}",
                    code, detail
                )));
                if code < 500 || attempt == OLLAMA_MAX_RETRIES {
                    return Err(last_error.unwrap());
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
                    url, t
                )));
                if attempt == OLLAMA_MAX_RETRIES {
                    return Err(last_error.unwrap());
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

    Err(last_error.unwrap_or(OllamaError("Ollama call failed".to_string())))
}
