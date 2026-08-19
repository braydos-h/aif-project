//! Weight extraction from model output.
//!
//! Mirrors `CowWeightEstimator._parse_structured_response` /
//! `_extract_weight_from_text` in `aif/estimator.py`: structured JSON first
//! (`{"weight_kg": ...}` plus optional extras), then free-text extraction
//! (prefers `<n> kg`, then the first bare number).

use serde_json::Value;

/// Extra fields returned alongside the weight.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Extras {
    pub confidence: Option<f64>,
    pub breed: Option<String>,
    pub body_condition_score: Option<f64>,
}

/// Pull a weight + extras out of the model's reply.
///
/// Returns `None` when no weight can be extracted.
pub fn parse_structured_response(text: &str) -> Option<(f64, Extras)> {
    // Find the first {...} block in the text (no nesting — same as the
    // Python regex `\{[^{}]*\}`).
    let start = find_balanced(text);
    if let Some((open, close)) = start {
        let candidate = &text[open..=close];
        if let Ok(value) = serde_json::from_str::<Value>(candidate) {
            if let Some(obj) = value.as_object() {
                if let Some(weight) = obj.get("weight_kg") {
                    if let Some(weight_kg) = as_f64(weight) {
                        let mut extras = Extras::default();
                        if let Some(c) = obj.get("confidence") {
                            extras.confidence = as_f64(c);
                        }
                        if let Some(b) = obj.get("breed") {
                            if let Some(s) = b.as_str() {
                                extras.breed = Some(s.to_string());
                            }
                        }
                        if let Some(bcs) = obj.get("body_condition_score") {
                            extras.body_condition_score = as_f64(bcs);
                        }
                        return Some((weight_kg, extras));
                    }
                }
            }
        }
    }
    // No usable JSON — fall back to text extraction.
    extract_weight_from_text(text).map(|w| (w, Extras::default()))
}

/// Find the first `{...}` block with no nesting inside. Returns byte offsets
/// of the opening and closing brace, or None.
fn find_balanced(text: &str) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut open = None;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'{' && open.is_none() {
            open = Some(i);
        } else if b == b'}' && open.is_some() {
            return Some((open.unwrap(), i));
        }
    }
    None
}

/// Convert a JSON value to f64 if it's a number or a numeric string.
fn as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// Pull a weight in kilograms out of free-form model output.
///
/// Prefers an explicit "<number> kg" (case-insensitive), then falls back to
/// the first bare number. Mirrors `_extract_weight_from_text`.
pub fn extract_weight_from_text(text: &str) -> Option<f64> {
    // Prefer "<number> kg" (case-insensitive). Search for "kg" then scan
    // backwards for the number — this matches the Python regex
    // `(\d+(?:\.\d+)?)\s*kg` for typical output.
    let lower = text.to_ascii_lowercase();
    let mut search_from = 0;
    while let Some(kg_idx) = lower[search_from..].find("kg") {
        let kg_idx = search_from + kg_idx;
        // Skip whitespace backwards.
        let mut end = kg_idx;
        while end > 0 && text.as_bytes()[end - 1].is_ascii_whitespace() {
            end -= 1;
        }
        if let Some(num_start) = number_before(text, end) {
            return Some(parse_number(&text[num_start..end]));
        }
        search_from = kg_idx + 2;
    }
    // Fall back to the first bare number.
    first_number(text)
}

/// If `text[..end]` ends with a decimal number, return the start offset.
fn number_before(text: &str, end: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut i = end;
    if i == 0 {
        return None;
    }
    // Fractional part.
    let mut seen_dot = false;
    while i > 0 {
        let b = bytes[i - 1];
        if b.is_ascii_digit() {
            i -= 1;
        } else if b == b'.' && !seen_dot {
            seen_dot = true;
            i -= 1;
        } else {
            break;
        }
    }
    if i == end {
        return None;
    }
    // Guard: a lone "." before digits? Reject if the scan consumed only a dot.
    if seen_dot && end - i == 1 {
        return None;
    }
    Some(i)
}

/// Parse a plain decimal number (integer or with a fractional part).
fn parse_number(s: &str) -> f64 {
    s.parse::<f64>().unwrap_or(0.0)
}

/// Find the first bare number in the text. Mirrors `\d+(?:\.\d+)?`.
fn first_number(text: &str) -> Option<f64> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'.' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_digit() {
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
            }
            return parse_number(&text[start..i]).into();
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_structured_json() {
        let text = r#"Some preamble {"weight_kg": 612, "confidence": 0.82, "breed": "Angus", "body_condition_score": 6} trailing"#;
        let (weight, extras) = parse_structured_response(text).unwrap();
        assert_eq!(weight, 612.0);
        assert_eq!(extras.confidence, Some(0.82));
        assert_eq!(extras.breed.as_deref(), Some("Angus"));
        assert_eq!(extras.body_condition_score, Some(6.0));
    }

    #[test]
    fn string_weight_value_parsed() {
        let (weight, _) = parse_structured_response(r#"{"weight_kg": "500"}"#).unwrap();
        assert_eq!(weight, 500.0);
    }

    #[test]
    fn falls_back_to_kg_text() {
        let (weight, extras) = parse_structured_response("The cow weighs 450 kg roughly").unwrap();
        assert_eq!(weight, 450.0);
        assert_eq!(extras, Extras::default());
    }

    #[test]
    fn kg_match_is_case_insensitive() {
        assert_eq!(extract_weight_from_text("550 KG"), Some(550.0));
        assert_eq!(extract_weight_from_text("about 321.5 Kg"), Some(321.5));
    }

    #[test]
    fn first_bare_number_fallback() {
        assert_eq!(extract_weight_from_text("answer: 88"), Some(88.0));
        assert_eq!(extract_weight_from_text("no numbers"), None);
    }

    #[test]
    fn nested_braces_rejected_like_python_regex() {
        // Python's `\{[^{}]*\}` matches the innermost block; a nested block
        // means the first "{" to the first "}" — mirrors that behavior.
        let text = r#"{"weight_kg": 5, "meta": {"x": 1}}"#;
        // Our scanner stops at the first '}', giving `{"weight_kg": 5, "meta": {`...
        // which is invalid JSON, so it falls back to text extraction → 5.
        let (weight, _) = parse_structured_response(text).unwrap();
        assert_eq!(weight, 5.0);
    }

    #[test]
    fn decimal_kg_preferred_over_bare_number() {
        // "730" appears before "730.5 kg"; the kg rule must win.
        assert_eq!(extract_weight_from_text("around 730 in 730.5 kg range"), Some(730.5));
    }
}
