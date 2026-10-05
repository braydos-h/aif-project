//! Hand-rolled HTTP/1.1 server on `std::net`, one thread per connection.
//!
//! The Rust binary serves the WebUI and the JSON API from the same origin.
//! Static routes are explicit compile-time assets; no request can read an
//! arbitrary filesystem path. Every response carries CORS headers, and JSON
//! responses carry a request id in both the body and `x-request-id` header.
//!
//! Concurrency is bounded: at most [`server::MAX_CONCURRENT_CONNECTIONS`]
//! connections are handled at once; excess connections get a `503
//! server_busy` JSON response instead of spawning unbounded threads.
//!
//! Account routes (`/api/*`) enforce cookie sessions, CSRF tokens, and
//! per-user ownership server-side. When `AIF_REQUIRE_AUTH=1` (always in
//! production), estimation also requires a session.
//!
//! Split into focused submodules:
//! - [`server`] — listener loop and connection handling.
//! - [`response`] — response construction and header writing.
//! - [`request_id`] — per-request unique ids.
//! - [`assets`] — embedded WebUI files and the demo-image registry.
//! - [`validation`] — runtime option validation and limits.
//! - [`handlers`] — read-only info/demo handlers.
//! - [`estimate`] — `POST /estimate-weight` + `POST /estimate-batch`.
//! - [`session`] — cookie sessions, CSRF, client IP, gating.
//! - [`auth_api`] — login, logout, invites, recovery, password, profile.
//! - [`history`] — durable per-user estimate history + CSV export.
//! - [`animals`] — animal records, trends, scale measurements.
//! - [`account`] — account summary, export, deletion.
//! - [`operator`] — operator invites, users, usage, pause, audit.
//! - [`csv`] — spreadsheet-safe CSV field encoding.
//! - [`json_util`] — shared JSON body helpers.

pub mod account;
pub mod animals;
pub mod assets;
pub mod auth_api;
pub mod csv;
pub mod estimate;
pub mod handlers;
pub mod history;
pub mod json_util;
pub mod operator;
pub mod request_id;
pub mod response;
pub mod server;
pub mod session;
pub mod validation;

pub use server::serve;

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use crate::cache::Cache;
use crate::config::Config;
use crate::db::Db;
use crate::limits::{AttemptLimiter, InferenceGate};

use assets::{ACCOUNT_CSS, ACCOUNT_JS, APP_JS, INDEX_HTML, STYLES_CSS};
use estimate::{handle_estimate, handle_estimate_batch};
use handlers::{handle_demo_image, handle_demo_list, handle_info, handle_metrics};
use response::{error_json, Response, CODE_NOT_FOUND};
use server::RequestHead;

/// Runtime counters shared across connection threads.
pub struct ServerMetrics {
    start: Instant,
    pub(crate) total_requests: AtomicU64,
    pub(crate) active_connections: AtomicUsize,
    pub(crate) rejected_connections: AtomicU64,
}

impl ServerMetrics {
    /// Fresh counters starting now.
    pub fn new() -> Self {
        ServerMetrics {
            start: Instant::now(),
            total_requests: AtomicU64::new(0),
            active_connections: AtomicUsize::new(0),
            rejected_connections: AtomicU64::new(0),
        }
    }

    /// Seconds since the server started (saturating, monotonic).
    pub fn uptime_secs(&self) -> u64 {
        self.start.elapsed().as_secs()
    }

    /// Record one dispatched request.
    pub(crate) fn record_request(&self) {
        self.total_requests.fetch_add(1, Ordering::Relaxed);
    }

    /// Record one rejected (over-limit) connection.
    pub(crate) fn record_rejected(&self) {
        self.rejected_connections.fetch_add(1, Ordering::Relaxed);
    }

    /// Current in-flight connection count (for `/metrics`).
    pub fn active(&self) -> usize {
        self.active_connections.load(Ordering::Relaxed)
    }

