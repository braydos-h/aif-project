//! Self-service account controls: data export and account deletion.
//!
//! Export returns everything the account owns (profile, animals, estimate
//! history) as JSON — no password hashes, session tokens, or invite
//! material. Deletion requires the current password (reauthentication),
//! revokes all sessions, and removes all owned rows in one transaction.

use serde_json::{json, Value};

use super::history::estimate_to_json;
use super::json_util::need_json;
use super::response::{error_json, with_request_id, Response};
use super::server::RequestHead;
use super::session::{check_csrf, require_auth};
use super::ServerState;
use crate::db::HistoryFilter;
use crate::time_util::utc_day;

/// `GET /api/account` — profile summary with usage and retention pointers.
pub(crate) fn handle_summary(
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let requester = match require_auth(state, head, request_id, peer_ip) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let usage = state
        .db
        .daily_usage(&requester.user.id, &utc_day())
        .unwrap_or(0);
    let animals = state
        .db
        .list_animals(&requester.user.id, true)
        .map(|a| a.len() as i64)
        .unwrap_or(0);
    let estimates = state.db.estimate_count(&requester.user.id).unwrap_or(0);
    Response::json(
        200,
        with_request_id(
            json!({
                "id": requester.user.id,
                "email": requester.user.email,
                "display_name": requester.user.display_name,
                "role": requester.user.role,
                "created_at": requester.user.created_at,
                "last_login_at": requester.user.last_login_at,
                "animals": animals,
                "estimates": estimates,
                "today_image_estimates": usage,
                "daily_limit": state.config.daily_estimate_limit,
                "photo_policy": "transient-only: uploaded photos are sent to the AI provider for the estimate and are never stored on this server",
            }),
            request_id,
        ),
    )
    .with_policy(state.config.production)
}

/// `GET /api/account/export` — full owned-data JSON export.
pub(crate) fn handle_export(
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let requester = match require_auth(state, head, request_id, peer_ip) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let animals = state
        .db
        .list_animals(&requester.user.id, true)
        .unwrap_or_default();
    let animal_items: Vec<Value> = animals
        .iter()
        .map(|a| {
            json!({
                "id": a.id, "name": a.name, "breed": a.breed, "sex": a.sex,
                "birth_year": a.birth_year, "notes": a.notes, "archived": a.archived,
                "created_at": a.created_at, "updated_at": a.updated_at,
            })
        })
        .collect();
    // All estimates, oldest first (bounded: 10k rows is far beyond two
    // users' plausible history; larger exports can repeat with filters).
    let mut all = Vec::new();
    for page in 1..=100 {
        let filter = HistoryFilter {
            page,
            per_page: 100,
            ..HistoryFilter::default()
        };
        match state.db.list_estimates(&requester.user.id, &filter) {
            Ok((items, total)) => {
                all.extend(items);
                if all.len() as i64 >= total {
                    break;
                }
            }
            Err(_) => {
                return Response::json(
                    502,
                    error_json(
                        super::response::CODE_ESTIMATION_FAILED,
                        "Could not export account data",
                        request_id,
                    ),
                )
                .with_policy(state.config.production)
            }
        }
    }
    all.reverse();
    let estimate_items: Vec<Value> = all.iter().map(estimate_to_json).collect();
    state.audit(
        Some(&requester.user.id),
        "account_exported",
        &format!(
            "animals={} estimates={}",
            animal_items.len(),
            estimate_items.len()
        ),
        request_id,
    );
    Response::json(
        200,
        with_request_id(
            json!({
                "user": {
                    "id": requester.user.id,
                    "email": requester.user.email,
                    "display_name": requester.user.display_name,
                    "role": requester.user.role,
                    "created_at": requester.user.created_at,
                },
                "animals": animal_items,
                "estimates": estimate_items,
                "exported_at": crate::time_util::rfc3339(crate::time_util::unix_now()),
            }),
            request_id,
        ),
    )
    .with_policy(state.config.production)
}

/// `DELETE /api/account` — reauthenticated self-deletion. Requires the
/// current password, then revokes every session and deletes all owned data
/// (estimates, animals, counters, recovery tokens) in one transaction.
pub(crate) fn handle_delete(
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
    let password = super::json_util::opt_str(state, &payload, "password", request_id)
        .unwrap_or(None)
        .unwrap_or("")
        .to_string();
    let stored = match state.db.user_by_email_with_hash(&requester.user.email) {
        Ok(Some((_, hash))) => hash,
        _ => {
            return Response::json(
                401,
                error_json(
                    super::response::CODE_INVALID_CREDENTIALS,
                    "Password is incorrect",
                    request_id,
                ),
            )
            .with_policy(state.config.production)
        }
    };
    if !crate::auth::verify_password(&password, &stored) {
        return Response::json(
            401,
            error_json(
                super::response::CODE_INVALID_CREDENTIALS,
                "Password is incorrect",
                request_id,
            ),
        )
        .with_policy(state.config.production);
    }
    state.audit(
        Some(&requester.user.id),
        "account_deleted",
        &format!("email={}", requester.user.email),
        request_id,
    );
    match state.db.delete_account(&requester.user.id) {
        Ok(_) => Response::json(200, with_request_id(json!({"ok": true}), request_id))
            .with_header(
                "Set-Cookie",
                &crate::auth::clear_cookie_value(state.config.cookie_secure),
            )
            .with_policy(state.config.production),
        Err(_) => Response::json(
            502,
            error_json(
                super::response::CODE_ESTIMATION_FAILED,
                "Could not delete the account",
                request_id,
            ),
        )
        .with_policy(state.config.production),
    }
}
