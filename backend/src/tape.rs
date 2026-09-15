//! Tape-measure estimator (Schaeffer's formula) for field use.
//!
//! When a scale is unavailable, heart girth + body length give a useful
//! offline cross-check with no network call and no image needed:
//! `weight_lbs = girth_in^2 * length_in / 300`, converted here to metric as
//! `weight_kg = girth_cm^2 * length_cm / 10838`.
//!
//! Tape results use `source == "tape_measure"` and a narrower ±5% range than
//! the ±10% photo range, since a careful tape measurement is typically
//! tighter than a single-photo AI guess. Every result also carries the
//! dosing disclaimer: never dose medication from an estimate.

use serde_json::{json, Value};

use crate::config::{kg_to_lbs, round1};

/// Divisor for the metric Schaeffer formula (cm → kg).
/// `300 * 2.54^3 * 2.20462 ≈ 10838`.
pub const TAPE_DIVISOR: f64 = 10838.0;
/// Plausible tape bounds in centimetres (calf through large bull).
pub const MIN_TAPE_CM: f64 = 50.0;
pub const MAX_TAPE_CM: f64 = 300.0;
/// Tape range half-width (±5%) and photo range half-width (±10%).
pub const TAPE_RANGE_FRACTION: f64 = 0.05;
pub const PHOTO_RANGE_FRACTION: f64 = 0.10;

/// Dosing disclaimer attached to every successful estimate.
pub const DISCLAIMER: &str =
    "Estimate only — verify with a scale. Do not dose medication from this estimate.";

/// Schaeffer weight in kg from girth/length in cm, rounded to 1 decimal.
pub fn tape_weight_kg(heart_girth_cm: f64, body_length_cm: f64) -> f64 {
    round1(heart_girth_cm * heart_girth_cm * body_length_cm / TAPE_DIVISOR)
}

/// Symmetric range around `weight_kg` with the given half-width fraction.
pub fn weight_range_kg(weight_kg: f64, fraction: f64) -> (f64, f64) {
    (
        round1(weight_kg * (1.0 - fraction)),
        round1(weight_kg * (1.0 + fraction)),
    )
}

/// Validate one tape measurement; returns a human message on failure.
pub fn validate_tape_measure(value: f64, field: &str) -> Result<(), String> {
    if !value.is_finite() {
        return Err(format!("{} must be a number.", field));
    }
    if !(MIN_TAPE_CM..=MAX_TAPE_CM).contains(&value) {
        return Err(format!(
            "{} must be between {} and {} cm.",
            field, MIN_TAPE_CM as u32, MAX_TAPE_CM as u32
        ));
    }
    Ok(())
}

/// Build the tape-measure result dict (no network, no image needed).
pub fn estimate_tape(heart_girth_cm: f64, body_length_cm: f64, prompt: &str) -> Value {
    let weight_kg = tape_weight_kg(heart_girth_cm, body_length_cm);
    let (min_kg, max_kg) = weight_range_kg(weight_kg, TAPE_RANGE_FRACTION);
    json!({
        "estimated_weight_kg": weight_kg,
        "estimated_weight_lbs": kg_to_lbs(weight_kg),
        "weight_min_kg": min_kg,
        "weight_max_kg": max_kg,
        "weight_min_lbs": kg_to_lbs(min_kg),
        "weight_max_lbs": kg_to_lbs(max_kg),
        "source": "tape_measure",
        "method": "schaeffer_tape",
        "heart_girth_cm": heart_girth_cm,
        "body_length_cm": body_length_cm,
        "prompt_used": prompt,
        "model_response": "",
        "model": null,
        "confidence": null,
        "breed": null,
        "body_condition_score": null,
        "disclaimer": DISCLAIMER,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_tape_vector_matches_hand_calc() {
        // 180cm girth × 150cm length → 180²×150/10838 ≈ 448.4 kg.
        assert_eq!(tape_weight_kg(180.0, 150.0), 448.4);
        assert_eq!(kg_to_lbs(448.4), 988.6);
    }

    #[test]
    fn tape_range_is_five_percent() {
        let (min, max) = weight_range_kg(400.0, TAPE_RANGE_FRACTION);
        assert_eq!((min, max), (380.0, 420.0));
    }

    #[test]
    fn photo_range_is_ten_percent() {
        let (min, max) = weight_range_kg(500.0, PHOTO_RANGE_FRACTION);
        assert_eq!((min, max), (450.0, 550.0));
    }

    #[test]
    fn rejects_out_of_range_measure() {
        assert!(validate_tape_measure(180.0, "heart_girth_cm").is_ok());
        assert!(validate_tape_measure(20.0, "heart_girth_cm").is_err());
        assert!(validate_tape_measure(f64::NAN, "heart_girth_cm").is_err());
    }

    #[test]
    fn tape_result_schema() {
        let v = estimate_tape(180.0, 150.0, "p");
        assert_eq!(v["source"], "tape_measure");
        assert_eq!(v["method"], "schaeffer_tape");
        assert_eq!(v["estimated_weight_kg"], 448.4);
        assert_eq!(v["heart_girth_cm"], 180.0);
        assert!(v["disclaimer"].as_str().unwrap().contains("Do not dose"));
    }
}
