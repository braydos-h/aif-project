//! Account-owned estimate history: persistence, listing, detail,
//! deletion, and spreadsheet-safe CSV export.
//!
//! Estimates are saved only for authenticated callers (anonymous local
//! estimates are never persisted). Deterministic `local_fallback` results
//! are stored with `placeholder = true` and always rendered with an
//! explicit placeholder label — they never masquerade as AI measurements.
//! Dates are stored as UTC RFC 3339; the WebUI displays them in the
//! viewer's timezone.

use serde_json::{json, Value};

use super::csv::{csv_number, csv_number_req, csv_text};
use super::response::{error_json, with_request_id, Response};
use super::server::RequestHead;
use super::session::{check_csrf, require_auth};
use super::ServerState;
use crate::auth::new_id;
use crate::config::{ESTIMATOR_VERSION, PROMPT_VERSION};
use crate::db::{Estimate, HistoryFilter};
use crate::time_util::{rfc3339, unix_now};

/// Context gathered while handling an estimate, recorded alongside the
/// saved row.
pub(crate) struct SaveContext {
    pub(crate) animal_id: Option<String>,
    pub(crate) measured_at: Option<String>,
    pub(crate) idempotency_key: Option<String>,
    pub(crate) request_id: String,
    pub(crate) provider: String,
}

/// Build a history row from a successful estimate result. Returns `None`
/// when the result lacks the numeric fields a history row requires.
pub(crate) fn row_from_result(
    user_id: &str,
    result: &Value,
    ctx: &SaveContext,
    prompt_used: &str,
) -> Option<Estimate> {
    let weight_kg = result.get("estimated_weight_kg")?.as_f64()?;
    let min_kg = result.get("weight_min_kg")?.as_f64()?;
    let max_kg = result.get("weight_max_kg")?.as_f64()?;
    if !weight_kg.is_finite() || !min_kg.is_finite() || !max_kg.is_finite() {
        return None;
    }
    let source = result
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let str_field = |name: &str| {
        result
            .get(name)
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
    let num_field = |name: &str| {
        result
            .get(name)
            .and_then(|v| v.as_f64())
            .filter(|v| v.is_finite())
    };
    Some(Estimate {
        id: new_id(),
        user_id: user_id.to_string(),
        animal_id: ctx.animal_id.clone(),
        created_at: rfc3339(unix_now()),
        measured_at: ctx.measured_at.clone(),
        weight_kg,
        weight_min_kg: min_kg,
        weight_max_kg: max_kg,
        source: source.clone(),
        method: str_field("method").unwrap_or_else(|| {
            if source == "tape_measure" {
                "schaeffer_tape".to_string()
            } else if source == "local_fallback" {
                "fallback_hash".to_string()
            } else {
                "ollama_photo".to_string()
            }
        }),
        model: str_field("model"),
        provider: Some(ctx.provider.clone()),
        estimator_version: ESTIMATOR_VERSION.to_string(),
        prompt_version: PROMPT_VERSION.to_string(),
        prompt_used: prompt_used.to_string(),
        heart_girth_cm: num_field("heart_girth_cm"),
        body_length_cm: num_field("body_length_cm"),
        confidence: num_field("confidence"),
        breed: str_field("breed"),
        body_condition_score: num_field("body_condition_score"),
        animal_breed: str_field("animal_breed"),
        animal_sex: str_field("animal_sex"),
        animal_age_years: num_field("animal_age_years"),
        scale_weight_kg: None,
        placeholder: source == "local_fallback",
        disclaimer: str_field("disclaimer").unwrap_or_default(),
        idempotency_key: ctx.idempotency_key.clone(),
        request_id: ctx.request_id.clone(),
    })
}

/// Render a history row for API responses (includes the placeholder flag
/// and version stamps so exports stay interpretable after model changes).
pub(crate) fn estimate_to_json(e: &Estimate) -> Value {
    json!({
        "id": e.id,
        "created_at": e.created_at,
        "measured_at": e.measured_at,
        "weight_kg": e.weight_kg,
        "weight_min_kg": e.weight_min_kg,
        "weight_max_kg": e.weight_max_kg,
        "source": e.source,
        "method": e.method,
        "model": e.model,
        "provider": e.provider,
        "estimator_version": e.estimator_version,
        "prompt_version": e.prompt_version,
        "heart_girth_cm": e.heart_girth_cm,
        "body_length_cm": e.body_length_cm,
        "confidence": e.confidence,
        "breed": e.breed,
        "body_condition_score": e.body_condition_score,
        "animal_id": e.animal_id,
        "animal_breed": e.animal_breed,
        "animal_sex": e.animal_sex,
        "animal_age_years": e.animal_age_years,
        "scale_weight_kg": e.scale_weight_kg,
        "placeholder": e.placeholder,
        "disclaimer": e.disclaimer,
        "request_id": e.request_id,
    })
}

/// Replay an idempotently stored result: the original estimate JSON plus a
/// `replayed: true` marker and the stored history id.
pub(crate) fn stored_result_json(e: &Estimate) -> Value {
    let mut out = estimate_to_json(e);
    out["estimated_weight_kg"] = Value::from(e.weight_kg);
    out["replayed"] = Value::from(true);
    out["history_id"] = Value::from(e.id.clone());
    out
}

/// Split `path` into the part before `?` and parsed query pairs.
pub(crate) fn split_query(path: &str) -> (&str, Vec<(String, String)>) {
    match path.split_once('?') {
        Some((base, query)) => (base, parse_query(query)),
        None => (path, Vec::new()),
    }
}

fn parse_query(query: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (key, value) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => (pair, ""),
        };
        out.push((url_decode(key), url_decode(value)));
    }
    out
}

