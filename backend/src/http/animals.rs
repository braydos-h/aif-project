//! Private animal records: one owner, one trend per animal.
//!
//! Estimates carry an optional `animal_id` so a weight trend never mixes
//! unrelated cows. Scale (verified) weights are recorded as first-class
//! history rows with `source == "scale"` so photo/tape guesses can be
//! compared against the scale, never merged into it.

use serde_json::{json, Value};

use super::history::{estimate_to_json, split_query};
use super::json_util::{need_json, opt_num, opt_str};
use super::response::{error_json, with_request_id, Response};
use super::server::RequestHead;
use super::session::{check_csrf, require_auth};
use super::validation::ALLOWED_SEXES;
use super::ServerState;
use crate::auth::new_id;
use crate::config::{ESTIMATOR_VERSION, PROMPT_VERSION};
use crate::db::{Animal, HistoryFilter};
use crate::tape::DISCLAIMER;
use crate::time_util::{rfc3339, unix_now};

fn animal_json(a: &Animal, estimate_count: i64) -> Value {
    json!({
        "id": a.id,
        "name": a.name,
        "breed": a.breed,
        "sex": a.sex,
        "birth_year": a.birth_year,
        "notes": a.notes,
        "archived": a.archived,
        "created_at": a.created_at,
        "updated_at": a.updated_at,
        "estimate_count": estimate_count,
    })
}

fn count_for(state: &ServerState, user_id: &str, animal_id: &str) -> i64 {
    let filter = HistoryFilter {
        animal_id: Some(animal_id.to_string()),
        page: 1,
        per_page: 1,
        ..HistoryFilter::default()
    };
    state
        .db
        .list_estimates(user_id, &filter)
        .map(|(_, total)| total)
        .unwrap_or(0)
}

fn not_found(state: &ServerState, request_id: &str) -> Response {
    Response::json(
        404,
        error_json(super::response::CODE_NOT_FOUND, "Not found", request_id),
    )
    .with_policy(state.config.production)
}

fn bad(state: &ServerState, request_id: &str, message: &str) -> Response {
    Response::json(
        400,
        error_json(super::response::CODE_INVALID_OPTIONS, message, request_id),
    )
    .with_policy(state.config.production)
}

fn valid_name(name: &str) -> bool {
    let t = name.trim();
    !t.is_empty() && t.len() <= 64 && !t.bytes().any(|b| b.is_ascii_control())
}

fn valid_breed(breed: &str) -> bool {
    let t = breed.trim();
    !t.is_empty()
        && t.len() <= 64
        && t.chars()
            .all(|c| c.is_ascii_alphabetic() || c == ' ' || c == '-' || c == '\'')
        && t.chars().any(|c| c.is_ascii_alphabetic())
}

fn valid_notes(notes: &str) -> bool {
    notes.len() <= 500 && !notes.bytes().any(|b| b.is_ascii_control() && b != b'\n')
}

fn valid_birth_year(year: i64) -> bool {
    (1900..=2100).contains(&year)
}

/// Fetch one animal owned by the caller (others' ids 404, no oracle).
fn owned_animal(
    state: &ServerState,
    user_id: &str,
    id: &str,
    request_id: &str,
) -> Result<Animal, Response> {
    if id.len() > 64 {
        return Err(not_found(state, request_id));
    }
    match state.db.animal_by_id(id) {
        Ok(Some(a)) if a.user_id == user_id => Ok(a),
        _ => Err(not_found(state, request_id)),
    }
}

/// Parse the editable animal fields from a JSON body.
struct AnimalFields {
    name: String,
    breed: Option<String>,
    sex: Option<String>,
    birth_year: Option<i64>,
    notes: Option<String>,
    archived: bool,
}

