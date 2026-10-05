//! `aif-eval`: repeatable estimate-quality evaluation.
//!
//! Runs a permissioned reference dataset (scale weights + tape/photo
//! inputs) through the estimator and reports error metrics per method,
//! stamped with provider/model/prompt/estimator versions. Rerun before
//! approving any model or prompt change and keep the previous report for
//! rollback comparison.
//!
//! ```sh
//! cargo run --manifest-path backend/Cargo.toml --bin aif-eval -- \
//!   --dataset eval/fixtures/example.json --out /tmp/opencode/eval-report.json
//! ```
//!
//! Photo rows need image bytes (inline base64 or a path relative to the
//! dataset file) and a configured backend; tape rows run offline. Exit
//! status is nonzero when the dataset's acceptance gate fails.

use std::path::{Path, PathBuf};

use aif_backend::cache::Cache;
use aif_backend::config::{self, Config, ESTIMATOR_VERSION, PROMPT_VERSION};
use aif_backend::eval::{evaluation_rows, summarize, validate_dataset, Dataset, MethodMetrics};
use aif_backend::tape::tape_weight_kg;

const USAGE: &str = "Usage: aif-eval --dataset PATH [--out PATH] [--repeat N]\n";

fn main() {
    let result = run(&std::env::args().collect::<Vec<_>>());
    std::process::exit(result);
}

fn run(args: &[String]) -> i32 {
    let mut dataset_path: Option<String> = None;
    let mut out_path: Option<String> = None;
    let mut repeat: usize = 2;
    let mut i = 1;
    while i < args.len() {
        let (flag, inline): (&str, Option<&str>) = match args[i].split_once('=') {
            Some((f, v)) => (f, Some(v)),
            None => (args[i].as_str(), None),
        };
        let mut take_value = |inline: Option<&str>| -> Result<String, String> {
            if let Some(v) = inline {
                return Ok(v.to_string());
            }
            i += 1;
            args.get(i)
                .cloned()
                .ok_or_else(|| format!("Missing value for {}", args[i - 1]))
        };
        match flag {
            "--dataset" => dataset_path = Some(take_value(inline).unwrap_or_default()),
            "--out" => out_path = Some(take_value(inline).unwrap_or_default()),
            "--repeat" => {
                let value = take_value(inline).unwrap_or_default();
                repeat = value.parse::<usize>().unwrap_or(0).clamp(1, 5);
            }
            "-h" | "--help" => {
                print!("{}", USAGE);
                return 0;
            }
            other => {
                eprintln!("Unknown argument: {}\n{}", other, USAGE);
                return 2;
            }
        }
        i += 1;
    }
    let Some(dataset_path) = dataset_path.filter(|p| !p.is_empty()) else {
        eprintln!("Missing --dataset\n{}", USAGE);
        return 2;
    };
    match evaluate(&dataset_path, repeat) {
        Ok(report) => {
            let text = serde_json::to_string_pretty(&report).unwrap_or_default();
            if let Some(out) = out_path {
                if let Err(e) = std::fs::write(&out, format!("{}\n", text)) {
                    eprintln!("cannot write {}: {}", out, e);
                    return 1;
                }
                println!("wrote {}", out);
            } else {
                println!("{}", text);
            }
            if report["gate_pass"] == serde_json::Value::Bool(false) {
                eprintln!("acceptance gate FAILED");
                return 3;
            }
            0
        }
        Err(e) => {
            eprintln!("evaluation failed: {}", e);
            1
        }
    }
}

