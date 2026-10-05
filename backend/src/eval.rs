//! Reference-dataset evaluation for estimate quality (roadmap item 18).
//!
//! Compares photo and tape estimates against contemporaneous scale weights
//! from a permissioned reference set, reporting absolute/percentage error,
//! bias, worst case, repeatability, and implausible-value counts. Every
//! report stamps the provider, model, prompt version, and estimator
//! version so model/prompt changes get a repeatable before/after check.
//!
//! Calibration rows and evaluation rows are kept separate by the dataset
//! (`split` field); metrics are computed on the evaluation split only.
//! Ranges are reported as estimates — never as statistically calibrated
//! confidence — unless a real dataset supports that claim.

use serde::{Deserialize, Serialize};

/// One reference row: a scale-weighed animal with optional tape
/// measurements and an optional photo.
#[derive(Debug, Clone, Deserialize)]
pub struct ReferenceRow {
    /// Stable row id within the dataset.
    pub id: String,
    /// Verified scale weight in kilograms (ground truth).
    pub scale_weight_kg: f64,
    /// Heart girth in centimetres (Schaeffer input).
    pub heart_girth_cm: Option<f64>,
    /// Body length in centimetres (Schaeffer input).
    pub body_length_cm: Option<f64>,
    /// Base64/data-URI image, or a path relative to the dataset file.
    pub image: Option<String>,
    /// `calibration` rows tune prompts; `evaluation` rows score them.
    #[serde(default = "default_split")]
    pub split: String,
    /// True for synthetic/demonstration rows (never real cattle data).
    #[serde(default)]
    pub synthetic: bool,
    /// Optional metadata: breed, age, pose, lighting, notes.
    #[serde(default)]
    pub breed: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

fn default_split() -> String {
    "evaluation".to_string()
}

/// A reference dataset file.
#[derive(Debug, Deserialize)]
pub struct Dataset {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub acceptance_mape_pct: Option<f64>,
    #[serde(default)]
    pub acceptance_bias_pct: Option<f64>,
    pub rows: Vec<ReferenceRow>,
}

/// Per-method error summary over the evaluation split.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct MethodMetrics {
    /// Number of evaluated rows.
    pub n: usize,
    /// Mean absolute error (kg).
    pub mae_kg: f64,
    /// Mean absolute percentage error (%).
    pub mape_pct: f64,
    /// Mean signed error (kg, + means estimates run heavy).
    pub bias_kg: f64,
    /// Mean signed percentage error (%).
    pub bias_pct: f64,
    /// Worst absolute error (kg) and the row that produced it.
    pub max_abs_kg: f64,
    pub max_abs_row: String,
    /// Worst percentage error (%) and the row that produced it.
    pub max_pct: f64,
    pub max_pct_row: String,
    /// Fraction of rows within ±10% of the scale weight.
    pub within_10_pct: f64,
    /// Fraction of rows within ±20% of the scale weight.
    pub within_20_pct: f64,
    /// Rows whose estimate fell outside the plausible cattle range.
    pub implausible: usize,
}

/// Compute [`MethodMetrics`] from (row id, scale kg, estimate kg) triples.
/// Estimates that are non-finite count as implausible and are skipped.
pub fn summarize(triples: &[(String, f64, f64)]) -> MethodMetrics {
    let mut n = 0usize;
    let mut abs_sum = 0.0;
    let mut pct_sum = 0.0;
    let mut signed_sum = 0.0;
    let mut signed_pct_sum = 0.0;
    let mut within_10 = 0usize;
    let mut within_20 = 0usize;
    let mut implausible = 0usize;
    let mut max_abs_kg = 0.0;
    let mut max_abs_row = String::new();
    let mut max_pct = 0.0;
    let mut max_pct_row = String::new();
    for (id, scale, estimate) in triples {
        if !estimate.is_finite()
            || !(crate::parse::MIN_PLAUSIBLE_WEIGHT_KG..=crate::parse::MAX_PLAUSIBLE_WEIGHT_KG)
                .contains(estimate)
        {
            implausible += 1;
            continue;
        }
        let err = (estimate - scale).abs();
        let pct = if *scale > 0.0 { err / scale * 100.0 } else { 0.0 };
        n += 1;
        abs_sum += err;
        pct_sum += pct;
        signed_sum += estimate - scale;
        signed_pct_sum += (estimate - scale) / scale * 100.0;
        if pct <= 10.0 {
            within_10 += 1;
        }
        if pct <= 20.0 {
            within_20 += 1;
        }
        if err > max_abs_kg {
            max_abs_kg = err;
            max_abs_row = id.clone();
        }
        if pct > max_pct {
            max_pct = pct;
            max_pct_row = id.clone();
        }
    }
    let denom = n.max(1) as f64;
    MethodMetrics {
        n,
        mae_kg: round3(abs_sum / denom),
        mape_pct: round3(pct_sum / denom),
        bias_kg: round3(signed_sum / denom),
        bias_pct: round3(signed_pct_sum / denom),
        max_abs_kg: round3(max_abs_kg),
        max_abs_row,
        max_pct: round3(max_pct),
        max_pct_row,
        within_10_pct: round3(within_10 as f64 / denom),
        within_20_pct: round3(within_20 as f64 / denom),
        implausible,
    }
}

