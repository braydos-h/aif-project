//! Hand-rolled HTTP/1.1 server on `std::net`, one thread per connection.
//!
//! The Rust binary serves the WebUI and the JSON API from the same origin.
//! Static routes are explicit compile-time assets; no request can read an
//! arbitrary filesystem path. Every response carries CORS headers, and JSON
//! responses carry a request id in both the body and `x-request-id` header.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};

use crate::cache::Cache;
use crate::config::{Config, DEFAULT_OLLAMA_URL, DEFAULT_PROMPT};
use crate::fallback::estimate_fallback;
use crate::ollama::estimate_via_ollama;
use crate::validate::ImageValidationError;

const MAX_BODY_BYTES: usize = 20 * 1024 * 1024;
const MAX_PROMPT_BYTES: usize = 16 * 1024;
const MAX_MODEL_BYTES: usize = 256;
const MAX_URL_BYTES: usize = 2 * 1024;
const MAX_API_KEY_BYTES: usize = 4 * 1024;

const INDEX_HTML: &[u8] = include_bytes!("../../web/index.html");
const STYLES_CSS: &[u8] = include_bytes!("../../web/styles.css");
const APP_JS: &[u8] = include_bytes!("../../web/app.js");

/// The only demo files exposed by the server. They are compiled into the
/// binary so the route cannot escape the approved `cows/` directory.
#[derive(Clone, Copy)]
struct DemoCow {
    id: &'static str,
    name: &'static str,
    mime_type: &'static str,
    body: &'static [u8],
}

const DEMO_COWS: &[DemoCow] = &[
    DemoCow {
        id: "1",
        name: "cow 1.webp",
        mime_type: "image/webp",
        body: include_bytes!("../../cows/cow 1.webp"),
    },
    DemoCow {
        id: "2",
        name: "cow 2.jpg",
        mime_type: "image/jpeg",
        body: include_bytes!("../../cows/cow 2.jpg"),
    },
    DemoCow {
        id: "3",
        name: "cow 3.jpg",
        mime_type: "image/jpeg",
        body: include_bytes!("../../cows/cow 3.jpg"),
    },
];

/// Error codes shared with the JSON API.
const CODE_MISSING_BODY: &str = "missing_body";
const CODE_INVALID_JSON: &str = "invalid_json";
const CODE_MISSING_IMAGE: &str = "missing_image";
const CODE_INVALID_IMAGE: &str = "invalid_image";
const CODE_INVALID_OPTIONS: &str = "invalid_options";
const CODE_NOT_FOUND: &str = "not_found";
const CODE_ESTIMATION_FAILED: &str = "estimation_failed";

/// Shared server state: config + cache, safe to hand to threads.
pub struct ServerState {
    pub config: Config,
    pub cache: Cache,
}

/// Serve on `host:port` (port 0 picks an ephemeral port, printed on stdout
/// so the launcher can read it). Blocks forever.
pub fn serve(state: Arc<ServerState>, host: &str, port: u16) -> std::io::Result<()> {
    let listener = TcpListener::bind((host, port))?;
    let actual = listener.local_addr()?;
    println!("listening on http://{}:{}/", actual.ip(), actual.port());
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let state = Arc::clone(&state);
                thread::spawn(move || {
                    let _ = handle_connection(stream, &state);
                });
            }
            Err(e) => eprintln!("accept error: {}", e),
        }
    }
    Ok(())
}

use std::sync::atomic::{AtomicU64, Ordering};

/// Per-process counter mixed into request ids so two requests in the same
/// clock tick still differ.
static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Generate a short unique id for the current request (8 hex chars).
fn new_request_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let counter = REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut hasher, nanos.to_le_bytes());
    sha2::Digest::update(&mut hasher, std::process::id().to_le_bytes());
    sha2::Digest::update(&mut hasher, counter.to_le_bytes());
    let digest = sha2::Digest::finalize(hasher);
    let mut out = String::with_capacity(8);
    for b in digest.iter().take(4) {
        out.push_str(&format!("{:02x}", b));
    }
    out
}

/// A finished HTTP response.
struct Response {
    status: u16,
    content_type: &'static str,
    body: Vec<u8>,
}

impl Response {
    fn json(status: u16, payload: Value) -> Response {
        let mut body = payload.to_string().into_bytes();
        // Header injection guard: JSON escapes model text, but keep this
        // defensive check at the final response boundary.
        body.retain(|b| *b != b'\r' && *b != b'\n');
        Response {
            status,
            content_type: "application/json; charset=utf-8",
            body,
        }
    }

    fn bytes(status: u16, content_type: &'static str, body: &[u8]) -> Response {
        Response {
            status,
            content_type,
            body: body.to_vec(),
        }
    }
}

fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        502 => "Bad Gateway",
        _ => "Error",
    }
}