    /// Total dispatched requests (for `/metrics`).
    pub fn total(&self) -> u64 {
        self.total_requests.load(Ordering::Relaxed)
    }

    /// Total rejected connections (for `/metrics`).
    pub fn rejected(&self) -> u64 {
        self.rejected_connections.load(Ordering::Relaxed)
    }
}

impl Default for ServerMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Shared server state: config + cache + metrics + database + limiters,
/// safe to hand to threads.
pub struct ServerState {
    pub config: Config,
    pub cache: Cache,
    pub metrics: ServerMetrics,
    pub db: Db,
    pub(crate) login_limiter: AttemptLimiter,
    pub(crate) invite_limiter: AttemptLimiter,
    pub(crate) recovery_limiter: AttemptLimiter,
    pub(crate) inference_gate: InferenceGate,
    paused: AtomicBool,
}

impl ServerState {
    /// Build shared state from the effective config. Opens (creating) and
    /// migrates the SQLite database.
    pub fn new(config: Config) -> Result<Self, String> {
        let db = Db::open(&config.data_dir)?;
        let gate_max = config.max_concurrent_inference;
        let paused = config.inference_paused;
        let cache = Cache::new(config.cache_ttl);
        Ok(ServerState {
            config,
            cache,
            metrics: ServerMetrics::new(),
            db,
            // Login: 10 attempts per 10 min per IP/account.
            login_limiter: AttemptLimiter::new(10, 600),
            // Invite acceptance: 10 attempts per 10 min per IP.
            invite_limiter: AttemptLimiter::new(10, 600),
            // Recovery: 5 requests per hour per IP/account.
            recovery_limiter: AttemptLimiter::new(5, 3600),
            inference_gate: InferenceGate::new(gate_max),
            paused: AtomicBool::new(paused),
        })
    }

    /// True when the operator has paused provider-backed inference (or the
    /// server started paused via `AIF_INFERENCE_PAUSED=1`).
    pub fn inference_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    /// Flip the runtime pause switch (operator only, via API).
    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    /// Best-effort audit logging: failures never fail the request.
    /// Callers must never include secrets, tokens, prompts, or images.
    pub fn audit(&self, actor_user_id: Option<&str>, action: &str, detail: &str, request_id: &str) {
        let _ = self.db.audit(actor_user_id, action, detail, request_id);
    }
}