fn round3(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// Evaluation-split rows only (calibration rows never score).
pub fn evaluation_rows(dataset: &Dataset) -> Vec<&ReferenceRow> {
    dataset
        .rows
        .iter()
        .filter(|r| r.split == "evaluation" && r.scale_weight_kg.is_finite() && r.scale_weight_kg > 0.0)
        .collect()
}

/// Validate dataset shape before running: finite scale weights, plausible
/// tape ranges, known splits, and at least one evaluation row.
pub fn validate_dataset(dataset: &Dataset) -> Result<(), String> {
    if dataset.rows.is_empty() {
        return Err("dataset has no rows".to_string());
    }
    for row in &dataset.rows {
        if row.id.is_empty() || row.id.len() > 64 {
            return Err(format!("row has a bad id: {:?}", row.id));
        }
        if !row.scale_weight_kg.is_finite()
            || !(20.0..=2500.0).contains(&row.scale_weight_kg)
        {
            return Err(format!("row {} has an implausible scale weight", row.id));
        }
        if row.split != "evaluation" && row.split != "calibration" {
            return Err(format!("row {} has an unknown split {:?}", row.id, row.split));
        }
        for (label, value) in [
            ("heart_girth_cm", row.heart_girth_cm),
            ("body_length_cm", row.body_length_cm),
        ] {
            if let Some(v) = value {
                if !v.is_finite() || !(50.0..=300.0).contains(&v) {
                    return Err(format!("row {} has a bad {}", row.id, label));
                }
            }
        }
    }
    if evaluation_rows(dataset).is_empty() {
        return Err("dataset has no evaluation rows".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dataset(rows: Vec<ReferenceRow>) -> Dataset {
        Dataset {
            name: "test".to_string(),
            version: "1".to_string(),
            acceptance_mape_pct: None,
            acceptance_bias_pct: None,
            rows,
        }
    }

    fn row(id: &str, scale: f64) -> ReferenceRow {
        ReferenceRow {
            id: id.to_string(),
            scale_weight_kg: scale,
            heart_girth_cm: None,
            body_length_cm: None,
            image: None,
            split: "evaluation".to_string(),
            synthetic: true,
            breed: None,
            notes: None,
        }
    }

    #[test]
    fn metrics_capture_error_bias_and_worst_case() {
        let m = summarize(&[
            ("a".to_string(), 500.0, 550.0),
            ("b".to_string(), 500.0, 450.0),
        ]);
        assert_eq!(m.n, 2);
        assert_eq!(m.mae_kg, 50.0);
        assert_eq!(m.mape_pct, 10.0);
        assert_eq!(m.bias_kg, 0.0);
        assert_eq!(m.max_abs_kg, 50.0);
        assert_eq!(m.within_10_pct, 1.0);
        assert_eq!(m.within_20_pct, 1.0);
        assert_eq!(m.implausible, 0);
    }

    #[test]
    fn non_finite_estimates_count_as_implausible() {
        let m = summarize(&[
            ("a".to_string(), 500.0, 500.0),
            ("b".to_string(), 500.0, f64::NAN),
            ("c".to_string(), 500.0, 1e12),
        ]);
        assert_eq!(m.n, 1);
        assert_eq!(m.implausible, 2);
    }

    #[test]
    fn calibration_rows_never_score() {
        let mut a = row("a", 500.0);
        a.split = "calibration".to_string();
        let ds = dataset(vec![a, row("b", 500.0)]);
        assert_eq!(evaluation_rows(&ds).len(), 1);
        assert!(validate_dataset(&ds).is_ok());
        assert!(validate_dataset(&dataset(vec![])).is_err());
    }
}
