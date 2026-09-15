//! Read-only handlers: WebUI info, metrics, and demos.

use serde_json::{json, Value};

use super::assets::DEMO_COWS;
use super::response::{error_json, with_request_id, Response, CODE_NOT_FOUND};
use super::validation::safe_url_for_info;
use super::ServerState;
use crate::config::{DEFAULT_PROMPT, VERSION};

pub(crate) fn handle_info(request_id: &str, state: &ServerState) -> Response {
    Response::json(
        200,
        with_request_id(
            json!({
                "name": "Cow Weight Estimator",
                "version": VERSION,
                "backend": state.config.backend,
                "model": state.config.model,
                "ollama_url": safe_url_for_info(&state.config.ollama_url),
                "ollama_configured": state.config.ollama_api_key.is_some(),
                "default_prompt": DEFAULT_PROMPT,
                "endpoints": [
                    "GET /",
                    "GET /styles.css",
                    "GET /app.js",
                    "GET /info",
                    "GET /health",
                    "GET /metrics",
                    "GET /demo-cows",
                    "GET /demo-cows/{id}",
                    "POST /estimate-weight",
                    "POST /estimate-batch"
                ],
            }),
            request_id,
        ),
    )
}

/// Operator observability: uptime, request totals, live connection count,
/// rejected (over-limit) count, and cache size. No secrets are included.
pub(crate) fn handle_metrics(request_id: &str, state: &ServerState) -> Response {
    Response::json(
        200,
        with_request_id(
            json!({
                "version": VERSION,
                "backend": state.config.backend,
                "model": state.config.model,
                "uptime_secs": state.metrics.uptime_secs(),
                "total_requests": state.metrics.total(),
                "active_connections": state.metrics.active(),
                "rejected_connections": state.metrics.rejected(),
                "cache_entries": state.cache.len(),
            }),
            request_id,
        ),
    )
}

pub(crate) fn handle_demo_list(request_id: &str) -> Response {
    let demos: Vec<Value> = DEMO_COWS
        .iter()
        .map(|demo| {
            json!({
                "id": demo.id,
                "name": demo.name,
                "mime_type": demo.mime_type,
                "url": format!("/demo-cows/{}", demo.id),
            })
        })
        .collect();
    Response::json(200, with_request_id(json!({"demos": demos}), request_id))
}

pub(crate) fn handle_demo_image(path: &str, request_id: &str) -> Response {
    let id = path.strip_prefix("/demo-cows/").unwrap_or("");
    if id.is_empty() || id.contains('/') {
        return Response::json(404, error_json(CODE_NOT_FOUND, "Not found", request_id));
    }
    match DEMO_COWS.iter().find(|demo| demo.id == id) {
        Some(demo) => Response::bytes(200, demo.mime_type, demo.body),
        None => Response::json(404, error_json(CODE_NOT_FOUND, "Not found", request_id)),
    }
}
