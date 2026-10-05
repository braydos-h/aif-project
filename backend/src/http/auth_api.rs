//! Invite-only authentication API: login, logout, invite acceptance,
//! recovery, password change, and profile updates.
//!
//! There is no public signup route. Accounts are created only through
//! operator-issued single-use invites, and the service caps active users at
//! two. Failures are generic (`invalid_credentials`, `invite_invalid`) so
//! callers cannot enumerate accounts, and invite/recovery tokens never
//! appear in logs or audit entries.

use serde_json::{json, Value};

use super::response::{error_json, with_request_id, Response};
use super::server::RequestHead;
use super::session::{authenticate, client_ip, rate_limited, require_auth};
use super::validation::optional_string;
use super::ServerState;
use crate::auth::{
    check_password_policy, clear_cookie_value, hash_password, new_id, normalize_email, random_hex,
    session_cookie_value, token_hash, valid_display_name, valid_email, CSRF_BYTES, TOKEN_BYTES,
};
use crate::time_util::{rfc3339, unix_now};

/// Parse a JSON body or answer `400 missing_body` / `400 invalid_json`.
fn need_json(body: &[u8], request_id: &str) -> Result<Value, Response> {
    if body.is_empty() {
        return Err(Response::json(
            400,
            error_json(
                super::response::CODE_MISSING_BODY,
                "Missing request body",
                request_id,
            ),
        ));
    }
    serde_json::from_slice(body).map_err(|_| {
        Response::json(
            400,
            error_json(
                super::response::CODE_INVALID_JSON,
                "Invalid JSON payload",
                request_id,
            ),
        )
    })
}

fn opt<'a>(payload: &'a Value, field: &str) -> Result<Option<&'a str>, Response> {
    optional_string(payload, field).map_err(|message| {
        Response::json(
            400,
            error_json(super::response::CODE_INVALID_OPTIONS, &message, ""),
        )
    })
}

/// Session lifetime in seconds from server config.
fn session_max_age(state: &ServerState) -> u64 {
    state.config.session_days * 86_400
}

/// Create a session row and return the raw token + CSRF token.
fn start_session(state: &ServerState, user_id: &str) -> Result<(String, String), String> {
    let token = random_hex(TOKEN_BYTES);
    let csrf = random_hex(CSRF_BYTES);
    let expires_at = rfc3339(unix_now() + session_max_age(state));
    state
        .db
        .create_session(&token_hash(&token), user_id, &csrf, &expires_at)?;
    Ok((token, csrf))
}

fn user_json(state: &ServerState, user: &crate::db::User, csrf: &str) -> Value {
    let _ = state;
    json!({
        "id": user.id,
        "email": user.email,
        "display_name": user.display_name,
        "role": user.role,
        "status": user.status,
        "created_at": user.created_at,
        "last_login_at": user.last_login_at,
        "csrf_token": csrf,
    })
}

/// `GET /api/me` — session probe for the WebUI (also delivers the CSRF
/// token to logged-in browsers).
pub(crate) fn handle_me(
    _body: &[u8],
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
) -> Response {
    let payload = match authenticate(state, head) {
        Some(requester) => with_request_id(
            json!({
                "authenticated": true,
                "auth_required": state.config.require_auth,
                "production": state.config.production,
                "user": user_json(state, &requester.user, &requester.csrf_token),
            }),
            request_id,
        ),
        None => with_request_id(
            json!({
                "authenticated": false,
                "auth_required": state.config.require_auth,
                "production": state.config.production,
            }),
            request_id,
        ),
    };
    Response::json(200, payload).with_policy(state.config.production)
}

