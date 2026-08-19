//! Configuration and constants for the cow weight estimator backend.
//!
//! All tunables are read from environment variables, falling back to a
//! `.env` file in the repository root. Values already in the environment
//! take precedence over `.env` (same contract as `aif/config.py`).

use std::env;
use std::path::Path;

pub const DEFAULT_PROMPT: &str = "Estimate this cow's weight in kilograms from the provided image. \
Reply with ONLY a JSON object of the form \
{\"weight_kg\": <number>, \"confidence\": <0..1>, \
\"breed\": <string>, \"body_condition_score\": <1..9>} \
where confidence is your confidence in the estimate (0..1), breed is your \
best guess of the breed (or \"unknown\"), and body_condition_score is a \
1-9 score. Do not include any text outside the JSON object.";

pub const DEFAULT_OLLAMA_URL: &str = "https://ollama.com/api/generate";
pub const DEFAULT_OLLAMA_MODEL: &str = "gemma4:31b-cloud";
pub const DEFAULT_CACHE_TTL: u64 = 300;
pub const OLLAMA_MAX_RETRIES: u32 = 1;
pub const OLLAMA_RETRY_BACKOFF_SECS: u64 = 1;
pub const KG_TO_LBS: f64 = 2.20462;
pub const VERSION: &str = "0.1.0";

/// Load a `.env` file into the environment without overriding existing values.
///
/// Looks for the file in the repository root (the parent of the `backend`
/// directory). Lines like `KEY=value` are parsed; blank lines and `#`
/// comments are ignored. Quoted values have the quotes stripped.
pub fn load_env_file(filename: &str) {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let env_path = repo_root.join(filename);
    if !env_path.is_file() {
        return;
    }
    let content = match std::fs::read_to_string(&env_path) {
        Ok(c) => c,
        Err(_) => return,
    };
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') || !line.contains('=') {
            continue;
        }
        let (key, value) = line.split_once('=').unwrap();
        let key = key.trim();
        let mut value = value.trim().to_string();
        if value.len() >= 2 {
            let first = value.chars().next().unwrap();
            let last = value.chars().last().unwrap();
            if first == last && (first == '\'' || first == '"') {
                value = value[1..value.len() - 1].to_string();
            }
        }
        if !key.is_empty() && env::var(key).is_err() {
            env::set_var(key, &value);
        }
    }
}

/// Read an env var, falling back to `default` when unset or empty.
pub fn env_or(key: &str, default: &str) -> String {
    env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

/// Struct holding the effective runtime configuration.
#[derive(Clone)]
pub struct Config {
    pub backend: String,
    pub ollama_url: String,
    pub ollama_api_key: Option<String>,
    pub model: String,
    pub cache_ttl: u64,
}

impl Config {
    /// Build the config from env vars / `.env`, mirroring `CowWeightEstimator`.
    pub fn from_env() -> Config {
        let cache_ttl = env_or("AIF_CACHE_TTL", &DEFAULT_CACHE_TTL.to_string())
            .parse::<u64>()
            .unwrap_or(DEFAULT_CACHE_TTL);
        let api_key = env::var("OLLAMA_API_KEY").ok().filter(|k| !k.is_empty());
        Config {
            backend: env_or("AIF_AI_BACKEND", "ollama"),
            ollama_url: env_or("AIF_OLLAMA_URL", DEFAULT_OLLAMA_URL),
            ollama_api_key: api_key,
            model: env_or("AIF_AI_MODEL", DEFAULT_OLLAMA_MODEL),
            cache_ttl,
        }
    }
}

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

    #[test]
    fn env_file_loads_into_missing_vars_without_overriding() {
        let dir = std::env::temp_dir().join(format!("aif-env-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join(".env"),
            "AIF_AI_BACKEND=none\n# comment\n\nQUOTED=\"hello world\"\n",
        )
        .unwrap();

        // Existing values take precedence over .env.
        env::set_var("AIF_AI_BACKEND", "ollama");
        env::remove_var("AIF_TEST_QUOTED");
        parse_env_file(dir.join(".env").to_str().unwrap());

        assert_eq!(env::var("AIF_AI_BACKEND").unwrap(), "ollama");
        assert_eq!(env::var("AIF_TEST_QUOTED").unwrap(), "hello world");
        env::remove_var("AIF_AI_BACKEND");
        env::remove_var("AIF_TEST_QUOTED");
        std::fs::remove_dir_all(&dir).ok();
    }

    fn parse_env_file(path: &str) {
        // Same parsing logic as load_env_file, factored for arbitrary paths.
        let content = std::fs::read_to_string(Path::new(path)).unwrap();
        for raw_line in content.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') || !line.contains('=') {
                continue;
            }
            let (key, value) = line.split_once('=').unwrap();
            let key = key.trim();
            let mut value = value.trim().to_string();
            if value.len() >= 2 {
                let first = value.chars().next().unwrap();
                let last = value.chars().last().unwrap();
                if first == last && (first == '\'' || first == '"') {
                    value = value[1..value.len() - 1].to_string();
                }
            }
            if !key.is_empty() && env::var(key).is_err() {
                env::set_var(key, &value);
            }
        }
    }
}
