//! Runtime option validation for the estimator API.
//!
//! Optional request fields (`backend`, `model`, `ollama_url`,
//! `ollama_api_key`, `prompt`) are type/size/allow-list validated before
//! use. A request-local [`Config`] clone carries the overrides; shared
//! server state is never mutated.

use serde_json::Value;

use crate::config::{Config, DEFAULT_OLLAMA_URL};

pub(crate) const MAX_BODY_BYTES: usize = 20 * 1024 * 1024;
pub(crate) const MAX_PROMPT_BYTES: usize = 16 * 1024;
pub(crate) const MAX_MODEL_BYTES: usize = 256;
pub(crate) const MAX_URL_BYTES: usize = 2 * 1024;
pub(crate) const MAX_API_KEY_BYTES: usize = 4 * 1024;
/// Max `animal_breed` hint length in bytes.
pub(crate) const MAX_BREED_BYTES: usize = 64;
/// Allowed `animal_sex` hints (lowercase; `unknown` means "no hint").
pub(crate) const ALLOWED_SEXES: &[&str] = &["cow", "bull", "steer", "heifer", "calf", "unknown"];
/// Max plausible cattle age in years.
pub(crate) const MAX_AGE_YEARS: f64 = 30.0;

pub(crate) fn optional_string<'a>(
    payload: &'a Value,
    field: &str,
) -> Result<Option<&'a str>, String> {
    match payload.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .map(Some)
            .ok_or_else(|| format!("{} must be a string", field)),
    }
}

/// Optional numeric field: accepts a JSON number only (no strings/bools).
/// Returns `Ok(None)` when absent or null so old clients keep working.
pub(crate) fn optional_number(payload: &Value, field: &str) -> Result<Option<f64>, String> {
    match payload.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n
            .as_f64()
            .map(Some)
            .ok_or_else(|| format!("{} must be a number", field)),
        Some(_) => Err(format!("{} must be a number", field)),
    }
}

pub(crate) fn has_control_or_whitespace(value: &str) -> bool {
    value
        .bytes()
        .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
}

pub(crate) fn valid_http_url(url: &str) -> bool {
    if url.is_empty()
        || url.len() > MAX_URL_BYTES
        || has_control_or_whitespace(url)
        || url.contains('@')
        || url.contains('?')
        || url.contains('#')
    {
        return false;
    }
    let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or("");
    let host = authority.split(':').next().unwrap_or("");
    !host.is_empty()
}

pub(crate) fn validate_runtime_config(config: &Config, prompt: &str) -> Result<(), String> {
    if config.backend != "ollama" && config.backend != "none" {
        return Err("Unsupported backend. Choose ollama or none.".to_string());
    }
    if config.model.is_empty()
        || config.model.len() > MAX_MODEL_BYTES
        || has_control_or_whitespace(&config.model)
    {
        return Err("Model must be a non-empty single-line value under 256 bytes.".to_string());
    }
    if !valid_http_url(&config.ollama_url) {
        return Err(
            "Ollama URL must be an http(s) URL without credentials or query parameters."
                .to_string(),
        );
    }
    if let Some(key) = &config.ollama_api_key {
        if key.len() > MAX_API_KEY_BYTES || key.bytes().any(|byte| byte.is_ascii_control()) {
            return Err("API key is too long or contains invalid control characters.".to_string());
        }
    }
    if prompt.len() > MAX_PROMPT_BYTES {
        return Err("Prompt is too long; keep it under 16 KiB.".to_string());
    }
    Ok(())
}

/// Optional animal-profile hints (`animal_breed`, `animal_sex`,
/// `animal_age_years`) sent with an estimate to sharpen the AI guess.
///
/// Separate `animal_*` names avoid colliding with the model's own `breed`
/// output field. Hints are echoed back on success and folded into the prompt
/// sent to Ollama (so the result cache stays correct); the deterministic
/// `none` placeholder weight is unchanged by hints.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct AnimalProfile {
    pub(crate) breed: Option<String>,
    pub(crate) sex: Option<String>,
    pub(crate) age_years: Option<f64>,
}