/// `POST /api/auth/login` — password login with per-IP/account rate
/// limits. Failures share one generic message (no enumeration oracle).
pub(crate) fn handle_login(
    body: &[u8],
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let ip = client_ip(state, peer_ip, head);
    if !state
        .login_limiter
        .check_and_record(&format!("login-ip:{}", ip))
    {
        return rate_limited(
            state,
            request_id,
            "Too many login attempts; try again later",
        );
    }
    let payload = match need_json(body, request_id) {
        Ok(p) => p,
        Err(r) => return r.with_policy(state.config.production),
    };
    let email = match opt(&payload, "email") {
        Ok(v) => normalize_email(v.unwrap_or("")),
        Err(mut r) => {
            r.cors_origin = None;
            return r.with_policy(state.config.production);
        }
    };
    let password = match opt(&payload, "password") {
        Ok(v) => v.unwrap_or("").to_string(),
        Err(r) => return r.with_policy(state.config.production),
    };
    if !state
        .login_limiter
        .check_and_record(&format!("login-email:{}", email))
    {
        return rate_limited(
            state,
            request_id,
            "Too many login attempts; try again later",
        );
    }
    let fail = || {
        Response::json(
            401,
            error_json(
                super::response::CODE_INVALID_CREDENTIALS,
                "Invalid email or password",
                request_id,
            ),
        )
        .with_policy(state.config.production)
    };
    let (user, hash) = match state.db.user_by_email_with_hash(&email) {
        Ok(Some(pair)) => pair,
        _ => return fail(),
    };
    if user.status != "active" || !crate::auth::verify_password(&password, &hash) {
        return fail();
    }
    let (token, csrf) = match start_session(state, &user.id) {
        Ok(pair) => pair,
        Err(_) => {
            return Response::json(
                502,
                error_json(
                    super::response::CODE_ESTIMATION_FAILED,
                    "Login failed; try again",
                    request_id,
                ),
            )
            .with_policy(state.config.production)
        }
    };
    state.login_limiter.reset(&format!("login-ip:{}", ip));
    let _ = state.db.record_login(&user.id);
    state.audit(
        Some(&user.id),
        "login",
        &format!("email={}", user.email),
        request_id,
    );
    let cookie = session_cookie_value(&token, session_max_age(state), state.config.cookie_secure);
    Response::json(
        200,
        with_request_id(json!({"user": user_json(state, &user, &csrf)}), request_id),
    )
    .with_header("Set-Cookie", &cookie)
    .with_policy(state.config.production)
}

/// `POST /api/auth/logout` — revoke the current session. Always succeeds.
pub(crate) fn handle_logout(
    _body: &[u8],
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
) -> Response {
    if let Some(requester) = authenticate(state, head) {
        if let Some(cookie) = head.cookie.as_deref() {
            if let Some(token) = crate::auth::session_token_from_cookie(cookie) {
                let _ = state.db.revoke_session(&token_hash(&token));
            }
        }
        state.audit(
            Some(&requester.user.id),
            "logout",
            &format!("email={}", requester.user.email),
            request_id,
        );
    }
    Response::json(200, with_request_id(json!({"ok": true}), request_id))
        .with_header(
            "Set-Cookie",
            &clear_cookie_value(state.config.cookie_secure),
        )
        .with_policy(state.config.production)
}