/// Inject the request id into a JSON body unless already present.
fn with_request_id(payload: Value, request_id: &str) -> Value {
    if payload.get("request_id").is_some() {
        return payload;
    }
    if let Some(obj) = payload.as_object() {
        let mut obj = obj.clone();
        obj.insert("request_id".to_string(), Value::from(request_id));
        return Value::Object(obj);
    }
    payload
}

/// Build the JSON error body with a machine-readable code.
fn error_json(code: &str, message: &str, request_id: &str) -> Value {
    json!({
        "error": message,
        "code": code,
        "request_id": request_id,
    })
}

fn write_response(
    stream: &mut TcpStream,
    request_id: &str,
    response: &Response,
) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {} {}\r\n\
         Content-Type: {}\r\n\
         Content-Length: {}\r\n\
         x-request-id: {}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: POST, GET, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Referrer-Policy: no-referrer\r\n\
         Content-Security-Policy: default-src 'self'; img-src 'self' blob: data:; style-src 'self'; script-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'\r\n\
         Connection: close\r\n\
         \r\n",
        response.status,
        status_text(response.status),
        response.content_type,
        response.body.len(),
        request_id,
    );
    stream.write_all(head.as_bytes())?;
    stream.write_all(&response.body)?;
    stream.flush()
}

/// Read the request line + headers + body from the connection and produce a
/// response. One connection = one request (Connection: close).
fn handle_connection(mut stream: TcpStream, state: &ServerState) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let request_id = new_request_id();
    let mut reader = BufReader::new(stream.try_clone()?);

    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(()); // client closed before sending anything
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("")
        .to_string();

    let mut content_length = 0usize;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("Content-Length") {
                content_length = value.trim().parse().unwrap_or(0);
            }
        }
    }

    if method == "POST" && content_length > MAX_BODY_BYTES {
        let response = Response::json(
            400,
            error_json(CODE_INVALID_JSON, "Request body too large", &request_id),
        );
        return write_response(&mut stream, &request_id, &response);
    }

    let mut body = Vec::new();
    if method == "POST" && content_length > 0 {
        body.resize(content_length, 0);
        if reader.read_exact(&mut body).is_err() {
            let response = Response::json(
                400,
                error_json(CODE_INVALID_JSON, "Truncated request body", &request_id),
            );
            return write_response(&mut stream, &request_id, &response);
        }
    }

    let response = dispatch(&method, &path, &body, &request_id, state);
    write_response(&mut stream, &request_id, &response)
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

fn handle_info(request_id: &str, state: &ServerState) -> Response {
    Response::json(
        200,
        with_request_id(
            json!({
                "name": "Cow Weight Estimator",
                "version": crate::config::VERSION,
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
                    "GET /demo-cows",
                    "GET /demo-cows/{id}",
                    "POST /estimate-weight"
                ],
            }),
            request_id,
        ),
    )
}

fn handle_demo_list(request_id: &str) -> Response {
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

fn handle_demo_image(path: &str, request_id: &str) -> Response {
    let id = path.strip_prefix("/demo-cows/").unwrap_or("");
    if id.is_empty() || id.contains('/') {
        return Response::json(404, error_json(CODE_NOT_FOUND, "Not found", request_id));
    }
    match DEMO_COWS.iter().find(|demo| demo.id == id) {
        Some(demo) => Response::bytes(200, demo.mime_type, demo.body),
        None => Response::json(404, error_json(CODE_NOT_FOUND, "Not found", request_id)),
    }
}

fn optional_string<'a>(payload: &'a Value, field: &str) -> Result<Option<&'a str>, String> {
    match payload.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .map(Some)
            .ok_or_else(|| format!("{} must be a string", field)),
    }
}

fn has_control_or_whitespace(value: &str) -> bool {
    value
        .bytes()
        .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace())
}

fn valid_http_url(url: &str) -> bool {
    if url.is_empty()
        || url.len() > MAX_URL_BYTES
        || has_control_or_whitespace(url)
        || url.contains('@')
        || url.contains('?')
        || url.contains('#')
    {
        return false;
    }
    let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or("");
    let host = authority.split(':').next().unwrap_or("");
    !host.is_empty()
}

fn validate_runtime_config(config: &Config, prompt: &str) -> Result<(), String> {
    if config.backend != "ollama" && config.backend != "none" {
        return Err("Unsupported backend. Choose ollama or none.".to_string());
    }
    if config.model.is_empty()
        || config.model.len() > MAX_MODEL_BYTES
        || has_control_or_whitespace(&config.model)
    {
        return Err("Model must be a non-empty single-line value under 256 bytes.".to_string());
    }
    if !valid_http_url(&config.ollama_url) {
        return Err(
            "Ollama URL must be an http(s) URL without credentials or query parameters."
                .to_string(),
        );
    }
    if let Some(key) = &config.ollama_api_key {
        if key.len() > MAX_API_KEY_BYTES || key.bytes().any(|byte| byte.is_ascii_control()) {
            return Err("API key is too long or contains invalid control characters.".to_string());
        }
    }
    if prompt.len() > MAX_PROMPT_BYTES {
        return Err("Prompt is too long; keep it under 16 KiB.".to_string());
    }
    Ok(())
}

