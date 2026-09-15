//! `POST /estimate-weight` and `POST /estimate-batch` handlers.
//!
//! Accepts `image_url` / `image_base64` plus optional tape measurements
//! (`heart_girth_cm`, `body_length_cm`), animal-profile hints
//! (`animal_breed`, `animal_sex`, `animal_age_years`), and runtime overrides
//! (`backend`, `model`, `ollama_url`, `ollama_api_key`, `prompt`). Overrides
//! are validated and stored on a request-local [`Config`] clone; the shared
//! server state is never mutated and secrets are never logged.
//!
//! Tape-only requests need no image and no network: they return a Schaeffer
//! estimate with `source == "tape_measure"`. When image and tape are both
//! present, the photo estimate is returned with the tape cross-check merged
//! in as `tape_weight_kg` / `tape_weight_lbs` / `tape_min_kg` / `tape_max_kg`.
//!
//! The batch route takes `{"items": [<single payload>, ...]}` (at most
//! [`MAX_BATCH_ITEMS`]) and returns per-item `{status, body}` pairs so one
//! bad item never fails the whole batch. Each item body carries its own
//! `{parent}-{index}` request id for traceability.

use serde_json::{json, Value};

use super::response::{
    error_json, with_request_id, Response, CODE_ESTIMATION_FAILED, CODE_INVALID_IMAGE,
    CODE_INVALID_JSON, CODE_INVALID_OPTIONS, CODE_MISSING_BODY, CODE_MISSING_IMAGE,
};
use super::validation::{
    optional_number, optional_string, parse_animal_profile, profile_prompt_suffix, redact_secret,
    validate_runtime_config, AnimalProfile,
};
use super::ServerState;
use crate::cache::Cache;
use crate::config::{Config, DEFAULT_PROMPT};
use crate::fallback::estimate_fallback;
use crate::ollama::estimate_via_ollama;
use crate::tape::{estimate_tape, tape_weight_kg, validate_tape_measure, weight_range_kg};
use crate::validate::ImageValidationError;

/// Max items accepted by `POST /estimate-batch`. Bounds per-connection work:
/// items run sequentially and share the 20 MiB body limit.
pub(crate) const MAX_BATCH_ITEMS: usize = 20;

pub(crate) fn handle_estimate(body: &[u8], request_id: &str, state: &ServerState) -> Response {
    if body.is_empty() {
        return Response::json(
            400,
            error_json(CODE_MISSING_BODY, "Missing request body", request_id),
        );
    }
    let payload: Value = match serde_json::from_slice(body) {
        Ok(p) => p,
        Err(_) => {
            return Response::json(
                400,
                error_json(CODE_INVALID_JSON, "Invalid JSON payload", request_id),
            );
        }
    };
    let (status, result) = estimate_one(&payload, request_id, state);
    Response::json(status, result)
}

/// Estimate a batch of payloads. Whole-batch shape errors return 400;
/// per-item failures are isolated into their own `{status, body}` entry.
pub(crate) fn handle_estimate_batch(
    body: &[u8],
    request_id: &str,
    state: &ServerState,
) -> Response {
    if body.is_empty() {
        return Response::json(
            400,
            error_json(CODE_MISSING_BODY, "Missing request body", request_id),
        );
    }
    let payload: Value = match serde_json::from_slice(body) {
        Ok(p) => p,
        Err(_) => {
            return Response::json(
                400,
                error_json(CODE_INVALID_JSON, "Invalid JSON payload", request_id),
            );
        }
    };
    let items = match payload.get("items") {
        Some(Value::Array(items)) => items,
        _ => {
            return Response::json(
                400,
                error_json(
                    CODE_INVALID_OPTIONS,
                    "items must be an array of estimate payloads",
                    request_id,
                ),
            );
        }
    };
    if items.is_empty() {
        return Response::json(
            400,
            error_json(
                CODE_INVALID_OPTIONS,
                "items must contain at least one estimate payload",
                request_id,
            ),
        );
    }
    if items.len() > MAX_BATCH_ITEMS {
        return Response::json(
            400,
            error_json(
                CODE_INVALID_OPTIONS,
                "items must contain at most 20 estimate payloads",
                request_id,
            ),
        );
    }
    let mut results = Vec::with_capacity(items.len());
    for (index, item) in items.iter().enumerate() {
        let sub_id = format!("{}-{}", request_id, index);
        let (status, body) = match item.as_object() {
            None => (
                400,
                error_json(
                    CODE_INVALID_OPTIONS,
                    "Each item must be a JSON object",
                    &sub_id,
                ),
            ),
            Some(_) => estimate_one(item, &sub_id, state),
        };
        results.push(json!({"status": status, "body": body}));
    }
    Response::json(
        200,
        with_request_id(json!({"results": results}), request_id),
    )
}