/// `POST /api/auth/accept-invite` — redeem a single-use invite into the
/// account it was issued for (email-bound) and log in immediately.
pub(crate) fn handle_accept_invite(
    body: &[u8],
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let ip = client_ip(state, peer_ip, head);
    if !state
        .invite_limiter
        .check_and_record(&format!("invite:{}", ip))
    {
        return rate_limited(
            state,
            request_id,
            "Too many invite attempts; try again later",
        );
    }
    let payload = match need_json(body, request_id) {
        Ok(p) => p,
        Err(r) => return r.with_policy(state.config.production),
    };
    let get = |field: &str| match opt(&payload, field) {
        Ok(v) => Ok(v.unwrap_or("").to_string()),
        Err(r) => Err(r),
    };
    let token = match get("token") {
        Ok(v) => v,
        Err(r) => return r.with_policy(state.config.production),
    };
    let password = match get("password") {
        Ok(v) => v,
        Err(r) => return r.with_policy(state.config.production),
    };
    let display_name = match get("display_name") {
        Ok(v) => v,
        Err(r) => return r.with_policy(state.config.production),
    };
    if token.is_empty() || token.len() > 256 {
        return invite_error(
            state,
            request_id,
            super::response::CODE_INVITE_INVALID,
            "That invitation link is not valid",
        );
    }
    let invite = match state.db.invite_by_token_hash(&token_hash(&token)) {
        Ok(Some(inv)) => inv,
        _ => {
            return invite_error(
                state,
                request_id,
                super::response::CODE_INVITE_INVALID,
                "That invitation link is not valid",
            );
        }
    };
    if invite.revoked_at.is_some() {
        return invite_error(
            state,
            request_id,
            super::response::CODE_INVITE_REVOKED,
            "That invitation was revoked; ask the operator for a new one",
        );
    }
    if invite.used_at.is_some() {
        return invite_error(
            state,
            request_id,
            super::response::CODE_INVITE_USED,
            "That invitation was already used",
        );
    }
    if invite.expires_at.as_str() < rfc3339(unix_now()).as_str() {
        return invite_error(
            state,
            request_id,
            super::response::CODE_INVITE_EXPIRED,
            "That invitation expired; ask the operator for a new one",
        );
    }
    if let Err(message) = check_password_policy(&password) {
        return Response::json(
            400,
            error_json(super::response::CODE_INVALID_OPTIONS, &message, request_id),
        )
        .with_policy(state.config.production);
    }
    let name = {
        let trimmed = display_name.trim();
        if trimmed.is_empty() {
            invite.email.clone()
        } else if !valid_display_name(trimmed) {
            return Response::json(
                400,
                error_json(
                    super::response::CODE_INVALID_OPTIONS,
                    "Display name must be 1-64 characters without control characters",
                    request_id,
                ),
            )
            .with_policy(state.config.production);
        } else {
            trimmed.to_string()
        }
    };
    let hash = match hash_password(&password) {
        Ok(h) => h,
        Err(_) => {
            return Response::json(
                502,
                error_json(
                    super::response::CODE_ESTIMATION_FAILED,
                    "Could not create the account; try again",
                    request_id,
                ),
            )
            .with_policy(state.config.production)
        }
    };
    let user = match state.db.create_user(
        &new_id(),
        &normalize_email(&invite.email),
        &name,
        &hash,
        &invite.role,
    ) {
        Ok(user) => user,
        Err(message) => {
            if message.starts_with("user_limit") {
                return Response::json(
                    403,
                    error_json(
                        super::response::CODE_USER_LIMIT,
                        "The service already has two active accounts",
                        request_id,
                    ),
                )
                .with_policy(state.config.production);
            }
            return invite_error(
                state,
                request_id,
                super::response::CODE_INVITE_INVALID,
                "That invitation link is not valid",
            );
        }
    };
    let _ = state.db.mark_invite_used(&invite.id, &user.id);
    let (token, csrf) = match start_session(state, &user.id) {
        Ok(pair) => pair,
        Err(_) => {
            return Response::json(
                502,
                error_json(
                    super::response::CODE_ESTIMATION_FAILED,
                    "Account created but login failed; use the login screen",
                    request_id,
                ),
            )
            .with_policy(state.config.production)
        }
    };
    state.audit(
        Some(&user.id),
        "invite_accepted",
        &format!("invite={} email={}", invite.id, user.email),
        request_id,
    );
    let cookie = session_cookie_value(&token, session_max_age(state), state.config.cookie_secure);
    Response::json(
        200,
        with_request_id(json!({"user": user_json(state, &user, &csrf)}), request_id),
    )
    .with_header("Set-Cookie", &cookie)
    .with_policy(state.config.production)
}

fn invite_error(state: &ServerState, request_id: &str, code: &str, message: &str) -> Response {
    Response::json(400, error_json(code, message, request_id)).with_policy(state.config.production)
}

