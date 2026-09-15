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
//! Split into focused submodules:
//! - [`server`] — listener loop and connection handling.
//! - [`response`] — response construction and header writing.
//! - [`request_id`] — per-request unique ids.
//! - [`assets`] — embedded WebUI files and the demo-image registry.
//! - [`validation`] — runtime option validation and limits.
//! - [`handlers`] — read-only info/demo handlers.
//! - [`estimate`] — `POST /estimate-weight` + `POST /estimate-batch`.

pub mod assets;
pub mod estimate;
pub mod handlers;
pub mod request_id;
pub mod response;
pub mod server;
pub mod validation;

pub use server::serve;

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Instant;

use serde_json::json;

use crate::cache::Cache;
use crate::config::Config;

use assets::{APP_JS, INDEX_HTML, STYLES_CSS};
use estimate::{handle_estimate, handle_estimate_batch};
use handlers::{handle_demo_image, handle_demo_list, handle_info, handle_metrics};
use response::{error_json, with_request_id, Response, CODE_NOT_FOUND};

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

/// Shared server state: config + cache + metrics, safe to hand to threads.
pub struct ServerState {
    pub config: Config,
    pub cache: Cache,
    pub metrics: ServerMetrics,
}

impl ServerState {
    /// Build shared state from the effective config.
    pub fn new(config: Config) -> Self {
        let cache = Cache::new(config.cache_ttl);
        ServerState {
            config,
            cache,
            metrics: ServerMetrics::new(),
        }
    }
}

/// Dispatch a request to the WebUI, demo, or API handler.
fn dispatch(
    method: &str,
    path: &str,
    body: &[u8],
    request_id: &str,
    state: &ServerState,
) -> Response {
    state.metrics.record_request();
    match (method, path) {
        ("GET", "/") => Response::bytes(200, "text/html; charset=utf-8", INDEX_HTML),
        ("GET", "/styles.css") => Response::bytes(200, "text/css; charset=utf-8", STYLES_CSS),
        ("GET", "/app.js") => Response::bytes(200, "application/javascript; charset=utf-8", APP_JS),
        ("GET", "/health") => Response::json(
            200,
            with_request_id(
                json!({
                    "status": "ok",
                    "backend": state.config.backend,
                    "model": state.config.model,
                    "ollama_configured": state.config.ollama_api_key.is_some(),
                    "uptime_secs": state.metrics.uptime_secs(),
                    "active_connections": state.metrics.active(),
                }),
                request_id,
            ),
        ),
        ("GET", "/info") => handle_info(request_id, state),
        ("GET", "/metrics") => handle_metrics(request_id, state),
        ("GET", "/demo-cows") => handle_demo_list(request_id),
        ("GET", path) if path.starts_with("/demo-cows/") => handle_demo_image(path, request_id),
        ("OPTIONS", _) => Response {
            status: 204,
            content_type: "text/plain; charset=utf-8",
            body: Vec::new(),
        },
        ("POST", "/estimate-weight") => handle_estimate(body, request_id, state),
        ("POST", "/estimate-batch") => handle_estimate_batch(body, request_id, state),
        (_, _) => Response::json(404, error_json(CODE_NOT_FOUND, "Not found", request_id)),
    }
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
