//! Ollama response parsing and error-body helpers.

use serde_json::Value;

/// Extract the model text from a parsed response: `response` field, or
/// `message.content` when a chat-style object is present.
pub(crate) fn extract_text(parsed: &Value) -> Option<String> {
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
pub(crate) fn error_detail(body: &str) -> Option<String> {
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
pub(crate) fn error_body_or_detail(body: &str, code: u16) -> String {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        format!("HTTP {}", code)
    } else {
        trimmed.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        assert_eq!(
            error_detail(r#"{"error": "bad key"}"#).as_deref(),
            Some("bad key")
        );
        assert_eq!(error_detail("raw text"), None);
    }

    #[test]
    fn error_body_or_detail_falls_back_to_status() {
        assert!(error_body_or_detail("", 500).contains("500"));
        assert_eq!(error_body_or_detail("boom", 500), "boom");
    }
}