/// `POST /api/auth/recovery/request` — always answers success (generic, no
/// enumeration). When the email belongs to an active account a recovery
/// token is minted for operator relay (see docs/recovery.md); the token
/// itself is never returned or logged here.
pub(crate) fn handle_recovery_request(
    body: &[u8],
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let ok = || {
        Response::json(
            200,
            with_request_id(
                json!({"ok": true, "message": "If that email has an account, the operator has been notified with recovery steps"}),
                request_id,
            ),
        )
        .with_policy(state.config.production)
    };
    let payload = match need_json(body, request_id) {
        Ok(p) => p,
        Err(_) => return ok(),
    };
    let email = match opt(&payload, "email") {
        Ok(v) => normalize_email(v.unwrap_or("")),
        Err(_) => return ok(),
    };
    if !valid_email(&email) {
        return ok();
    }
    let ip = client_ip(state, peer_ip, head);
    if !state
        .recovery_limiter
        .check_and_record(&format!("rec-ip:{}", ip))
        || !state
            .recovery_limiter
            .check_and_record(&format!("rec-email:{}", email))
    {
        return ok();
    }
    let user = match state.db.user_by_email_with_hash(&email) {
        Ok(Some((user, _))) if user.status == "active" => user,
        _ => return ok(),
    };
    // Bound resend attempts: at most 3 live requests per 24 h per account.
    let since = rfc3339(unix_now().saturating_sub(86_400));
    let recent = state
        .db
        .recent_recovery_count(&user.id, &since)
        .unwrap_or(99);
    if recent < 3 {
        let token = random_hex(TOKEN_BYTES);
        let expires = rfc3339(unix_now() + 24 * 3600);
        let _ = state
            .db
            .create_recovery(&new_id(), &user.id, &token_hash(&token), &expires);
        state.audit(
            Some(&user.id),
            "recovery_requested",
            &format!("email={}", user.email),
            request_id,
        );
    }
    ok()
}

/// `POST /api/auth/recovery/complete` — redeem a recovery token for a new
/// password. Single-use: reuse fails safely, and all sessions are revoked.
pub(crate) fn handle_recovery_complete(
    body: &[u8],
    request_id: &str,
    state: &ServerState,
) -> Response {
    let payload = match need_json(body, request_id) {
        Ok(p) => p,
        Err(r) => return r.with_policy(state.config.production),
    };
    let get = |field: &str| match opt(&payload, field) {
        Ok(v) => Ok(v.unwrap_or("").to_string()),
        Err(r) => Err(r),
    };
    let token = match get("token") {
        Ok(v) => v,
        Err(r) => return r.with_policy(state.config.production),
    };
    let password = match get("new_password") {
        Ok(v) => v,
        Err(r) => return r.with_policy(state.config.production),
    };
    let fail = || {
        Response::json(
            400,
            error_json(
                "recovery_invalid",
                "That recovery link is invalid, expired, or already used",
                request_id,
            ),
        )
        .with_policy(state.config.production)
    };
    if token.is_empty() || token.len() > 256 {
        return fail();
    }
    if check_password_policy(&password).is_err() {
        return Response::json(
            400,
            error_json(
                super::response::CODE_INVALID_OPTIONS,
                "New password must be 10-256 characters",
                request_id,
            ),
        )
        .with_policy(state.config.production);
    }
    let live = match state
        .db
        .live_recovery(&token_hash(&token), &rfc3339(unix_now()))
    {
        Ok(Some(t)) => t,
        _ => return fail(),
    };
    let hash = match hash_password(&password) {
        Ok(h) => h,
        Err(_) => return fail(),
    };
    if state
        .db
        .consume_recovery(&live.id, &live.user_id, &hash)
        .is_err()
    {
        return fail();
    }
    state.audit(Some(&live.user_id), "recovery_completed", "", request_id);
    Response::json(
        200,
        with_request_id(
            json!({"ok": true, "message": "Password updated; log in again"}),
            request_id,
        ),
    )
    .with_header(
        "Set-Cookie",
        &clear_cookie_value(state.config.cookie_secure),
    )
    .with_policy(state.config.production)
}