fn url_decode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hex = &input[i + 1..i + 3];
                match u8::from_str_radix(hex, 16) {
                    Ok(b) => {
                        out.push(b as char);
                        i += 3;
                    }
                    Err(_) => {
                        out.push('%');
                        i += 1;
                    }
                }
            }
            _ => {
                out.push(bytes[i] as char);
                i += 1;
            }
        }
    }
    out
}

fn query_value(query: &[(String, String)], key: &str) -> Option<String> {
    query
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
        .filter(|v| !v.is_empty() && v.len() <= 256)
}

/// `GET /api/history` — paginated, filterable history for the caller.
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
    // Page/per-page are bounded numerics; anything else falls back to defaults.
    let page = query_value(&query, "page")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(1)
        .clamp(1, 1_000_000);
    let per_page = query_value(&query, "per_page")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(20)
        .clamp(1, 100);
    let filter = HistoryFilter {
        animal_id: query_value(&query, "animal_id"),
        source: query_value(&query, "source").filter(|s| {
            matches!(
                s.as_str(),
                "ollama" | "local_fallback" | "tape_measure" | "scale"
            )
        }),
        from: query_value(&query, "from"),
        to: query_value(&query, "to"),
        page,
        per_page,
    };
    match state.db.list_estimates(&requester.user.id, &filter) {
        Ok((items, total)) => {
            let rendered: Vec<Value> = items.iter().map(estimate_to_json).collect();
            Response::json(
                200,
                with_request_id(
                    json!({"items": rendered, "page": page, "per_page": per_page, "total": total}),
                    request_id,
                ),
            )
            .with_policy(state.config.production)
        }
        Err(_) => Response::json(
            502,
            error_json(
                super::response::CODE_ESTIMATION_FAILED,
                "Could not load history",
                request_id,
            ),
        )
        .with_policy(state.config.production),
    }
}

/// Fetch one owned row or answer 404 (cross-user ids also 404 so record
/// existence never leaks across accounts).
fn owned_estimate(
    state: &ServerState,
    user_id: &str,
    id: &str,
    request_id: &str,
) -> Result<Estimate, Response> {
    if id.len() > 64 {
        return Err(not_found(state, request_id));
    }
    match state.db.estimate_by_id(id) {
        Ok(Some(row)) if row.user_id == user_id => Ok(row),
        _ => Err(not_found(state, request_id)),
    }
}

fn not_found(state: &ServerState, request_id: &str) -> Response {
    Response::json(
        404,
        error_json(super::response::CODE_NOT_FOUND, "Not found", request_id),
    )
    .with_policy(state.config.production)
}

/// `GET /api/history/{id}` — detail view for one owned estimate.
pub(crate) fn handle_get(
    id: &str,
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let requester = match require_auth(state, head, request_id, peer_ip) {
        Ok(r) => r,
        Err(r) => return r,
    };
    match owned_estimate(state, &requester.user.id, id, request_id) {
        Ok(row) => Response::json(200, with_request_id(estimate_to_json(&row), request_id))
            .with_policy(state.config.production),
        Err(r) => r,
    }
}

