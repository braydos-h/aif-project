//! Structured `{"weight_kg": ...}` extraction from model output.
//!
//! Finds the first `{...}` block with no nesting (mirroring Python
//! `re.search(r"\{[^{}]*\}")`) and pulls `weight_kg` plus optional extras.

use serde_json::Value;

use super::Extras;

/// Convert a JSON value to f64 if it's a number or a numeric string.
fn as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        _ => None,
    }
}

/// Find the first `{...}` block containing no inner braces, mirroring
/// Python `re.search(r"\{[^{}]*\}")`. Returns byte offsets of the braces.
fn find_first_braceless_block(text: &str) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            if let Some(rel) = bytes[i + 1..].iter().position(|&b| b == b'}') {
                let close = i + 1 + rel;
                if !bytes[i + 1..close].contains(&b'{') {
                    return Some((i, close));
                }
            }
        }
        i += 1;
    }
    None
}

/// Try to parse the first `{...}` block as a weight object.
/// Returns `None` when there is no block or no usable `weight_kg`.
pub(crate) fn parse_json_block(text: &str) -> Option<(f64, Extras)> {
    let (open, close) = find_first_braceless_block(text)?;
    let candidate = &text[open..=close];
    let value: Value = serde_json::from_str(candidate).ok()?;
    let obj = value.as_object()?;
    let weight_kg = as_f64(obj.get("weight_kg")?)?;
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
    Some((weight_kg, extras))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_weight_and_extras_from_block() {
        let text = r#"Some preamble {"weight_kg": 612, "confidence": 0.82, "breed": "Angus", "body_condition_score": 6} trailing"#;
        let (weight, extras) = parse_json_block(text).unwrap();
        assert_eq!(weight, 612.0);
        assert_eq!(extras.confidence, Some(0.82));
        assert_eq!(extras.breed.as_deref(), Some("Angus"));
        assert_eq!(extras.body_condition_score, Some(6.0));
    }

    #[test]
    fn string_weight_value_parsed() {
        let (weight, _) = parse_json_block(r#"{"weight_kg": "500"}"#).unwrap();
        assert_eq!(weight, 500.0);
    }

    #[test]
    fn missing_weight_returns_none() {
        assert!(parse_json_block(r#"{"confidence": 0.5}"#).is_none());
        assert!(parse_json_block("no braces here").is_none());
    }
}
