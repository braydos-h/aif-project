//! Operator controls: invites, users, usage, inference pause, audit.
//!
//! All routes require the `operator` role. Invite and recovery tokens never
//! appear in logs or audit entries — only invite ids and emails are
//! recorded. The invite URL itself is returned once in the creation
//! response for out-of-band delivery by the operator.

use serde_json::{json, Value};

use super::json_util::{need_json, opt_str};
use super::response::{error_json, with_request_id, Response};
use super::server::RequestHead;
use super::session::{check_csrf, require_auth, require_operator};
use super::ServerState;
use crate::auth::{new_id, normalize_email, random_hex, token_hash, valid_email, TOKEN_BYTES};
use crate::time_util::{rfc3339, unix_now};

fn authed_operator(
    state: &ServerState,
    head: &RequestHead,
    request_id: &str,
    peer_ip: &str,
) -> Result<super::session::Requester, Response> {
    let requester = require_auth(state, head, request_id, peer_ip)?;
    check_csrf(state, head, &requester, request_id)?;
    require_operator(state, &requester, request_id)?;
    Ok(requester)
}

fn invite_status(inv: &crate::db::Invite, now: &str) -> &'static str {
    if inv.used_at.is_some() {
        "used"
    } else if inv.revoked_at.is_some() {
        "revoked"
    } else if inv.expires_at.as_str() < now {
        "expired"
    } else {
        "active"
    }
}

/// `POST /api/operator/invites` — mint a single-use invite. The raw token
/// is returned once as `invite_url` (fragment, never sent to the server);
/// only its hash is stored.
pub(crate) fn handle_create_invite(
    body: &[u8],
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let requester = match authed_operator(state, head, request_id, peer_ip) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let payload = match need_json(state, body, request_id) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let email = normalize_email(
        opt_str(state, &payload, "email", request_id)
            .unwrap_or(None)
            .unwrap_or(""),
    );
    if !valid_email(&email) {
        return bad(state, request_id, "email must be a valid email address");
    }
    let role = opt_str(state, &payload, "role", request_id)
        .unwrap_or(None)
        .unwrap_or("user")
        .to_ascii_lowercase();
    if role != "user" && role != "operator" {
        return bad(state, request_id, "role must be user or operator");
    }
    let token = random_hex(TOKEN_BYTES);
    let expires_at = rfc3339(unix_now() + state.config.invite_days * 86_400);
    let id = new_id();
    if state
        .db
        .create_invite(
            &id,
            &email,
            &role,
            &token_hash(&token),
            Some(&requester.user.id),
            &expires_at,
        )
        .is_err()
    {
        return Response::json(
            502,
            error_json(
                super::response::CODE_ESTIMATION_FAILED,
                "Could not create the invite",
                request_id,
            ),
        )
        .with_policy(state.config.production);
    }
    let origin = state.config.public_origin.trim_end_matches('/');
    let invite_url = format!("{}/#invite={}", origin, token);
    state.audit(
        Some(&requester.user.id),
        "invite_created",
        &format!("invite={} email={} role={}", id, email, role),
        request_id,
    );
    Response::json(
        200,
        with_request_id(
            json!({
                "id": id,
                "email": email,
                "role": role,
                "expires_at": expires_at,
                "invite_url": invite_url,
                "warning": "This link is shown once. Deliver it to the recipient over a private channel.",
            }),
            request_id,
        ),
    )
    .with_policy(state.config.production)
}

/// `GET /api/operator/invites` — invite list with status (no token material).
pub(crate) fn handle_list_invites(
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let _ = match authed_operator(state, head, request_id, peer_ip) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let now = rfc3339(unix_now());
    match state.db.list_invites() {
        Ok(invites) => {
            let items: Vec<Value> = invites
                .iter()
                .map(|inv| {
                    json!({
                        "id": inv.id,
                        "email": inv.email,
                        "role": inv.role,
                        "created_at": inv.created_at,
                        "expires_at": inv.expires_at,
                        "used_at": inv.used_at,
                        "revoked_at": inv.revoked_at,
                        "status": invite_status(inv, &now),
                    })
                })
                .collect();
            Response::json(200, with_request_id(json!({"invites": items}), request_id))
                .with_policy(state.config.production)
        }
        Err(_) => Response::json(
            502,
            error_json(
                super::response::CODE_ESTIMATION_FAILED,
                "Could not load invites",
                request_id,
            ),
        )
        .with_policy(state.config.production),
    }
}

/// `POST /api/operator/invites/{id}/revoke` — revoke an unused invite.
pub(crate) fn handle_revoke_invite(
    id: &str,
    body: &[u8],
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let _ = body;
    let requester = match authed_operator(state, head, request_id, peer_ip) {
        Ok(r) => r,
        Err(r) => return r,
    };
    if id.len() > 64 {
        return not_found(state, request_id);
    }
    match state.db.revoke_invite(id) {
        Ok(true) => {
            state.audit(
                Some(&requester.user.id),
                "invite_revoked",
                &format!("invite={}", id),
                request_id,
            );
            Response::json(200, with_request_id(json!({"ok": true}), request_id))
                .with_policy(state.config.production)
        }
        _ => not_found(state, request_id),
    }
}

