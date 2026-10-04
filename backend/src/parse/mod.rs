//! Weight extraction from model output.
//!
//! Structured JSON first (`{"weight_kg": ...}` plus optional extras), then
//! free-text extraction (prefers `<n> kg`, then the first bare number).
//!
//! Split into focused submodules:
//! - [`structured`] — first `{...}` JSON block parsing.
//! - [`text`] — free-text `<n> kg` / bare-number fallback.

pub mod structured;
pub mod text;

pub use text::extract_weight_from_text;

/// Extra fields returned alongside the weight.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Extras {
    pub confidence: Option<f64>,
    pub breed: Option<String>,
    pub body_condition_score: Option<f64>,
}

/// Plausible cattle weight bounds in kilograms.
///
/// Model output outside this range (negative, absurd, non-finite) is treated
/// as unparseable at the call site rather than returned to clients.
pub const MIN_PLAUSIBLE_WEIGHT_KG: f64 = 20.0;
/// Upper bound for [`valid_weight_kg`] (heaviest bulls plus margin).
pub const MAX_PLAUSIBLE_WEIGHT_KG: f64 = 2500.0;

/// True for finite weights inside the plausible cattle range.
pub fn valid_weight_kg(weight_kg: f64) -> bool {
    weight_kg.is_finite()
        && (MIN_PLAUSIBLE_WEIGHT_KG..=MAX_PLAUSIBLE_WEIGHT_KG).contains(&weight_kg)
}

/// Pull a weight + extras out of the model's reply.
///
/// Returns `None` when no weight can be extracted.
pub fn parse_structured_response(text: &str) -> Option<(f64, Extras)> {
    // Structured JSON first; fall back to free-text extraction.
    if let Some(parsed) = structured::parse_json_block(text) {
        return Some(parsed);
    }
    extract_weight_from_text(text).map(|w| (w, Extras::default()))
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
    fn falls_back_to_kg_text() {
        let (weight, extras) = parse_structured_response("The cow weighs 450 kg roughly").unwrap();
        assert_eq!(weight, 450.0);
        assert_eq!(extras, Extras::default());
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
    fn nested_json_matches_python_innermost_block() {
        // Python: re.search(r"\{[^{}]*\}") finds the inner block -> 600.0
        let (weight, _) =
            parse_structured_response(r#"{"weight_kg": 100, "nested": {"weight_kg": 600}}"#)
                .unwrap();
        assert_eq!(weight, 600.0);
    }

    #[test]
    fn implausible_weights_are_rejected() {
        assert!(valid_weight_kg(20.0));
        assert!(valid_weight_kg(612.0));
        assert!(valid_weight_kg(2500.0));
        for bad in [
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            -5.0,
            0.0,
            19.9,
            2500.1,
            1e12,
        ] {
            assert!(!valid_weight_kg(bad), "expected reject for {}", bad);
        }
    }
}