fn parse_fields(
    state: &ServerState,
    payload: &Value,
    request_id: &str,
    archived_default: bool,
) -> Result<AnimalFields, Response> {
    let name = opt_str(state, payload, "name", request_id)?
        .unwrap_or("")
        .trim()
        .to_string();
    if !valid_name(&name) {
        return Err(bad(
            state,
            request_id,
            "Animal name must be 1-64 characters without control characters",
        ));
    }
    let breed = match opt_str(state, payload, "breed", request_id)? {
        Some(raw) if !raw.trim().is_empty() => {
            if !valid_breed(raw) {
                return Err(bad(
                    state,
                    request_id,
                    "Breed may only contain letters, spaces, and hyphens (max 64)",
                ));
            }
            Some(raw.trim().to_string())
        }
        _ => None,
    };
    let sex = match opt_str(state, payload, "sex", request_id)? {
        Some(raw) if !raw.trim().is_empty() => {
            let lowered = raw.trim().to_ascii_lowercase();
            if !ALLOWED_SEXES.contains(&lowered.as_str()) {
                return Err(bad(
                    state,
                    request_id,
                    "Sex must be one of cow, bull, steer, heifer, calf, or unknown",
                ));
            }
            Some(lowered)
        }
        _ => None,
    };
    let birth_year = match payload.get("birth_year") {
        None | Some(Value::Null) => None,
        Some(Value::Number(n)) => {
            let year = n
                .as_i64()
                .ok_or_else(|| bad(state, request_id, "birth_year must be a whole year"))?;
            if !valid_birth_year(year) {
                return Err(bad(
                    state,
                    request_id,
                    "birth_year must be between 1900 and 2100",
                ));
            }
            Some(year)
        }
        Some(_) => return Err(bad(state, request_id, "birth_year must be a whole year")),
    };
    let notes = match opt_str(state, payload, "notes", request_id)? {
        Some(raw) if !raw.trim().is_empty() => {
            if !valid_notes(raw) {
                return Err(bad(state, request_id, "Notes must be under 500 characters"));
            }
            Some(raw.trim().to_string())
        }
        _ => None,
    };
    let archived = match payload.get("archived") {
        None | Some(Value::Null) => archived_default,
        Some(Value::Bool(b)) => *b,
        Some(_) => return Err(bad(state, request_id, "archived must be true or false")),
    };
    Ok(AnimalFields {
        name,
        breed,
        sex,
        birth_year,
        notes,
        archived,
    })
}

/// `GET /api/animals` — list the caller's animals.
pub(crate) fn handle_list(
    path: &str,
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let requester = match require_auth(state, head, request_id, peer_ip) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let (_, query) = split_query(path);
    let include_archived = query
        .iter()
        .any(|(k, v)| k == "include_archived" && (v == "1" || v == "true"));
    match state.db.list_animals(&requester.user.id, include_archived) {
        Ok(animals) => {
            let items: Vec<Value> = animals
                .iter()
                .map(|a| animal_json(a, count_for(state, &requester.user.id, &a.id)))
                .collect();
            Response::json(200, with_request_id(json!({"animals": items}), request_id))
                .with_policy(state.config.production)
        }
        Err(_) => Response::json(
            502,
            error_json(
                super::response::CODE_ESTIMATION_FAILED,
                "Could not load animals",
                request_id,
            ),
        )
        .with_policy(state.config.production),
    }
}

/// `POST /api/animals` — create an animal record.
pub(crate) fn handle_create(
    body: &[u8],
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let requester = match require_auth(state, head, request_id, peer_ip) {
        Ok(r) => r,
        Err(r) => return r,
    };
    if let Err(r) = check_csrf(state, head, &requester, request_id) {
        return r;
    }
    let payload = match need_json(state, body, request_id) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let fields = match parse_fields(state, &payload, request_id, false) {
        Ok(f) => f,
        Err(r) => return r,
    };
    match state.db.create_animal(
        &new_id(),
        &requester.user.id,
        &fields.name,
        fields.breed.as_deref(),
        fields.sex.as_deref(),
        fields.birth_year,
        fields.notes.as_deref(),
    ) {
        Ok(animal) => {
            state.audit(
                Some(&requester.user.id),
                "animal_created",
                &format!("animal={}", animal.id),
                request_id,
            );
            Response::json(200, with_request_id(animal_json(&animal, 0), request_id))
                .with_policy(state.config.production)
        }
        Err(_) => Response::json(
            502,
            error_json(
                super::response::CODE_ESTIMATION_FAILED,
                "Could not save the animal",
                request_id,
            ),
        )
        .with_policy(state.config.production),
    }
}

