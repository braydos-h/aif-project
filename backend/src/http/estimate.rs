//! `POST /estimate-weight` and `POST /estimate-batch` handlers.
//!
//! Accepts `image_url` / `image_base64` plus optional tape measurements
//! (`heart_girth_cm`, `body_length_cm`), animal-profile hints
//! (`animal_breed`, `animal_sex`, `animal_age_years`), and runtime overrides
//! (`backend`, `model`, `ollama_url`, `ollama_api_key`, `prompt`). Overrides
//! are validated and stored on a request-local [`Config`] clone; the shared
//! server state is never mutated and secrets are never logged.
//!
//! In production (`AIF_PRODUCTION=1`) per-request provider overrides are
//! rejected: the model, backend, endpoint, and API key are server-side only.
//!
//! Authenticated estimates are saved to the caller's server-side history
//! (with source/model/version stamps and a placeholder flag on deterministic
//! fallback results). Callers may pass `idempotency_key` so a retried
//! upload replays the stored result instead of spending inference twice.
//! Photo estimates count against the caller's daily quota; tape-only
//! estimates are free. When the operator pauses inference, provider-backed
//! estimates answer `503 inference_paused` while tape estimates keep
//! working.
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

use super::history::{row_from_result, stored_result_json, SaveContext};
use super::response::{
    error_json, with_request_id, Response, CODE_ESTIMATION_FAILED, CODE_INVALID_IMAGE,
    CODE_INVALID_JSON, CODE_INVALID_OPTIONS, CODE_MISSING_BODY, CODE_MISSING_IMAGE, CODE_NOT_FOUND,
    CODE_PAUSED, CODE_QUOTA, CODE_SERVER_BUSY,
};
use super::server::RequestHead;
use super::session::{authenticate, check_csrf, Requester};
use super::validation::{
    optional_number, optional_string, parse_animal_profile, profile_prompt_suffix, redact_secret,
    validate_runtime_config, AnimalProfile,
};
use super::ServerState;
use crate::cache::Cache;
use crate::config::{Config, DEFAULT_PROMPT};
use crate::fallback::estimate_fallback;
use crate::ollama::cache_key::url_host;
use crate::ollama::estimate_via_ollama;
use crate::tape::{estimate_tape, tape_weight_kg, validate_tape_measure, weight_range_kg};
use crate::time_util::{rfc3339, unix_now, utc_day};
use crate::validate::fetch::check_image_url_allowed;
use crate::validate::ImageValidationError;

/// Max items accepted by `POST /estimate-batch`. Bounds per-connection work:
/// items run sequentially and share the 20 MiB body limit.
pub(crate) const MAX_BATCH_ITEMS: usize = 20;
/// Max `idempotency_key` length in bytes.
pub(crate) const MAX_IDEMPOTENCY_KEY_BYTES: usize = 128;
/// Stored prompt prefix length (prompts can be 16 KiB; history keeps a head).
pub(crate) const STORED_PROMPT_BYTES: usize = 2000;

fn policy(state: &ServerState, response: Response) -> Response {
    response.with_policy(state.config.production)
}

/// Resolve the caller. When auth is required, anonymous callers get 401;
/// otherwise the caller may be anonymous (estimates still work, but are
/// never saved to history).
fn resolve_caller(
    state: &ServerState,
    head: &RequestHead,
    request_id: &str,
    peer_ip: &str,
) -> Result<Option<Requester>, Response> {
    let _ = peer_ip;
    let requester = authenticate(state, head);
    if state.config.require_auth && requester.is_none() {
        return Err(policy(
            state,
            Response::json(
                401,
                error_json(
                    super::response::CODE_UNAUTHORIZED,
                    "Log in to use the estimator",
                    request_id,
                ),
            ),
        ));
    }
    if let Some(ref requester) = requester {
        check_csrf(state, head, requester, request_id)?;
    }
    Ok(requester)
}

