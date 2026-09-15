//! Configuration and constants for the cow weight estimator backend.
//!
//! All tunables are read from environment variables, falling back to a
//! `.env` file in the repository root. Values already in the environment
//! take precedence over `.env` (same contract as `aif/config.py`).
//!
//! Split into focused submodules:
//! - [`env`] — `.env` loading and `env_or` helpers.
//! - [`units`] — kilogram/pound conversion and rounding.

pub mod env;
pub mod units;

pub use env::{env_candidates, env_or, load_env_file};
pub use units::{kg_to_lbs, round1, KG_TO_LBS};

pub const DEFAULT_PROMPT: &str =
    "Estimate this cow's weight in kilograms from the provided image. \
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
pub const VERSION: &str = "0.1.0";

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
            .unwrap_or(DEFAULT_CACHE_TTL)
            .min(crate::cache::MAX_CACHE_TTL_SECS);
        let api_key = std::env::var("OLLAMA_API_KEY")
            .ok()
            .filter(|k| !k.is_empty());
        Config {
            backend: env_or("AIF_AI_BACKEND", "ollama"),
            ollama_url: env_or("AIF_OLLAMA_URL", DEFAULT_OLLAMA_URL),
            ollama_api_key: api_key,
            model: env_or("AIF_AI_MODEL", DEFAULT_OLLAMA_MODEL),
            cache_ttl,
        }
    }
}