/// Dispatch a request to the WebUI, demo, API, or account handler.
fn dispatch(
    method: &str,
    path: &str,
    body: &[u8],
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    state.metrics.record_request();
    let production = state.config.production;
    let policy = |response: Response| response.with_policy(production);
    match (method, path) {
        ("GET", "/") => policy(Response::bytes(200, "text/html; charset=utf-8", INDEX_HTML)),
        ("GET", "/styles.css") => {
            policy(Response::bytes(200, "text/css; charset=utf-8", STYLES_CSS))
        }
        ("GET", "/app.js") => policy(Response::bytes(
            200,
            "application/javascript; charset=utf-8",
            APP_JS,
        )),
        ("GET", "/account.js") => policy(Response::bytes(
            200,
            "application/javascript; charset=utf-8",
            ACCOUNT_JS,
        )),
        ("GET", "/account.css") => {
            policy(Response::bytes(200, "text/css; charset=utf-8", ACCOUNT_CSS))
        }
        ("GET", "/health") => policy(handlers::handle_health(request_id, state)),
        ("GET", "/info") => policy(handle_info(request_id, state)),
        ("GET", "/metrics") => policy(handle_metrics(request_id, state, head, peer_ip)),
        ("GET", "/demo-cows") => policy(handle_demo_list(request_id)),
        ("GET", path) if path.starts_with("/demo-cows/") => {
            policy(handle_demo_image(path, request_id))
        }
        ("OPTIONS", _) => policy(Response {
            status: 204,
            content_type: "text/plain; charset=utf-8",
            body: Vec::new(),
            extra_headers: Vec::new(),
            cors_origin: Some("*".to_string()),
            hsts: false,
            attachment: None,
        }),
        ("GET", "/api/me") => auth_api::handle_me(body, request_id, state, head),
        ("POST", "/api/auth/login") => {
            auth_api::handle_login(body, request_id, state, head, peer_ip)
        }
        ("POST", "/api/auth/logout") => auth_api::handle_logout(body, request_id, state, head),
        ("POST", "/api/auth/accept-invite") => {
            auth_api::handle_accept_invite(body, request_id, state, head, peer_ip)
        }
        ("POST", "/api/auth/recovery/request") => {
            auth_api::handle_recovery_request(body, request_id, state, head, peer_ip)
        }
        ("POST", "/api/auth/recovery/complete") => {
            auth_api::handle_recovery_complete(body, request_id, state)
        }
        ("POST", "/api/auth/change-password") => {
            auth_api::handle_change_password(body, request_id, state, head, peer_ip)
        }
        ("POST", "/api/auth/profile") => {
            auth_api::handle_update_profile(body, request_id, state, head, peer_ip)
        }
        ("GET", path) if path.starts_with("/api/history/export") => {
            history::handle_export(&with_query(path, head), request_id, state, head, peer_ip)
        }
        ("GET", path) if path.starts_with("/api/history") => {
            route_history_get(path, head, body, request_id, state, peer_ip)
        }
        ("DELETE", path) if path.starts_with("/api/history/") => {
            let id = path.strip_prefix("/api/history/").unwrap_or("");
            history::handle_delete(id, body, request_id, state, head, peer_ip)
        }
        ("GET", path) if path.starts_with("/api/animals") => {
            route_animals_get(path, head, body, request_id, state, peer_ip)
        }
        ("POST", "/api/animals") => animals::handle_create(body, request_id, state, head, peer_ip),
        ("PUT", path) if path.starts_with("/api/animals/") => {
            route_animals_put(path, body, request_id, state, head, peer_ip)
        }
        ("DELETE", path) if path.starts_with("/api/animals/") => {
            let id = path.strip_prefix("/api/animals/").unwrap_or("");
            animals::handle_delete(id, body, request_id, state, head, peer_ip)
        }
        ("POST", path) if path.starts_with("/api/animals/") => {
            route_animals_post(path, body, request_id, state, head, peer_ip)
        }
        ("GET", "/api/account") => account::handle_summary(request_id, state, head, peer_ip),
        ("GET", "/api/account/export") => account::handle_export(request_id, state, head, peer_ip),
        ("DELETE", "/api/account") => {
            account::handle_delete(body, request_id, state, head, peer_ip)
        }
        ("POST", "/api/operator/invites") => {
            operator::handle_create_invite(body, request_id, state, head, peer_ip)
        }
        ("GET", "/api/operator/invites") => {
            operator::handle_list_invites(request_id, state, head, peer_ip)
        }
        ("POST", path)
            if path.starts_with("/api/operator/invites/") && path.ends_with("/revoke") =>
        {
            let inner = path
                .strip_prefix("/api/operator/invites/")
                .unwrap_or("")
                .strip_suffix("/revoke")
                .unwrap_or("");
            operator::handle_revoke_invite(inner, body, request_id, state, head, peer_ip)
        }
        ("GET", "/api/operator/users") => {
            operator::handle_list_users(request_id, state, head, peer_ip)
        }
        ("DELETE", path) if path.starts_with("/api/operator/users/") => {
            let id = path.strip_prefix("/api/operator/users/").unwrap_or("");
            operator::handle_delete_user(id, body, request_id, state, head, peer_ip)
        }
        ("GET", "/api/operator/usage") => operator::handle_usage(request_id, state, head, peer_ip),
        ("GET", "/api/operator/status") => {
            operator::handle_status(request_id, state, head, peer_ip)
        }
        ("POST", "/api/operator/pause") => {
            operator::handle_pause(body, request_id, state, head, peer_ip)
        }
        ("GET", path) if path.starts_with("/api/operator/audit") => {
            operator::handle_audit(&with_query(path, head), request_id, state, head, peer_ip)
        }
        ("POST", "/estimate-weight") => handle_estimate(body, request_id, state, head, peer_ip),
        ("POST", "/estimate-batch") => {
            handle_estimate_batch(body, request_id, state, head, peer_ip)
        }
        (_, _) => policy(Response::json(
            404,
            error_json(CODE_NOT_FOUND, "Not found", request_id),
        )),
    }
}