/// `GET /api/operator/users` — account list for the operator.
pub(crate) fn handle_list_users(
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let _ = match authed_operator(state, head, request_id, peer_ip) {
        Ok(r) => r,
        Err(r) => return r,
    };
    match state.db.list_users() {
        Ok(users) => {
            let items: Vec<Value> = users
                .iter()
                .map(|u| {
                    json!({
                        "id": u.id,
                        "email": u.email,
                        "display_name": u.display_name,
                        "role": u.role,
                        "status": u.status,
                        "created_at": u.created_at,
                        "last_login_at": u.last_login_at,
                    })
                })
                .collect();
            Response::json(200, with_request_id(json!({"users": items}), request_id))
                .with_policy(state.config.production)
        }
        Err(_) => Response::json(
            502,
            error_json(
                super::response::CODE_ESTIMATION_FAILED,
                "Could not load users",
                request_id,
            ),
        )
        .with_policy(state.config.production),
    }
}

/// `DELETE /api/operator/users/{id}` — operator removes an account
/// (sessions revoked, data deleted). The operator cannot delete itself
/// here; self-service deletion lives under `/api/account`.
pub(crate) fn handle_delete_user(
    id: &str,
    body: &[u8],
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let _ = body;
    let requester = match authed_operator(state, head, request_id, peer_ip) {
        Ok(r) => r,
        Err(r) => return r,
    };
    if id.len() > 64 {
        return not_found(state, request_id);
    }
    if id == requester.user.id {
        return bad(
            state,
            request_id,
            "Use account deletion for your own account",
        );
    }
    match state.db.user_by_id(id) {
        Ok(Some(_)) => {
            let _ = state.db.delete_account(id);
            state.audit(
                Some(&requester.user.id),
                "user_deleted",
                &format!("user={}", id),
                request_id,
            );
            Response::json(200, with_request_id(json!({"ok": true}), request_id))
                .with_policy(state.config.production)
        }
        _ => not_found(state, request_id),
    }
}

/// `GET /api/operator/usage` — per-day inference usage and limits.
pub(crate) fn handle_usage(
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let _ = match authed_operator(state, head, request_id, peer_ip) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let rows = state.db.usage_rows(60).unwrap_or_default();
    let items: Vec<Value> = rows
        .iter()
        .map(|(user_id, day, count)| json!({"user_id": user_id, "day": day, "image_estimates": count}))
        .collect();
    Response::json(
        200,
        with_request_id(
            json!({
                "daily_limit_per_user": state.config.daily_estimate_limit,
                "max_concurrent_inference": state.config.max_concurrent_inference,
                "inference_paused": state.inference_paused(),
                "usage": items,
            }),
            request_id,
        ),
    )
    .with_policy(state.config.production)
}

/// `GET /api/operator/status` — health summary for the operator (no secrets).
pub(crate) fn handle_status(
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let _ = match authed_operator(state, head, request_id, peer_ip) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let active_users = state.db.active_user_count().unwrap_or(-1);
    Response::json(
        200,
        with_request_id(
            json!({
                "backend": state.config.backend,
                "model": state.config.model,
                "production": state.config.production,
                "inference_paused": state.inference_paused(),
                "active_users": active_users,
                "max_users": crate::db::MAX_ACTIVE_USERS,
                "uptime_secs": state.metrics.uptime_secs(),
                "total_requests": state.metrics.total(),
                "rejected_connections": state.metrics.rejected(),
                "cache_entries": state.cache.len(),
            }),
            request_id,
        ),
    )
    .with_policy(state.config.production)
}

/// `POST /api/operator/pause` — flip the inference pause switch.
pub(crate) fn handle_pause(
    body: &[u8],
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let requester = match authed_operator(state, head, request_id, peer_ip) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let payload = match need_json(state, body, request_id) {
        Ok(p) => p,
        Err(r) => return r,
    };
    let paused = match payload.get("paused") {
        Some(Value::Bool(b)) => *b,
        _ => return bad(state, request_id, "paused must be true or false"),
    };
    state.set_paused(paused);
    state.audit(
        Some(&requester.user.id),
        if paused {
            "inference_paused"
        } else {
            "inference_resumed"
        },
        "",
        request_id,
    );
    Response::json(200, with_request_id(json!({"paused": paused}), request_id))
        .with_policy(state.config.production)
}

/// `GET /api/operator/audit` — recent redacted audit entries.
pub(crate) fn handle_audit(
    path: &str,
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let _ = match authed_operator(state, head, request_id, peer_ip) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let limit = super::history::split_query(path)
        .1
        .iter()
        .find(|(k, _)| k == "limit")
        .and_then(|(_, v)| v.parse::<i64>().ok())
        .unwrap_or(100)
        .clamp(1, 500);
    match state.db.recent_audit(limit) {
        Ok(entries) => {
            let items: Vec<Value> = entries
                .iter()
                .map(|e| {
                    json!({
                        "ts": e.ts,
                        "actor": e.actor_user_id,
                        "action": e.action,
                        "detail": e.detail,
                        "request_id": e.request_id,
                    })
                })
                .collect();
            Response::json(200, with_request_id(json!({"audit": items}), request_id))
                .with_policy(state.config.production)
        }
        Err(_) => Response::json(
            502,
            error_json(
                super::response::CODE_ESTIMATION_FAILED,
                "Could not load audit log",
                request_id,
            ),
        )
        .with_policy(state.config.production),
    }
}

fn bad(state: &ServerState, request_id: &str, message: &str) -> Response {
    Response::json(
        400,
        error_json(super::response::CODE_INVALID_OPTIONS, message, request_id),
    )
    .with_policy(state.config.production)
}

fn not_found(state: &ServerState, request_id: &str) -> Response {
    Response::json(
        404,
        error_json(super::response::CODE_NOT_FOUND, "Not found", request_id),
    )
    .with_policy(state.config.production)
}