/// `DELETE /api/history/{id}` — delete one owned estimate.
pub(crate) fn handle_delete(
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
    let _ = body;
    match owned_estimate(state, &requester.user.id, id, request_id) {
        Ok(_) => {
            // Fetch linked photos first: estimate deletion cascades their
            // rows via FK, so files must be collected beforehand.
            let linked = state.db.photos_for_estimate(id).unwrap_or_default();
            match state.db.delete_estimate(id) {
                Ok(true) => {
                    for photo in linked {
                        let _ = crate::photos::delete_photo(
                            &state.db,
                            &state.config.data_dir,
                            &photo.id,
                        );
                    }
                    state.audit(
                        Some(&requester.user.id),
                        "history_deleted",
                        &format!("estimate={}", id),
                        request_id,
                    );
                    Response::json(200, with_request_id(json!({"ok": true}), request_id))
                        .with_policy(state.config.production)
                }
                _ => not_found(state, request_id),
            }
        }
        Err(r) => r,
    }
}

/// `GET /api/history/export?format=csv` — owned CSV export with
/// spreadsheet-safe text fields and numeric measurement columns.
pub(crate) fn handle_export(
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
    let filter = HistoryFilter {
        animal_id: query_value(&query, "animal_id"),
        source: query_value(&query, "source").filter(|s| {
            matches!(
                s.as_str(),
                "ollama" | "local_fallback" | "tape_measure" | "scale"
            )
        }),
        from: query_value(&query, "from"),
        to: query_value(&query, "to"),
        page: 1,
        per_page: 100,
    };
    // Page through the full filtered set (bounded per query above).
    let mut all = Vec::new();
    let mut page = 1i64;
    loop {
        let f = HistoryFilter {
            animal_id: filter.animal_id.clone(),
            source: filter.source.clone(),
            from: filter.from.clone(),
            to: filter.to.clone(),
            page,
            per_page: 100,
        };
        match state.db.list_estimates(&requester.user.id, &f) {
            Ok((items, total)) => {
                all.extend(items);
                if all.len() as i64 >= total || page > 100 {
                    break;
                }
                page += 1;
            }
            Err(_) => {
                return Response::json(
                    502,
                    error_json(
                        super::response::CODE_ESTIMATION_FAILED,
                        "Could not export history",
                        request_id,
                    ),
                )
                .with_policy(state.config.production)
            }
        }
    }
    let mut out = String::from(
        "created_at,measured_at,id,animal_id,weight_kg,weight_min_kg,weight_max_kg,source,method,model,provider,estimator_version,prompt_version,heart_girth_cm,body_length_cm,confidence,breed,body_condition_score,animal_breed,animal_sex,animal_age_years,scale_weight_kg,placeholder,disclaimer,request_id\r\n",
    );
    for e in all.iter().rev() {
        let fields = vec![
            csv_text(&e.created_at),
            csv_text(e.measured_at.as_deref().unwrap_or("")),
            csv_text(&e.id),
            csv_text(e.animal_id.as_deref().unwrap_or("")),
            csv_number_req(e.weight_kg),
            csv_number_req(e.weight_min_kg),
            csv_number_req(e.weight_max_kg),
            csv_text(&e.source),
            csv_text(&e.method),
            csv_text(e.model.as_deref().unwrap_or("")),
            csv_text(e.provider.as_deref().unwrap_or("")),
            csv_text(&e.estimator_version),
            csv_text(&e.prompt_version),
            csv_number(e.heart_girth_cm),
            csv_number(e.body_length_cm),
            csv_number(e.confidence),
            csv_text(e.breed.as_deref().unwrap_or("")),
            csv_number(e.body_condition_score),
            csv_text(e.animal_breed.as_deref().unwrap_or("")),
            csv_text(e.animal_sex.as_deref().unwrap_or("")),
            csv_number(e.animal_age_years),
            csv_number(e.scale_weight_kg),
            if e.placeholder {
                "1".to_string()
            } else {
                "0".to_string()
            },
            csv_text(&e.disclaimer),
            csv_text(&e.request_id),
        ];
        out.push_str(&fields.join(","));
        out.push_str("\r\n");
    }
    state.audit(
        Some(&requester.user.id),
        "history_exported",
        &format!("rows={}", all.len()),
        request_id,
    );
    Response::csv(200, out.into_bytes(), "cow-weight-history.csv")
        .with_policy(state.config.production)
}
