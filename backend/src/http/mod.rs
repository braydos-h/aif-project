//! Hand-rolled HTTP/1.1 server on `std::net`, one thread per connection.
//!
//! The Rust binary serves the WebUI and the JSON API from the same origin.
//! Static routes are explicit compile-time assets; no request can read an
//! arbitrary filesystem path. Every response carries CORS headers, and JSON
//! responses carry a request id in both the body and `x-request-id` header.
//!
//! Split into focused submodules:
//! - [`server`] — listener loop and connection handling.
//! - [`response`] — response construction and header writing.
//! - [`request_id`] — per-request unique ids.
//! - [`assets`] — embedded WebUI files and the demo-image registry.
//! - [`validation`] — runtime option validation and limits.
//! - [`handlers`] — read-only info/demo handlers.
//! - [`estimate`] — `POST /estimate-weight` handler.

pub mod assets;
pub mod estimate;
pub mod handlers;
pub mod request_id;
pub mod response;
pub mod server;
pub mod validation;

pub use server::serve;

use serde_json::json;

use crate::cache::Cache;
use crate::config::Config;

use assets::{APP_JS, INDEX_HTML, STYLES_CSS};
use estimate::handle_estimate;
use handlers::{handle_demo_image, handle_demo_list, handle_info};
use response::{error_json, with_request_id, Response, CODE_NOT_FOUND};

/// Shared server state: config + cache, safe to hand to threads.
pub struct ServerState {
    pub config: Config,
    pub cache: Cache,
}

/// Dispatch a request to the WebUI, demo, or API handler.
fn dispatch(
    method: &str,
    path: &str,
    body: &[u8],
    request_id: &str,
    state: &ServerState,
) -> Response {
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
                }),
                request_id,
            ),
        ),
        ("GET", "/info") => handle_info(request_id, state),
        ("GET", "/demo-cows") => handle_demo_list(request_id),
        ("GET", path) if path.starts_with("/demo-cows/") => handle_demo_image(path, request_id),
        ("OPTIONS", _) => Response {
            status: 204,
            content_type: "text/plain; charset=utf-8",
            body: Vec::new(),
        },
        ("POST", "/estimate-weight") => handle_estimate(body, request_id, state),
        (_, _) => Response::json(404, error_json(CODE_NOT_FOUND, "Not found", request_id)),
    }
}
