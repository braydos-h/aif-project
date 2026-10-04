//! HTTP transport for the Ollama backend with retry policy.
//!
//! Sends the JSON payload via `ureq` and maps transport/HTTP failures to
//! [`OllamaError`]. Retries once on 5xx, network/timeout errors, and
//! response-read failures after a backoff; 4xx errors are not retried.

use std::time::Duration;

use crate::config::{OLLAMA_MAX_RETRIES, OLLAMA_RETRY_BACKOFF_SECS};

use super::response::{error_body_or_detail, error_detail};
use super::OllamaError;

/// Max Ollama response body: model replies are short JSON/text; anything
/// larger is a misbehaving upstream, not an estimate.
pub(crate) const MAX_OLLAMA_BODY_BYTES: usize = 5 * 1024 * 1024;

/// Read a response body with a cap, so a compromised/misbehaving upstream
/// cannot OOM the worker with an unbounded reply.
fn read_capped_body(response: ureq::Response) -> Result<String, String> {
    use std::io::Read;
    let mut buf: Vec<u8> = Vec::new();
    response
        .into_reader()
        .take((MAX_OLLAMA_BODY_BYTES + 1) as u64)
        .read_to_end(&mut buf)
        .map_err(|e| format!("Failed to read Ollama response: {}", e))?;
    if buf.len() > MAX_OLLAMA_BODY_BYTES {
        return Err("Ollama response exceeded 5 MiB".to_string());
    }
    String::from_utf8(buf).map_err(|e| format!("Ollama response was not UTF-8: {}", e))
}

/// POST `payload` to `url` with an optional bearer key, retrying transient
/// failures. Returns the raw response body on success.
pub(crate) fn post_with_retry(
    url: &str,
    api_key: Option<&str>,
    payload: &str,
) -> Result<String, OllamaError> {
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(60))
        .redirects(0)
        .build();

    let mut last_error: Option<OllamaError> = None;
    for attempt in 0..=OLLAMA_MAX_RETRIES {
        let mut request = agent.post(url).set("Content-Type", "application/json");
        if let Some(key) = api_key {
            request = request.set("Authorization", &format!("Bearer {}", key));
        }
        match request.send_string(payload) {
            Ok(response) => match read_capped_body(response) {
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
                // Error bodies are capped like success bodies; an oversize
                // error falls back to the status text.
                let error_body = read_capped_body(response).unwrap_or_default();
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