fn evaluate(dataset_path: &str, repeat: usize) -> Result<serde_json::Value, String> {
    let raw = std::fs::read_to_string(dataset_path)
        .map_err(|e| format!("cannot read {}: {}", dataset_path, e))?;
    let dataset: Dataset =
        serde_json::from_str(&raw).map_err(|e| format!("invalid dataset JSON: {}", e))?;
    validate_dataset(&dataset)?;
    let base = Path::new(dataset_path).parent().unwrap_or(Path::new("."));

    config::env::load_env_file(".env");
    let config = Config::from_env();
    let cache = Cache::new(0);

    let mut tape_triples: Vec<(String, f64, f64)> = Vec::new();
    let mut photo_triples: Vec<(String, f64, f64)> = Vec::new();
    let mut repeat_diffs: Vec<f64> = Vec::new();
    let mut photo_placeholders = 0usize;

    for row in evaluation_rows(&dataset) {
        if let (Some(g), Some(l)) = (row.heart_girth_cm, row.body_length_cm) {
            let estimate = tape_weight_kg(g, l);
            tape_triples.push((row.id.clone(), row.scale_weight_kg, estimate));
        }
        if let Some(image) = row.image.as_deref().filter(|s| !s.is_empty()) {
            let bytes = load_image(base, image)?;
            let first = run_photo(&config, &cache, &bytes)?;
            photo_placeholders += first.1 as usize;
            for _ in 1..repeat {
                let again = run_photo(&config, &cache, &bytes)?;
                if first.0.is_finite() && again.0.is_finite() {
                    repeat_diffs.push((first.0 - again.0).abs());
                }
            }
            photo_triples.push((row.id.clone(), row.scale_weight_kg, first.0));
        }
    }

    let tape: MethodMetrics = summarize(&tape_triples);
    let photo: MethodMetrics = summarize(&photo_triples);
    let max_repeat_diff = repeat_diffs.iter().cloned().fold(0.0f64, f64::max);
    let gate_pass = check_gate(&dataset, &tape, &photo);
    let synthetic_rows = dataset.rows.iter().filter(|r| r.synthetic).count();

    Ok(serde_json::json!({
        "dataset": dataset.name,
        "dataset_version": dataset.version,
        "evaluation_rows": evaluation_rows(&dataset).len(),
        "synthetic_rows": synthetic_rows,
        "warning": synthetic_rows > 0,
        "provider": config.backend,
        "model": config.model,
        "prompt_version": PROMPT_VERSION,
        "estimator_version": ESTIMATOR_VERSION,
        "tape_vs_scale": tape,
        "photo_vs_scale": photo,
        "photo_placeholder_rows": photo_placeholders,
        "repeatability_max_diff_kg": (max_repeat_diff * 1000.0).round() / 1000.0,
        "repeat_runs": repeat,
        "gate_pass": gate_pass,
        "note": "Ranges are estimates, not calibrated confidence. Real-data acceptance is blocked until a permissioned cattle dataset is evaluated.",
    }))
}

/// Acceptance gate from the dataset's thresholds. The photo path gates the
/// AI behavior (MAPE and bias when photo rows exist); tape rows are the
/// reported offline baseline. A method with zero rows never fails the gate.
fn check_gate(dataset: &Dataset, tape: &MethodMetrics, photo: &MethodMetrics) -> bool {
    let _ = tape;
    let photo_ok = match dataset.acceptance_mape_pct {
        Some(max) if photo.n > 0 => photo.mape_pct <= max,
        _ => true,
    };
    let bias_ok = match dataset.acceptance_bias_pct {
        Some(max) if photo.n > 0 => photo.bias_pct.abs() <= max,
        _ => true,
    };
    photo_ok && bias_ok
}

/// Load image bytes from inline base64/data-URI or a dataset-relative path.
fn load_image(base: &Path, reference: &str) -> Result<String, String> {
    if reference.starts_with("data:") || reference.len() > 256 {
        return Ok(reference.to_string());
    }
    let candidate: PathBuf = base.join(reference);
    if candidate.is_file() {
        let bytes = std::fs::read(&candidate)
            .map_err(|e| format!("cannot read {}: {}", candidate.display(), e))?;
        return Ok(aif_backend::validate::base64_encode(&bytes));
    }
    // Otherwise treat it as a raw base64 payload (validated downstream).
    Ok(reference.to_string())
}

/// Run one photo estimate; returns (weight_kg, was_placeholder).
fn run_photo(config: &Config, cache: &Cache, image_b64: &str) -> Result<(f64, bool), String> {
    if config.backend == "none" {
        let value = aif_backend::fallback::estimate_fallback(image_b64, config::DEFAULT_PROMPT);
        let weight = value
            .get("estimated_weight_kg")
            .and_then(|v| v.as_f64())
            .unwrap_or(f64::NAN);
        return Ok((weight, true));
    }
    match aif_backend::ollama::estimate_via_ollama(config, cache, image_b64, config::DEFAULT_PROMPT) {
        Ok(value) => Ok((
            value
                .get("estimated_weight_kg")
                .and_then(|v| v.as_f64())
                .unwrap_or(f64::NAN),
            false,
        )),
        Err(e) => Err(format!("provider estimate failed: {}", e)),
    }
}