/// `GET /api/animals/{id}` — one owned animal with its estimate history.
pub(crate) fn handle_get(
    id: &str,
    path: &str,
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let requester = match require_auth(state, head, request_id, peer_ip) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let animal = match owned_animal(state, &requester.user.id, id, request_id) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let (_, query) = split_query(path);
    let page = query
        .iter()
        .find(|(k, _)| k == "page")
        .and_then(|(_, v)| v.parse::<i64>().ok())
        .unwrap_or(1)
        .clamp(1, 1_000_000);
    let filter = HistoryFilter {
        animal_id: Some(animal.id.clone()),
        page,
        per_page: 50,
        ..HistoryFilter::default()
    };
    let (items, total) = state
        .db
        .list_estimates(&requester.user.id, &filter)
        .unwrap_or_default();
    let rendered: Vec<Value> = items.iter().map(estimate_to_json).collect();
    let mut out = animal_json(&animal, total);
    out["estimates"] = Value::from(rendered);
    out["estimates_total"] = Value::from(total);
    Response::json(200, with_request_id(out, request_id)).with_policy(state.config.production)
}

/// `PUT /api/animals/{id}` — edit an owned animal.
pub(crate) fn handle_update(
    id: &str,
    body: &[u8],
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let requester = match require_auth(state, head, request_id, peer_ip) {
        Ok(r) => r,
        Err(r) => return r,
    };
    if let Err(r) = check_csrf(state, head, &requester, request_id) {
        return r;
    }
    let animal = match owned_animal(state, &requester.user.id, id, request_id) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let payload = match need_json(state, body, request_id) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let fields = match parse_fields(state, &payload, request_id, animal.archived) {
        Ok(f) => f,
        Err(r) => return r,
    };
    if state
        .db
        .update_animal(
            &animal.id,
            &fields.name,
            fields.breed.as_deref(),
            fields.sex.as_deref(),
            fields.birth_year,
            fields.notes.as_deref(),
            fields.archived,
        )
        .is_err()
    {
        return Response::json(
            502,
            error_json(
                super::response::CODE_ESTIMATION_FAILED,
                "Could not save the animal",
                request_id,
            ),
        )
        .with_policy(state.config.production);
    }
    state.audit(
        Some(&requester.user.id),
        "animal_updated",
        &format!("animal={}", animal.id),
        request_id,
    );
    match state.db.animal_by_id(&animal.id) {
        Ok(Some(updated)) => Response::json(
            200,
            with_request_id(
                animal_json(&updated, count_for(state, &requester.user.id, &updated.id)),
                request_id,
            ),
        )
        .with_policy(state.config.production),
        _ => not_found(state, request_id),
    }
}

/// `DELETE /api/animals/{id}` — delete an owned animal. Linked estimates
/// keep their rows with `animal_id` cleared so history is never silently
/// lost with the animal.
pub(crate) fn handle_delete(
    id: &str,
    body: &[u8],
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let _ = body;
    let requester = match require_auth(state, head, request_id, peer_ip) {
        Ok(r) => r,
        Err(r) => return r,
    };
    if let Err(r) = check_csrf(state, head, &requester, request_id) {
        return r;
    }
    match owned_animal(state, &requester.user.id, id, request_id) {
        Ok(animal) => {
            if state.db.delete_animal(&animal.id).is_err() {
                return Response::json(
                    502,
                    error_json(
                        super::response::CODE_ESTIMATION_FAILED,
                        "Could not delete the animal",
                        request_id,
                    ),
                )
                .with_policy(state.config.production);
            }
            state.audit(
                Some(&requester.user.id),
                "animal_deleted",
                &format!("animal={}", animal.id),
                request_id,
            );
            Response::json(200, with_request_id(json!({"ok": true}), request_id))
                .with_policy(state.config.production)
        }
        Err(r) => r,
    }
}