fn safe_url_for_info(url: &str) -> String {
    if valid_http_url(url) {
        url.to_string()
    } else {
        DEFAULT_OLLAMA_URL.to_string()
    }
}

fn redact_secret(message: &str, configs: [&Config; 2]) -> String {
    let mut redacted = message.to_string();
    for config in configs {
        if let Some(key) = &config.ollama_api_key {
            if !key.is_empty() {
                redacted = redacted.replace(key, "[redacted]");
            }
        }
    }
    redacted
}

fn handle_estimate(body: &[u8], request_id: &str, state: &ServerState) -> Response {
    if body.is_empty() {
        return Response::json(
            400,
            error_json(CODE_MISSING_BODY, "Missing request body", request_id),
        );
    }
    let payload: Value = match serde_json::from_slice(body) {
        Ok(p) => p,
        Err(_) => {
            return Response::json(
                400,
                error_json(CODE_INVALID_JSON, "Invalid JSON payload", request_id),
            );
        }
    };

    let image_url = match optional_string(&payload, "image_url") {
        Ok(value) => value,
        Err(message) => {
            return Response::json(400, error_json(CODE_INVALID_OPTIONS, &message, request_id));
        }
    };
    let image_base64 = match optional_string(&payload, "image_base64") {
        Ok(value) => value,
        Err(message) => {
            return Response::json(400, error_json(CODE_INVALID_OPTIONS, &message, request_id));
        }
    };
    let prompt_value = match optional_string(&payload, "prompt") {
        Ok(value) => value,
        Err(message) => {
            return Response::json(400, error_json(CODE_INVALID_OPTIONS, &message, request_id));
        }
    };
    let prompt = prompt_value
        .filter(|value| !value.is_empty())
        .unwrap_or(DEFAULT_PROMPT);

    let image_reference = image_url
        .filter(|value| !value.is_empty())
        .or_else(|| image_base64.filter(|value| !value.is_empty()));
    let Some(image_reference) = image_reference else {
        return Response::json(
            400,
            error_json(
                CODE_MISSING_IMAGE,
                "Provide image_url or image_base64 in request payload",
                request_id,
            ),
        );
    };

    let mut request_config = state.config.clone();
    for (field, target) in [
        ("backend", &mut request_config.backend),
        ("model", &mut request_config.model),
        ("ollama_url", &mut request_config.ollama_url),
    ] {
        let value = match optional_string(&payload, field) {
            Ok(value) => value,
            Err(message) => {
                return Response::json(400, error_json(CODE_INVALID_OPTIONS, &message, request_id));
            }
        };
        if let Some(value) = value {
            *target = value.to_string();
        }
    }
    let api_key = match optional_string(&payload, "ollama_api_key") {
        Ok(value) => value,
        Err(message) => {
            return Response::json(400, error_json(CODE_INVALID_OPTIONS, &message, request_id));
        }
    };
    if let Some(value) = api_key {
        request_config.ollama_api_key = (!value.is_empty()).then(|| value.to_string());
    }

    if let Err(message) = validate_runtime_config(&request_config, prompt) {
        return Response::json(400, error_json(CODE_INVALID_OPTIONS, &message, request_id));
    }

    let result = match request_config.backend.as_str() {
        "none" => Ok(estimate_fallback(image_reference, prompt)),
        "ollama" => estimate_via_ollama(&request_config, &state.cache, image_reference, prompt),
        _ => unreachable!("validate_runtime_config checked backend"),
    };

    match result {
        Ok(result) => Response::json(200, with_request_id(result, request_id)),
        Err(error) => {
            let message = redact_secret(&error.to_string(), [&state.config, &request_config]);
            if error.is::<ImageValidationError>() {
                eprintln!("invalid_image [{}]: {}", request_id, message);
                Response::json(400, error_json(CODE_INVALID_IMAGE, &message, request_id))
            } else {
                eprintln!("estimation failed [{}]: {}", request_id, message);
                Response::json(
                    502,
                    error_json(CODE_ESTIMATION_FAILED, &message, request_id),
                )
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_ids_are_unique_under_burst() {
        let mut ids = std::collections::HashSet::new();
        for _ in 0..5000 {
            ids.insert(new_request_id());
        }
        assert_eq!(ids.len(), 5000);
    }

    #[test]
    fn static_routes_are_explicit_and_options_are_validated() {
        assert!(valid_http_url("https://ollama.com/api/generate"));
        assert!(valid_http_url("http://127.0.0.1:11434/api/generate"));
        assert!(!valid_http_url("file:///secret"));
        assert!(!valid_http_url("https://user:secret@example.com/api"));
        assert!(!valid_http_url("https://example.com/api?key=secret"));
    }
}
