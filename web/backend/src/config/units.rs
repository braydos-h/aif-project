//! Unit conversion and rounding helpers for weight results.

/// Kilograms-to-pounds factor.
pub const KG_TO_LBS: f64 = 2.20462;

/// Convert kilograms to pounds, rounded to one decimal place.
pub fn kg_to_lbs(kg: f64) -> f64 {
    round1(kg * KG_TO_LBS)
}

/// Round a float to one decimal place.
pub fn round1(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kg_to_lbs_rounds_to_one_decimal() {
        assert_eq!(kg_to_lbs(612.0), 1349.2);
        assert_eq!(kg_to_lbs(0.0), 0.0);
    }
}