pub(crate) fn handle_estimate(
    body: &[u8],
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let requester = match resolve_caller(state, head, request_id, peer_ip) {
        Ok(r) => r,
        Err(r) => return r,
    };
    if body.is_empty() {
        return policy(
            state,
            Response::json(
                400,
                error_json(CODE_MISSING_BODY, "Missing request body", request_id),
            ),
        );
    }
    let payload: Value = match serde_json::from_slice(body) {
        Ok(p) => p,
        Err(_) => {
            return policy(
                state,
                Response::json(
                    400,
                    error_json(CODE_INVALID_JSON, "Invalid JSON payload", request_id),
                ),
            );
        }
    };
    let (status, result) = estimate_one(&payload, request_id, state, head, &requester);
    let mut response = Response::json(status, result);
    if response_body_replayed(&response) {
        response = response.with_header("X-Idempotent-Replayed", "true");
    }
    policy(state, response)
}

fn response_body_replayed(response: &Response) -> bool {
    serde_json::from_slice::<Value>(&response.body)
        .map(|v| v.get("replayed").and_then(|r| r.as_bool()).unwrap_or(false))
        .unwrap_or(false)
}

/// Estimate a batch of payloads. Whole-batch shape errors return 400;
/// per-item failures are isolated into their own `{status, body}` entry.
pub(crate) fn handle_estimate_batch(
    body: &[u8],
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let requester = match resolve_caller(state, head, request_id, peer_ip) {
        Ok(r) => r,
        Err(r) => return r,
    };
    if body.is_empty() {
        return policy(
            state,
            Response::json(
                400,
                error_json(CODE_MISSING_BODY, "Missing request body", request_id),
            ),
        );
    }
    let payload: Value = match serde_json::from_slice(body) {
        Ok(p) => p,
        Err(_) => {
            return policy(
                state,
                Response::json(
                    400,
                    error_json(CODE_INVALID_JSON, "Invalid JSON payload", request_id),
                ),
            );
        }
    };
    let items = match payload.get("items") {
        Some(Value::Array(items)) => items,
        _ => {
            return policy(
                state,
                Response::json(
                    400,
                    error_json(
                        CODE_INVALID_OPTIONS,
                        "items must be an array of estimate payloads",
                        request_id,
                    ),
                ),
            );
        }
    };
    if items.is_empty() {
        return policy(
            state,
            Response::json(
                400,
                error_json(
                    CODE_INVALID_OPTIONS,
                    "items must contain at least one estimate payload",
                    request_id,
                ),
            ),
        );
    }
    if items.len() > MAX_BATCH_ITEMS {
        return policy(
            state,
            Response::json(
                400,
                error_json(
                    CODE_INVALID_OPTIONS,
                    "items must contain at most 20 estimate payloads",
                    request_id,
                ),
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
            Some(_) => estimate_one(item, &sub_id, state, head, &requester),
        };
        results.push(json!({"status": status, "body": body}));
    }
    policy(
        state,
        Response::json(
            200,
            with_request_id(json!({"results": results}), request_id),
        ),
    )
}

/// Validate an idempotency key: short opaque client token.
fn validate_idempotency_key(key: &str) -> Result<(), String> {
    if key.is_empty() || key.len() > MAX_IDEMPOTENCY_KEY_BYTES {
        return Err("idempotency_key must be 1-128 characters.".to_string());
    }
    if !key
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
    {
        return Err("idempotency_key may only contain letters, digits, and -_.:".to_string());
    }
    Ok(())
}

/// Validate an optional measurement timestamp (loose RFC 3339 shape, not
/// in the future). Stored separately from the record creation time.
fn validate_measured_at(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.len() < 10
        || trimmed.len() > 30
        || !trimmed.starts_with(|c: char| c.is_ascii_digit())
    {
        return Err("measured_at must be an RFC 3339 UTC timestamp.".to_string());
    }
    if trimmed > rfc3339(unix_now() + 3600).as_str() {
        return Err("measured_at cannot be in the future.".to_string());
    }
    Ok(trimmed.to_string())
}

/// Run one estimate payload against the configured backend.
///
/// Returns the HTTP status plus the JSON body (already carrying
/// `request_id`). Shared by the single and batch routes.
pub(crate) fn estimate_one(
    payload: &Value,
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    requester: &Option<Requester>,
) -> (u16, Value) {
    let err = |code: &str, message: &str| -> (u16, Value) {
        (400, error_json(code, message, request_id))
    };
    // Production keeps provider configuration server-side: ordinary
    // per-request overrides are rejected outright.
    if state.config.production {
        for field in ["backend", "model", "ollama_url", "ollama_api_key"] {
            if !matches!(payload.get(field), None | Some(Value::Null)) {
                return (
                    400,
                    error_json(
                        CODE_INVALID_OPTIONS,
                        "Provider settings are configured server-side in production",
                        request_id,
                    ),
                );
            }
        }
    }
    let image_url = match optional_string(payload, "image_url") {
        Ok(value) => value,
        Err(message) => {
            return err(CODE_INVALID_OPTIONS, &message);
        }
    };
    let image_base64 = match optional_string(payload, "image_base64") {
        Ok(value) => value,
        Err(message) => {
            return err(CODE_INVALID_OPTIONS, &message);
        }
    };
    let prompt_value = match optional_string(payload, "prompt") {
        Ok(value) => value,
        Err(message) => {
            return err(CODE_INVALID_OPTIONS, &message);
        }
    };
    let base_prompt = prompt_value
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_PROMPT);

    let girth = match optional_number(payload, "heart_girth_cm") {
        Ok(value) => value,
        Err(message) => {
            return err(CODE_INVALID_OPTIONS, &message);
        }
    };
    let length = match optional_number(payload, "body_length_cm") {
        Ok(value) => value,
        Err(message) => {
            return err(CODE_INVALID_OPTIONS, &message);
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
                return err(CODE_INVALID_OPTIONS, &message);
            }
            Some((g, l))
        }
        _ => None,
    };

    let profile = match parse_animal_profile(payload) {
        Ok(profile) => profile,
        Err(message) => {
            return err(CODE_INVALID_OPTIONS, &message);
        }
    };
    // Hints sharpen the AI prompt (and thus the Ollama cache key). With no
    // hints the prompt is byte-identical to before, keeping old clients
    // and cached entries stable.
    let enriched_prompt = format!("{}{}", base_prompt, profile_prompt_suffix(&profile));

    // History linkage + idempotency + measurement time (validated before
    // any inference or quota spend).
    let animal_id = match optional_string(payload, "animal_id") {
        Ok(value) => value.filter(|v| !v.is_empty()),
        Err(message) => return err(CODE_INVALID_OPTIONS, &message),
    };
    if let Some(id) = animal_id {
        match requester {
            None => {
                return err(
                    CODE_INVALID_OPTIONS,
                    "Log in to link an estimate to an animal",
                )
            }
            Some(requester) => match state.db.animal_by_id(id) {
                Ok(Some(animal)) if animal.user_id == requester.user.id => {}
                _ => {
                    return (
                        404,
                        error_json(CODE_NOT_FOUND, "Animal not found", request_id),
                    )
                }
            },
        }
    }
    let measured_at = match optional_string(payload, "measured_at") {
        Ok(Some(value)) if !value.trim().is_empty() => match validate_measured_at(value) {
            Ok(ts) => Some(ts),
            Err(message) => return err(CODE_INVALID_OPTIONS, &message),
        },
        Ok(_) => None,
        Err(message) => return err(CODE_INVALID_OPTIONS, &message),
    };
    let body_key = match optional_string(payload, "idempotency_key") {
        Ok(value) => value.filter(|v| !v.is_empty()),
        Err(message) => return err(CODE_INVALID_OPTIONS, &message),
    };
    let idempotency_key =
        body_key.or_else(|| head.idempotency_key.as_deref().filter(|v| !v.is_empty()));
    if let Some(key) = idempotency_key {
        if let Err(message) = validate_idempotency_key(key) {
            return err(CODE_INVALID_OPTIONS, &message);
        }
    }
    let idempotency_key = idempotency_key.map(str::to_string);

    // Idempotent replay: a retried upload returns the stored result
    // without spending quota or provider inference again.
    if let (Some(requester), Some(key)) = (requester, &idempotency_key) {
        match state.db.estimate_by_idempotency(&requester.user.id, key) {
            Ok(Some(existing)) => {
                let mut replayed = stored_result_json(&existing);
                replayed["request_id"] = Value::from(request_id);
                return (200, replayed);
            }
            Ok(None) => {}
            Err(_) => {
                return (
                    502,
                    error_json(
                        CODE_ESTIMATION_FAILED,
                        "Could not check the idempotency key",
                        request_id,
                    ),
                )
            }
        }
    }

    let image_reference = image_url
        .filter(|value| !value.is_empty())
        .or_else(|| image_base64.filter(|value| !value.is_empty()));
    // Tape-only requests skip the image requirement entirely (offline field use).
    if image_reference.is_none() {
        if let Some((g, l)) = tape {
            let result = attach_profile(estimate_tape(g, l, &enriched_prompt), &profile);
            return (
                200,
                with_request_id(
                    save_authenticated(
                        state,
                        requester,
                        result,
                        &enriched_prompt,
                        animal_id,
                        measured_at,
                        &idempotency_key,
                        request_id,
                        "none",
                    ),
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
    let Some(image_reference) = image_reference else {
        return (
            400,
            error_json(
                CODE_MISSING_IMAGE,
                "Provide image_url, image_base64, or both heart_girth_cm and body_length_cm",
                request_id,
            ),
        );
    };

    let mut request_config = state.config.clone();
    let mut ollama_url_override: Option<String> = None;
    for (field, target) in [
        ("backend", &mut request_config.backend),
        ("model", &mut request_config.model),
        ("ollama_url", &mut request_config.ollama_url),
    ] {
        let value = match optional_string(payload, field) {
            Ok(value) => value,
            Err(message) => {
                return err(CODE_INVALID_OPTIONS, &message);
            }
        };
        if let Some(value) = value {
            if field == "ollama_url" {
                ollama_url_override = Some(value.to_string());
            }
            *target = value.to_string();
        }
    }
    let api_key = match optional_string(payload, "ollama_api_key") {
        Ok(value) => value,
        Err(message) => {
            return err(CODE_INVALID_OPTIONS, &message);
        }
    };
    let explicit_key = matches!(api_key, Some(value) if !value.is_empty());
    if let Some(value) = api_key {
        request_config.ollama_api_key = (!value.is_empty()).then(|| value.to_string());
    }
    // Never forward the server-side API key to a different host than the one
    // the server is configured for. A per-request `ollama_url` pointing
    // elsewhere (local Ollama, test double, unrelated service) must not
    // receive the server's bearer token unless the same request supplies its
    // own key explicitly.
    if let Some(url) = ollama_url_override {
        // Per-request overrides additionally refuse private/local targets
        // (same SSRF policy as `image_url`): point the *server* at a local
        // Ollama via configuration instead of smuggling it per request.
        if check_image_url_allowed(&url).is_err() {
            return (
                400,
                error_json(
                    CODE_INVALID_OPTIONS,
                    "Ollama URL host is blocked (private or local address).",
                    request_id,
                ),
            );
        }
        if url_host(&url) != url_host(&state.config.ollama_url) && !explicit_key {
            request_config.ollama_api_key = None;
        }
    }

    if let Err(message) = validate_runtime_config(&request_config, &enriched_prompt) {
        return err(CODE_INVALID_OPTIONS, &message);
    }

    let needs_provider = request_config.backend == "ollama";
    if needs_provider && state.inference_paused() {
        return (
            503,
            error_json(
                CODE_PAUSED,
                "AI inference is paused by the operator; tape estimates still work",
                request_id,
            ),
        );
    }
    // Quotas count provider-bound photo estimates per user per day.
    // Anonymous local estimates are unmetered; tape-only estimates are free.
    if let Some(requester) = requester {
        let used = state
            .db
            .daily_usage(&requester.user.id, &utc_day())
            .unwrap_or(0);
        if used >= state.config.daily_estimate_limit as i64 {
            return (
                429,
                error_json(
                    CODE_QUOTA,
                    "Daily estimate limit reached; try again tomorrow",
                    request_id,
                ),
            );
        }
        if state.db.bump_usage(&requester.user.id, &utc_day()).is_err() {
            return (
                502,
                error_json(CODE_ESTIMATION_FAILED, "Could not record usage", request_id),
            );
        }
    }
    // Bound provider concurrency; tape/fallback never take this slot.
    let _slot = if needs_provider {
        match state.inference_gate.try_acquire() {
            Some(guard) => Some(guard),
            None => {
                return (
                    503,
                    error_json(
                        CODE_SERVER_BUSY,
                        "Inference is at capacity; try again shortly",
                        request_id,
                    ),
                )
            }
        }
    } else {
        None
    };

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
            let provider = request_config.backend.clone();
            let saved = save_authenticated(
                state,
                requester,
                result,
                &enriched_prompt,
                animal_id,
                measured_at,
                &idempotency_key,
                request_id,
                &provider,
            );
            (200, with_request_id(saved, request_id))
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

/// Persist an authenticated estimate to server-side history. Anonymous
/// results are returned unsaved. A failed insert never fails the estimate
/// itself; a key race returns the winning row (replay semantics).
#[allow(clippy::too_many_arguments)]
fn save_authenticated(
    state: &ServerState,
    requester: &Option<Requester>,
    mut result: Value,
    prompt_used: &str,
    animal_id: Option<&str>,
    measured_at: Option<String>,
    idempotency_key: &Option<String>,
    request_id: &str,
    provider: &str,
) -> Value {
    let requester = match requester {
        Some(r) => r,
        None => return result,
    };
    let stored_prompt = if prompt_used.len() > STORED_PROMPT_BYTES {
        format!("{}…[truncated]", &prompt_used[..STORED_PROMPT_BYTES])
    } else {
        prompt_used.to_string()
    };
    let ctx = SaveContext {
        animal_id: animal_id.map(str::to_string),
        measured_at,
        idempotency_key: idempotency_key.clone(),
        request_id: request_id.to_string(),
        provider: provider.to_string(),
    };
    let Some(row) = row_from_result(&requester.user.id, &result, &ctx, &stored_prompt) else {
        return result;
    };
    match state.db.insert_estimate(&row) {
        Ok(true) => {
            result["history_id"] = Value::from(row.id.clone());
            result["saved"] = Value::from(true);
            result
        }
        _ => {
            // Key race (UNIQUE) or other insert failure: a duplicate key
            // means another request won — replay it. Any other failure
            // still returns the estimate itself, just unsaved.
            match idempotency_key
                .as_deref()
                .and_then(|k| state.db.estimate_by_idempotency(&requester.user.id, k).ok())
                .flatten()
            {
                Some(existing) => {
                    let mut replayed = stored_result_json(&existing);
                    replayed["request_id"] = Value::from(request_id);
                    replayed
                }
                None => result,
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
        // `validate_runtime_config` runs before dispatch, so any other value
        // is a bug — but return a 502 rather than panicking the worker.
        _ => Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("Unsupported backend: {}", request_config.backend),
        )) as Box<dyn std::error::Error>),
    }
}