/// Parse and validate the animal-profile hints. Missing, null, or blank
/// values count as omitted so old clients keep working.
pub(crate) fn parse_animal_profile(payload: &Value) -> Result<AnimalProfile, String> {
    let breed = match optional_string(payload, "animal_breed")? {
        None => None,
        Some(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                None
            } else {
                if trimmed.len() > MAX_BREED_BYTES {
                    return Err("animal_breed must be under 64 bytes.".to_string());
                }
                if !trimmed
                    .chars()
                    .all(|c| c.is_ascii_alphabetic() || c == ' ' || c == '-' || c == '\'')
                    || !trimmed.chars().any(|c| c.is_ascii_alphabetic())
                {
                    return Err(
                        "animal_breed may only contain letters, spaces, and hyphens.".to_string(),
                    );
                }
                Some(trimmed.to_string())
            }
        }
    };
    let sex = match optional_string(payload, "animal_sex")? {
        None => None,
        Some(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                None
            } else {
                let lowered = trimmed.to_ascii_lowercase();
                if !ALLOWED_SEXES.contains(&lowered.as_str()) {
                    return Err(
                        "animal_sex must be one of cow, bull, steer, heifer, calf, or unknown."
                            .to_string(),
                    );
                }
                // "unknown" carries no information; treat it as omitted.
                (lowered != "unknown").then_some(lowered)
            }
        }
    };
    let age_years = match optional_number(payload, "animal_age_years")? {
        None => None,
        Some(age) => {
            if !age.is_finite() || age < 0.0 || age > MAX_AGE_YEARS {
                return Err("animal_age_years must be between 0 and 30.".to_string());
            }
            Some(age)
        }
    };
    Ok(AnimalProfile {
        breed,
        sex,
        age_years,
    })
}

/// Render the profile as a short suffix appended to the model prompt.
/// Returns an empty string when no hints were given (prompt unchanged).
pub(crate) fn profile_prompt_suffix(profile: &AnimalProfile) -> String {
    let mut parts = Vec::new();
    if let Some(breed) = &profile.breed {
        parts.push(format!("breed {}", breed));
    }
    if let Some(sex) = &profile.sex {
        parts.push(format!("sex {}", sex));
    }
    if let Some(age) = profile.age_years {
        parts.push(format!("age {} years", age));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" Animal context: {}.", parts.join("; "))
    }
}

pub(crate) fn safe_url_for_info(url: &str) -> String {
    if valid_http_url(url) {
        url.to_string()
    } else {
        DEFAULT_OLLAMA_URL.to_string()
    }
}

pub(crate) fn redact_secret(message: &str, configs: [&Config; 2]) -> String {
    let mut redacted = message.to_string();
    for config in configs {
        if let Some(key) = &config.ollama_api_key {
            if !key.is_empty() {
                redacted = redacted.replace(key, "[redacted]");
            }
        }
    }
    redacted
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_routes_are_explicit_and_options_are_validated() {
        assert!(valid_http_url("https://ollama.com/api/generate"));
        assert!(valid_http_url("http://127.0.0.1:11434/api/generate"));
        assert!(!valid_http_url("file:///secret"));
        assert!(!valid_http_url("https://user:secret@example.com/api"));
        assert!(!valid_http_url("https://example.com/api?key=secret"));
    }

    #[test]
    fn animal_profile_omitted_by_default() {
        let payload = serde_json::json!({});
        assert_eq!(
            parse_animal_profile(&payload).unwrap(),
            AnimalProfile::default()
        );
        assert_eq!(
            profile_prompt_suffix(&AnimalProfile::default()),
            String::new()
        );
    }

    #[test]
    fn animal_profile_accepts_valid_hints() {
        let payload = serde_json::json!({
            "animal_breed": "Angus",
            "animal_sex": "Cow",
            "animal_age_years": 4.5,
        });
        let profile = parse_animal_profile(&payload).unwrap();
        assert_eq!(profile.breed.as_deref(), Some("Angus"));
        // Sex is normalized to lowercase; the suffix feeds the model prompt.
        assert_eq!(profile.sex.as_deref(), Some("cow"));
        assert_eq!(profile.age_years, Some(4.5));
        assert_eq!(
            profile_prompt_suffix(&profile),
            " Animal context: breed Angus; sex cow; age 4.5 years."
        );
    }

    #[test]
    fn animal_profile_rejects_bad_hints() {
        for payload in [
            serde_json::json!({"animal_breed": "!!!"}),
            serde_json::json!({"animal_breed": "x".repeat(65)}),
            serde_json::json!({"animal_sex": "dinosaur"}),
            serde_json::json!({"animal_age_years": 99.0}),
            serde_json::json!({"animal_age_years": "old"}),
        ] {
            assert!(parse_animal_profile(&payload).is_err());
        }
        // Blank/unknown hints count as omitted, not errors.
        let payload = serde_json::json!({
            "animal_breed": "  ",
            "animal_sex": "unknown",
        });
        assert_eq!(
            parse_animal_profile(&payload).unwrap(),
            AnimalProfile::default()
        );
    }
}