/// Run one estimate payload against the configured backend.
///
/// Returns the HTTP status plus the JSON body (already carrying
/// `request_id`). Shared by the single and batch routes.
pub(crate) fn estimate_one(payload: &Value, request_id: &str, state: &ServerState) -> (u16, Value) {
    let image_url = match optional_string(payload, "image_url") {
        Ok(value) => value,
        Err(message) => {
            return (400, error_json(CODE_INVALID_OPTIONS, &message, request_id));
        }
    };
    let image_base64 = match optional_string(payload, "image_base64") {
        Ok(value) => value,
        Err(message) => {
            return (400, error_json(CODE_INVALID_OPTIONS, &message, request_id));
        }
    };
    let prompt_value = match optional_string(payload, "prompt") {
        Ok(value) => value,
        Err(message) => {
            return (400, error_json(CODE_INVALID_OPTIONS, &message, request_id));
        }
    };
    let base_prompt = prompt_value
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_PROMPT);

    let girth = match optional_number(payload, "heart_girth_cm") {
        Ok(value) => value,
        Err(message) => {
            return (400, error_json(CODE_INVALID_OPTIONS, &message, request_id));
        }
    };
    let length = match optional_number(payload, "body_length_cm") {
        Ok(value) => value,
        Err(message) => {
            return (400, error_json(CODE_INVALID_OPTIONS, &message, request_id));
        }
    };
    // Partial tape input is a client error, not a silent ignore.
    if girth.is_some() != length.is_some() {
        return (
            400,
            error_json(
                CODE_INVALID_OPTIONS,
                "Provide both heart_girth_cm and body_length_cm for a tape estimate.",
                request_id,
            ),
        );
    }
    let tape = match (girth, length) {
        (Some(g), Some(l)) => {
            if let Err(message) = validate_tape_measure(g, "heart_girth_cm")
                .and_then(|_| validate_tape_measure(l, "body_length_cm"))
            {
                return (400, error_json(CODE_INVALID_OPTIONS, &message, request_id));
            }
            Some((g, l))
        }
        _ => None,
    };

    let profile = match parse_animal_profile(payload) {
        Ok(profile) => profile,
        Err(message) => {
            return (400, error_json(CODE_INVALID_OPTIONS, &message, request_id));
        }
    };
    // Hints sharpen the AI prompt (and thus the Ollama cache key). With no
    // hints the prompt is byte-identical to before, keeping old clients
    // and cached entries stable.
    let enriched_prompt = format!("{}{}", base_prompt, profile_prompt_suffix(&profile));

    let image_reference = image_url
        .filter(|value| !value.is_empty())
        .or_else(|| image_base64.filter(|value| !value.is_empty()));
    // Tape-only requests skip the image requirement entirely (offline field use).
    if image_reference.is_none() {
        if let Some((g, l)) = tape {
            return (
                200,
                with_request_id(
                    attach_profile(estimate_tape(g, l, &enriched_prompt), &profile),
                    request_id,
                ),
            );
        }
        return (
            400,
            error_json(
                CODE_MISSING_IMAGE,
                "Provide image_url, image_base64, or both heart_girth_cm and body_length_cm",
                request_id,
            ),
        );
    }
    let image_reference = image_reference.unwrap();

    let mut request_config = state.config.clone();
    for (field, target) in [
        ("backend", &mut request_config.backend),
        ("model", &mut request_config.model),
        ("ollama_url", &mut request_config.ollama_url),
    ] {
        let value = match optional_string(payload, field) {
            Ok(value) => value,
            Err(message) => {
                return (400, error_json(CODE_INVALID_OPTIONS, &message, request_id));
            }
        };
        if let Some(value) = value {
            *target = value.to_string();
        }
    }
    let api_key = match optional_string(payload, "ollama_api_key") {
        Ok(value) => value,
        Err(message) => {
            return (400, error_json(CODE_INVALID_OPTIONS, &message, request_id));
        }
    };
    if let Some(value) = api_key {
        request_config.ollama_api_key = (!value.is_empty()).then(|| value.to_string());
    }

    if let Err(message) = validate_runtime_config(&request_config, &enriched_prompt) {
        return (400, error_json(CODE_INVALID_OPTIONS, &message, request_id));
    }

    let result = run_backend(
        &request_config,
        &state.cache,
        image_reference,
        &enriched_prompt,
    );

    match result {
        Ok(result) => {
            // Photo + tape in one call: keep the photo estimate primary and
            // attach the tape cross-check so the field user sees agreement.
            let mut result = attach_profile(result, &profile);
            if let Some((g, l)) = tape {
                let tape_kg = tape_weight_kg(g, l);
                let (tape_min, tape_max) =
                    weight_range_kg(tape_kg, crate::tape::TAPE_RANGE_FRACTION);
                result["tape_weight_kg"] = Value::from(tape_kg);
                result["tape_weight_lbs"] = Value::from(crate::config::kg_to_lbs(tape_kg));
                result["tape_min_kg"] = Value::from(tape_min);
                result["tape_max_kg"] = Value::from(tape_max);
                result["heart_girth_cm"] = Value::from(g);
                result["body_length_cm"] = Value::from(l);
            }
            (200, with_request_id(result, request_id))
        }
        Err(error) => {
            let message = redact_secret(&error.to_string(), [&state.config, &request_config]);
            if error.is::<ImageValidationError>() {
                eprintln!("invalid_image [{}]: {}", request_id, message);
                (400, error_json(CODE_INVALID_IMAGE, &message, request_id))
            } else {
                eprintln!("estimation failed [{}]: {}", request_id, message);
                (
                    502,
                    error_json(CODE_ESTIMATION_FAILED, &message, request_id),
                )
            }
        }
    }
}

/// Echo validated `animal_*` hints onto a success body. Uses dedicated
/// field names so a hint never overwrites the model's own `breed` guess.
fn attach_profile(mut result: Value, profile: &AnimalProfile) -> Value {
    if let Some(breed) = &profile.breed {
        result["animal_breed"] = Value::from(breed.clone());
    }
    if let Some(sex) = &profile.sex {
        result["animal_sex"] = Value::from(sex.clone());
    }
    if let Some(age) = profile.age_years {
        result["animal_age_years"] = Value::from(age);
    }
    result
}

/// Dispatch to the configured backend. `none` is a deterministic offline
/// placeholder; `ollama` performs the network call.
fn run_backend(
    request_config: &Config,
    cache: &Cache,
    image_reference: &str,
    prompt: &str,
) -> Result<Value, Box<dyn std::error::Error>> {
    match request_config.backend.as_str() {
        "none" => Ok(estimate_fallback(image_reference, prompt)),
        "ollama" => estimate_via_ollama(request_config, cache, image_reference, prompt),
        _ => unreachable!("validate_runtime_config checked backend"),
    }
}
