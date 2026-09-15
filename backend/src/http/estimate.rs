//! `POST /estimate-weight` handler.
//!
//! Accepts `image_url` / `image_base64` plus optional tape measurements
//! (`heart_girth_cm`, `body_length_cm`) and runtime overrides (`backend`,
//! `model`, `ollama_url`, `ollama_api_key`, `prompt`). Overrides are
//! validated and stored on a request-local [`Config`] clone; the shared
//! server state is never mutated and secrets are never logged.
//!
//! Tape-only requests need no image and no network: they return a Schaeffer
//! estimate with `source == "tape_measure"`. When image and tape are both
//! present, the photo estimate is returned with the tape cross-check merged
//! in as `tape_weight_kg` / `tape_weight_lbs` / `tape_min_kg` / `tape_max_kg`.

use serde_json::Value;

use super::response::{
    error_json, with_request_id, Response, CODE_ESTIMATION_FAILED, CODE_INVALID_IMAGE,
    CODE_INVALID_JSON, CODE_INVALID_OPTIONS, CODE_MISSING_BODY, CODE_MISSING_IMAGE,
};
use super::validation::{optional_number, optional_string, redact_secret, validate_runtime_config};
use super::ServerState;
use crate::cache::Cache;
use crate::config::{Config, DEFAULT_PROMPT};
use crate::fallback::estimate_fallback;
use crate::ollama::estimate_via_ollama;
use crate::tape::{estimate_tape, tape_weight_kg, validate_tape_measure, weight_range_kg};
use crate::validate::ImageValidationError;

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

    let image_url = match optional_string(&payload, "image_url") {
        Ok(value) => value,
        Err(message) => {
            return Response::json(400, error_json(CODE_INVALID_OPTIONS, &message, request_id));
        }
    };
    let image_base64 = match optional_string(&payload, "image_base64") {
        Ok(value) => value,
        Err(message) => {
            return Response::json(400, error_json(CODE_INVALID_OPTIONS, &message, request_id));
        }
    };
    let prompt_value = match optional_string(&payload, "prompt") {
        Ok(value) => value,
        Err(message) => {
            return Response::json(400, error_json(CODE_INVALID_OPTIONS, &message, request_id));
        }
    };
    let prompt = prompt_value
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_PROMPT);

    let girth = match optional_number(&payload, "heart_girth_cm") {
        Ok(value) => value,
        Err(message) => {
            return Response::json(400, error_json(CODE_INVALID_OPTIONS, &message, request_id));
        }
    };
    let length = match optional_number(&payload, "body_length_cm") {
        Ok(value) => value,
        Err(message) => {
            return Response::json(400, error_json(CODE_INVALID_OPTIONS, &message, request_id));
        }
    };
    // Partial tape input is a client error, not a silent ignore.
    if girth.is_some() != length.is_some() {
        return Response::json(
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
                return Response::json(400, error_json(CODE_INVALID_OPTIONS, &message, request_id));
            }
            Some((g, l))
        }
        _ => None,
    };

    let image_reference = image_url
        .filter(|value| !value.is_empty())
        .or_else(|| image_base64.filter(|value| !value.is_empty()));
    // Tape-only requests skip the image requirement entirely (offline field use).
    if image_reference.is_none() {
        if let Some((g, l)) = tape {
            return Response::json(
                200,
                with_request_id(estimate_tape(g, l, prompt), request_id),
            );
        }
        return Response::json(
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
        let value = match optional_string(&payload, field) {
            Ok(value) => value,
            Err(message) => {
                return Response::json(400, error_json(CODE_INVALID_OPTIONS, &message, request_id));
            }
        };
        if let Some(value) = value {
            *target = value.to_string();
        }
    }
    let api_key = match optional_string(&payload, "ollama_api_key") {
        Ok(value) => value,
        Err(message) => {
            return Response::json(400, error_json(CODE_INVALID_OPTIONS, &message, request_id));
        }
    };
    if let Some(value) = api_key {
        request_config.ollama_api_key = (!value.is_empty()).then(|| value.to_string());
    }

    if let Err(message) = validate_runtime_config(&request_config, prompt) {
        return Response::json(400, error_json(CODE_INVALID_OPTIONS, &message, request_id));
    }

    let result = run_backend(&request_config, &state.cache, image_reference, prompt);

    match result {
        Ok(mut result) => {
            // Photo + tape in one call: keep the photo estimate primary and
            // attach the tape cross-check so the field user sees agreement.
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
            Response::json(200, with_request_id(result, request_id))
        }
        Err(error) => {
            let message = redact_secret(&error.to_string(), [&state.config, &request_config]);
            if error.is::<ImageValidationError>() {
                eprintln!("invalid_image [{}]: {}", request_id, message);
                Response::json(400, error_json(CODE_INVALID_IMAGE, &message, request_id))
            } else {
                eprintln!("estimation failed [{}]: {}", request_id, message);
                Response::json(
                    502,
                    error_json(CODE_ESTIMATION_FAILED, &message, request_id),
                )
            }
        }
    }
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
