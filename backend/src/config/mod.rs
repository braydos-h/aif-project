//! Configuration and constants for the cow weight estimator backend.
//!
//! All tunables are read from environment variables, falling back to a
//! `.env` file in the repository root. Values already in the environment
//! take precedence over `.env`.
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
/// Version stamp recorded on every saved estimate so later model/prompt
/// changes can be compared against earlier results.
pub const ESTIMATOR_VERSION: &str = "0.1.0";
/// Prompt/method version recorded on saved estimates. Bump when
/// [`DEFAULT_PROMPT`] changes meaningfully.
pub const PROMPT_VERSION: &str = "2026-10-05-v1";

/// Struct holding the effective runtime configuration.
#[derive(Clone)]
pub struct Config {
    pub backend: String,
    pub ollama_url: String,
    pub ollama_api_key: Option<String>,
    pub model: String,
    pub cache_ttl: u64,
    /// When true, estimation/history/account routes require a session.
    /// Local default is open (matches historical behavior); production
    /// deployments must set `AIF_REQUIRE_AUTH=1` (implied by
    /// `AIF_PRODUCTION=1`).
    pub require_auth: bool,
    /// Full hardening profile: implies `require_auth`, secure cookies,
    /// private metrics, reduced info/health detail, same-origin CORS, and
    /// rejection of per-request provider overrides.
    pub production: bool,
    /// Directory holding `aif.db` (SQLite). Created on startup.
    pub data_dir: String,
    /// Configured public HTTPS origin used to build invite/recovery links.
    /// Invite links are never built from an untrusted `Host` header.
    pub public_origin: String,
    pub cookie_secure: bool,
    pub session_days: u64,
    pub invite_days: u64,
    /// Max provider-backed image estimates per user per UTC day.
    /// Tape-only estimates are free and never counted.
    pub daily_estimate_limit: u64,
    /// Max concurrent provider-backed inferences process-wide.
    pub max_concurrent_inference: usize,
    /// Operator pause switch (also flippable at runtime by the operator).
    pub inference_paused: bool,
    /// Exact IPs trusted to supply `X-Forwarded-For` (default: loopbacks).
    pub trusted_proxies: Vec<String>,
    /// Bootstrap operator email for first-run invite creation.
    pub operator_email: Option<String>,
}

fn env_bool(key: &str, default: bool) -> bool {
    match std::env::var(key).ok().map(|v| v.to_ascii_lowercase()) {
        Some(v) if v == "1" || v == "true" || v == "yes" => true,
        Some(v) if v == "0" || v == "false" || v == "no" => false,
        _ => default,
    }
}

fn env_u64(key: &str, default: u64) -> u64 {
    std::env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(default)
}

/// Validate production startup requirements. Returns a fatal message when
/// production is misconfigured; the binary refuses to start in that case.
pub fn validate_production_config(config: &Config) -> Result<(), String> {
    if !config.production {
        return Ok(());
    }
    if !config.require_auth {
        return Err("AIF_PRODUCTION=1 requires AIF_REQUIRE_AUTH=1".to_string());
    }
    if !(config.public_origin.starts_with("https://")
        && !config.public_origin.contains('@')
        && !config.public_origin.contains('?'))
    {
        return Err(
            "AIF_PRODUCTION=1 requires AIF_PUBLIC_ORIGIN=https://<domain>".to_string(),
        );
    }
    if config.backend == "ollama" && config.ollama_api_key.is_none() {
        return Err("AIF_PRODUCTION=1 with backend=ollama requires OLLAMA_API_KEY".to_string());
    }
    Ok(())
}

impl Config {
    /// Build the config from env vars / `.env`.
    pub fn from_env() -> Config {
        let raw_ttl = env_or("AIF_CACHE_TTL", &DEFAULT_CACHE_TTL.to_string());
        let parsed_ttl = raw_ttl.parse::<u64>();
        if parsed_ttl.is_err() {
            eprintln!(
                "warning: invalid AIF_CACHE_TTL {:?}; using default {}",
                raw_ttl, DEFAULT_CACHE_TTL
            );
        }
        let cache_ttl = parsed_ttl
            .unwrap_or(DEFAULT_CACHE_TTL)
            .min(crate::cache::MAX_CACHE_TTL_SECS);
        let api_key = std::env::var("OLLAMA_API_KEY")
            .ok()
            .filter(|k| !k.is_empty());
        let production = env_bool("AIF_PRODUCTION", false);
        let require_auth = production || env_bool("AIF_REQUIRE_AUTH", false);
        let cookie_secure = match std::env::var("AIF_COOKIE_SECURE").ok() {
            Some(v) if !v.is_empty() => {
                matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes")
            }
            _ => production,
        };
        let trusted_proxies = std::env::var("AIF_TRUSTED_PROXIES")
            .ok()
            .filter(|v| !v.is_empty())
            .map(|v| {
                v.split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect()
            })
            .unwrap_or_else(|| vec!["127.0.0.1".to_string(), "::1".to_string()]);
        Config {
            backend: env_or("AIF_AI_BACKEND", "ollama"),
            ollama_url: env_or("AIF_OLLAMA_URL", DEFAULT_OLLAMA_URL),
            ollama_api_key: api_key,
            model: env_or("AIF_AI_MODEL", DEFAULT_OLLAMA_MODEL),
            cache_ttl,
            require_auth,
            production,
            data_dir: env_or("AIF_DATA_DIR", "data"),
            public_origin: env_or("AIF_PUBLIC_ORIGIN", "http://127.0.0.1:8080"),
            cookie_secure,
            session_days: env_u64("AIF_SESSION_DAYS", 30).clamp(1, 365),
            invite_days: env_u64("AIF_INVITE_DAYS", 7).clamp(1, 30),
            daily_estimate_limit: env_u64("AIF_DAILY_LIMIT", 200).clamp(1, 100_000),
            max_concurrent_inference: env_u64("AIF_MAX_INFERENCE", 4).clamp(1, 64) as usize,
            inference_paused: env_bool("AIF_INFERENCE_PAUSED", false),
            trusted_proxies,
            operator_email: std::env::var("AIF_OPERATOR_EMAIL")
                .ok()
                .filter(|v| !v.is_empty()),
        }
    }
}