/// Reattach the query string the socket layer stripped, for handlers that
/// accept filters. The query never influences routing.
fn with_query(path: &str, head: &RequestHead) -> String {
    match head.query.as_deref() {
        Some(q) if !q.is_empty() => format!("{}?{}", path, q),
        _ => path.to_string(),
    }
}

/// Route `GET /api/history` (list) vs `GET /api/history/{id}` (detail).
fn route_history_get(
    path: &str,
    head: &RequestHead,
    body: &[u8],
    request_id: &str,
    state: &ServerState,
    peer_ip: &str,
) -> Response {
    let _ = body;
    if path == "/api/history" || path == "/api/history/" {
        return history::handle_list(&with_query(path, head), request_id, state, head, peer_ip);
    }
    if let Some(id) = path.strip_prefix("/api/history/") {
        if !id.is_empty() && !id.contains('/') {
            return history::handle_get(id, request_id, state, head, peer_ip);
        }
    }
    Response::json(404, error_json(CODE_NOT_FOUND, "Not found", request_id))
        .with_policy(state.config.production)
}

/// Route `GET /api/animals` (list) vs `GET /api/animals/{id}` (detail).
fn route_animals_get(
    path: &str,
    head: &RequestHead,
    body: &[u8],
    request_id: &str,
    state: &ServerState,
    peer_ip: &str,
) -> Response {
    let _ = body;
    if path == "/api/animals" || path == "/api/animals/" {
        return animals::handle_list(&with_query(path, head), request_id, state, head, peer_ip);
    }
    if let Some(id) = path.strip_prefix("/api/animals/") {
        if !id.is_empty() && !id.contains('/') {
            return animals::handle_get(
                id,
                &with_query(path, head),
                request_id,
                state,
                head,
                peer_ip,
            );
        }
    }
    Response::json(404, error_json(CODE_NOT_FOUND, "Not found", request_id))
        .with_policy(state.config.production)
}

/// Route `PUT /api/animals/{id}` (edit).
fn route_animals_put(
    path: &str,
    body: &[u8],
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    if let Some(id) = path.strip_prefix("/api/animals/") {
        if !id.is_empty() && !id.contains('/') {
            return animals::handle_update(id, body, request_id, state, head, peer_ip);
        }
    }
    Response::json(404, error_json(CODE_NOT_FOUND, "Not found", request_id))
        .with_policy(state.config.production)
}

/// Route `POST /api/animals/{id}/measurements` (scale entry).
fn route_animals_post(
    path: &str,
    body: &[u8],
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    if let Some(rest) = path.strip_prefix("/api/animals/") {
        if let Some(id) = rest.strip_suffix("/measurements") {
            if !id.is_empty() && !id.contains('/') {
                return animals::handle_add_scale(id, body, request_id, state, head, peer_ip);
            }
        }
    }
    Response::json(404, error_json(CODE_NOT_FOUND, "Not found", request_id))
        .with_policy(state.config.production)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_count_requests() {
        let m = ServerMetrics::new();
        assert_eq!(m.total(), 0);
        m.record_request();
        m.record_request();
        assert_eq!(m.total(), 2);
        assert!(m.uptime_secs() < 60);
    }
}