/// `POST /api/auth/change-password` — reauthenticated password change:
/// requires the current password, then revokes all sessions (fresh login).
pub(crate) fn handle_change_password(
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
    if let Err(r) = super::session::check_csrf(state, head, &requester, request_id) {
        return r;
    }
    let payload = match need_json(body, request_id) {
        Ok(p) => p,
        Err(r) => return r.with_policy(state.config.production),
    };
    let get = |field: &str| match opt(&payload, field) {
        Ok(v) => Ok(v.unwrap_or("").to_string()),
        Err(r) => Err(r),
    };
    let current = match get("current_password") {
        Ok(v) => v,
        Err(r) => return r.with_policy(state.config.production),
    };
    let next = match get("new_password") {
        Ok(v) => v,
        Err(r) => return r.with_policy(state.config.production),
    };
    if check_password_policy(&next).is_err() {
        return Response::json(
            400,
            error_json(
                super::response::CODE_INVALID_OPTIONS,
                "New password must be 10-256 characters",
                request_id,
            ),
        )
        .with_policy(state.config.production);
    }
    let stored = match state.db.user_by_email_with_hash(&requester.user.email) {
        Ok(Some((_, hash))) => hash,
        _ => {
            return Response::json(
                401,
                error_json(
                    super::response::CODE_INVALID_CREDENTIALS,
                    "Current password is incorrect",
                    request_id,
                ),
            )
            .with_policy(state.config.production)
        }
    };
    if !crate::auth::verify_password(&current, &stored) {
        return Response::json(
            401,
            error_json(
                super::response::CODE_INVALID_CREDENTIALS,
                "Current password is incorrect",
                request_id,
            ),
        )
        .with_policy(state.config.production);
    }
    let hash = match hash_password(&next) {
        Ok(h) => h,
        Err(_) => {
            return Response::json(
                502,
                error_json(
                    super::response::CODE_ESTIMATION_FAILED,
                    "Could not update the password",
                    request_id,
                ),
            )
            .with_policy(state.config.production)
        }
    };
    let _ = state.db.set_password(&requester.user.id, &hash);
    let _ = state.db.revoke_all_sessions(&requester.user.id);
    state.audit(Some(&requester.user.id), "password_changed", "", request_id);
    Response::json(
        200,
        with_request_id(
            json!({"ok": true, "message": "Password updated; log in again"}),
            request_id,
        ),
    )
    .with_header(
        "Set-Cookie",
        &clear_cookie_value(state.config.cookie_secure),
    )
    .with_policy(state.config.production)
}

/// `POST /api/auth/profile` — update the display name.
pub(crate) fn handle_update_profile(
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
    if let Err(r) = super::session::check_csrf(state, head, &requester, request_id) {
        return r;
    }
    let payload = match need_json(body, request_id) {
        Ok(p) => p,
        Err(r) => return r.with_policy(state.config.production),
    };
    let name = match opt(&payload, "display_name") {
        Ok(v) => v.unwrap_or("").trim().to_string(),
        Err(r) => return r.with_policy(state.config.production),
    };
    if !valid_display_name(&name) {
        return Response::json(
            400,
            error_json(
                super::response::CODE_INVALID_OPTIONS,
                "Display name must be 1-64 characters without control characters",
                request_id,
            ),
        )
        .with_policy(state.config.production);
    }
    let _ = state.db.set_display_name(&requester.user.id, &name);
    state.audit(Some(&requester.user.id), "profile_updated", "", request_id);
    Response::json(
        200,
        with_request_id(json!({"ok": true, "display_name": name}), request_id),
    )
    .with_policy(state.config.production)
}