/// `POST /api/animals/{id}/measurements` — record a verified scale weight
/// for an owned animal. This is a measurement, not an AI guess: it is
/// stored with `source == "scale"` and an exact (zero-width) range.
pub(crate) fn handle_add_scale(
    id: &str,
    body: &[u8],
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let requester = match require_auth(state, head, request_id, peer_ip) {
        Ok(r) => r,
        Err(r) => return r,
    };
    if let Err(r) = check_csrf(state, head, &requester, request_id) {
        return r;
    }
    let animal = match owned_animal(state, &requester.user.id, id, request_id) {
        Ok(a) => a,
        Err(r) => return r,
    };
    let payload = match need_json(state, body, request_id) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let weight = match opt_num(state, &payload, "scale_weight_kg", request_id) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let weight = match weight {
        Some(w) if w.is_finite() && (20.0..=2500.0).contains(&w) => (w * 10.0).round() / 10.0,
        _ => {
            return bad(
                state,
                request_id,
                "scale_weight_kg must be a finite weight between 20 and 2500 kg",
            )
        }
    };
    let now = rfc3339(unix_now());
    let measured_at = match opt_str(state, &payload, "measured_at", request_id) {
        Ok(Some(raw)) if !raw.trim().is_empty() => {
            let value = raw.trim();
            // Loose RFC 3339 shape check; lexicographic compare works on UTC.
            if value.len() < 10
                || value.len() > 30
                || !value.starts_with(|c: char| c.is_ascii_digit())
            {
                return bad(
                    state,
                    request_id,
                    "measured_at must be an RFC 3339 UTC timestamp",
                );
            }
            if value > rfc3339(unix_now() + 3600).as_str() {
                return bad(state, request_id, "measured_at cannot be in the future");
            }
            value.to_string()
        }
        Ok(_) => now.clone(),
        Err(r) => return r,
    };
    let row = crate::db::Estimate {
        id: new_id(),
        user_id: requester.user.id.clone(),
        animal_id: Some(animal.id.clone()),
        created_at: now,
        measured_at: Some(measured_at),
        weight_kg: weight,
        weight_min_kg: weight,
        weight_max_kg: weight,
        source: "scale".to_string(),
        method: "scale_manual".to_string(),
        model: None,
        provider: Some("manual".to_string()),
        estimator_version: ESTIMATOR_VERSION.to_string(),
        prompt_version: PROMPT_VERSION.to_string(),
        prompt_used: String::new(),
        heart_girth_cm: None,
        body_length_cm: None,
        confidence: None,
        breed: None,
        body_condition_score: None,
        animal_breed: None,
        animal_sex: None,
        animal_age_years: None,
        scale_weight_kg: Some(weight),
        placeholder: false,
        disclaimer: DISCLAIMER.to_string(),
        idempotency_key: None,
        request_id: request_id.to_string(),
    };
    if state.db.insert_estimate(&row).is_err() {
        return Response::json(
            502,
            error_json(
                super::response::CODE_ESTIMATION_FAILED,
                "Could not save the measurement",
                request_id,
            ),
        )
        .with_policy(state.config.production);
    }
    state.audit(
        Some(&requester.user.id),
        "scale_recorded",
        &format!("animal={}", animal.id),
        request_id,
    );
    Response::json(200, with_request_id(estimate_to_json(&row), request_id))
        .with_policy(state.config.production)
}
