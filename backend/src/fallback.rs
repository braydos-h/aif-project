//! Deterministic local fallback backend.
//!
//! Mirrors `CowWeightEstimator._estimate_fallback`: hash the image reference
//! to a stable weight in the range 250–900 kg. Never touches the network.

use serde_json::{Value, json};

use crate::config::{kg_to_lbs, round1};

/// Build the fallback estimate dict for an image reference.
///
/// The digest formula must match `aif/estimator.py`:
/// `int(sha256(reference)[:8], 16) / 0xFFFFFFFF` → 250 + ratio * 650.
pub fn estimate_fallback(image_reference: &str, prompt: &str) -> Value {
    let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut hasher, image_reference.as_bytes());
    let digest = sha2::Digest::finalize(hasher);
    let first_8 = &digest[..8];
    let raw: u32 = u32::from_be_bytes(first_8.try_into().unwrap());
    let normalized = raw as f64 / u32::MAX as f64;
    let weight_kg = round1(250.0 + normalized * 650.0);

    json!({
        "estimated_weight_kg": weight_kg,
        "estimated_weight_lbs": kg_to_lbs(weight_kg),
        "source": "local_fallback",
        "prompt_used": prompt,
        "model_response": "",
    })
}

/// Build a result Value from structured parts (shared by backends).
pub fn result_with_extras(
    weight_kg: f64,
    prompt: &str,
    model_response: &str,
    extras: &crate::parse::Extras,
) -> Value {
    let mut obj = json!({
        "estimated_weight_kg": weight_kg,
        "estimated_weight_lbs": kg_to_lbs(weight_kg),
        "prompt_used": prompt,
        "model_response": model_response,
    });
    if let Some(c) = extras.confidence {
        obj["confidence"] = Value::from(c);
    }
    if let Some(b) = &extras.breed {
        obj["breed"] = Value::from(b.clone());
    }
    if let Some(s) = extras.body_condition_score {
        obj["body_condition_score"] = Value::from(s);
    }
    obj
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_is_deterministic_and_in_range() {
        let a = estimate_fallback("https://example.com/cow.jpg", "p");
        let b = estimate_fallback("https://example.com/cow.jpg", "p");
        assert_eq!(a, b);
        let kg = a["estimated_weight_kg"].as_f64().unwrap();
        assert!((250.0..=900.0).contains(&kg));
        assert_eq!(a["source"], "local_fallback");
        assert_eq!(a["prompt_used"], "p");
        assert_eq!(a["model_response"], "");
    }
}
