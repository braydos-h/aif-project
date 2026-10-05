//! Request authentication: sessions, CSRF, client IP, and gating.
//!
//! Every sensitive operation resolves the caller through
//! [`authenticate`] (session cookie → live DB session → active user) and
//! enforces CSRF on state-changing API calls. Failures are generic
//! (`401 unauthorized`, `403 csrf_invalid`) and never leak whether an
//! account, invite, or record exists.

use serde_json::json;

use super::response::{
    error_json, with_request_id, Response, CODE_CSRF, CODE_FORBIDDEN, CODE_RATE_LIMITED,
    CODE_UNAUTHORIZED,
};
use super::server::RequestHead;
use super::ServerState;
use crate::auth::session_token_from_cookie;
use crate::db::User;
use crate::time_util::{rfc3339, unix_now};

/// An authenticated caller: the user row plus the session's CSRF token.
pub(crate) struct Requester {
    pub(crate) user: User,
    pub(crate) csrf_token: String,
}

/// Resolve the caller from the session cookie. `None` means anonymous.
pub(crate) fn authenticate(state: &ServerState, head: &RequestHead) -> Option<Requester> {
    let cookie = head.cookie.as_deref()?;
    let token = session_token_from_cookie(cookie)?;
    let now = rfc3339(unix_now());
    let session = state
        .db
        .check_session(&crate::auth::token_hash(&token), &now)
        .ok()??;
    let user = state.db.user_by_id(&session.user_id).ok()??;
    if user.status != "active" {
        return None;
    }
    Some(Requester {
        user,
        csrf_token: session.csrf_token,
    })
}

/// Effective client IP: the first `X-Forwarded-For` hop only when the TCP
/// peer is an explicitly configured trusted proxy, else the peer itself.
/// Forwarded headers from untrusted peers are never trusted (spending and
/// login limits key off this value).
pub(crate) fn client_ip(state: &ServerState, peer_ip: &str, head: &RequestHead) -> String {
    let trusted = state.config.trusted_proxies.iter().any(|p| p == peer_ip);
    if trusted {
        if let Some(forwarded) = head.forwarded_for.as_deref() {
            if let Some(first) = forwarded.split(',').next().map(str::trim) {
                if !first.is_empty() && first.len() <= 64 {
                    return first.to_string();
                }
            }
        }
    }
    peer_ip.to_string()
}

/// Require an authenticated caller. In open local mode anonymous callers
/// get the same 401; the WebUI probes `/api/me` to decide whether to show
/// the login screen.
pub(crate) fn require_auth(
    state: &ServerState,
    head: &RequestHead,
    request_id: &str,
    peer_ip: &str,
) -> Result<Requester, Response> {
    let _ = peer_ip;
    match authenticate(state, head) {
        Some(requester) => Ok(requester),
        None => Err(Response::json(
            401,
            error_json(
                CODE_UNAUTHORIZED,
                "Authentication is required for this operation",
                request_id,
            ),
        )
        .with_policy(state.config.production)),
    }
}

/// Require the caller to be the operator.
pub(crate) fn require_operator(
    state: &ServerState,
    requester: &Requester,
    request_id: &str,
) -> Result<(), Response> {
    if requester.user.role == "operator" {
        return Ok(());
    }
    Err(Response::json(
        403,
        error_json(
            CODE_FORBIDDEN,
            "Operator access is required for this operation",
            request_id,
        ),
    )
    .with_policy(state.config.production))
}

/// Enforce CSRF on state-changing API calls made with a cookie session: the
/// `X-CSRF-Token` header must match the session's token. Safe methods
/// (GET/OPTIONS) are exempt.
pub(crate) fn check_csrf(
    state: &ServerState,
    head: &RequestHead,
    requester: &Requester,
    request_id: &str,
) -> Result<(), Response> {
    if head.method == "GET" || head.method == "OPTIONS" || head.method == "HEAD" {
        return Ok(());
    }
    let presented = head.csrf_token.as_deref().unwrap_or("");
    if presented.is_empty() || presented != requester.csrf_token {
        return Err(Response::json(
            403,
            error_json(CODE_CSRF, "Missing or invalid CSRF token", request_id),
        )
        .with_policy(state.config.production));
    }
    Ok(())
}

/// Generic 429 response.
pub(crate) fn rate_limited(state: &ServerState, request_id: &str, message: &str) -> Response {
    Response::json(
        429,
        with_request_id(
            json!({"error": message, "code": CODE_RATE_LIMITED, "request_id": request_id}),
            request_id,
        ),
    )
    .with_policy(state.config.production)
}
