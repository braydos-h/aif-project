//! Hand-rolled HTTP/1.1 server on `std::net`, one thread per connection.
//!
//! Serves the same API contract as the old `EstimateHandler` in
//! `aif/server.py`: `POST /estimate-weight`, `GET /health`, `GET /` /
//! `GET /info`, `OPTIONS` (CORS preflight → 204), 404 otherwise. Every
//! response carries a `request_id` (8-char hex) in the JSON body and the
//! `x-request-id` header, plus CORS headers. Errors carry a machine-readable
//! `code` field alongside a human `error` message.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use crate::cache::Cache;
use crate::config::Config;
use crate::fallback::estimate_fallback;
use crate::ollama::estimate_via_ollama;
use crate::validate::ImageValidationError;

const MAX_BODY_BYTES: usize = 20 * 1024 * 1024;

/// Error codes shared with the Python API.
const CODE_MISSING_BODY: &str = "missing_body";
const CODE_INVALID_JSON: &str = "invalid_json";
const CODE_MISSING_IMAGE: &str = "missing_image";
const CODE_INVALID_IMAGE: &str = "invalid_image";
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
    // The launcher (app.py) reads this when port=0.
    println!("listening on http://{}:{}", actual.ip(), actual.port());
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

/// Generate a short unique id for the current request (8 hex chars).
fn new_request_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let mut hasher = <sha2::Sha256 as sha2::Digest>::new();
    sha2::Digest::update(&mut hasher, &nanos.to_le_bytes());
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
        // Header injection guard: the model text inside the JSON must not
        // smuggle CR/LF into the response stream.
        body.retain(|b| *b != b'\r' && *b != b'\n');
        Response {
            status,
            content_type: "application/json",
            body,
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
/// response. One connection = one request (Connection: close), matching the
/// old Python server's keep-alive-free behavior.
fn handle_connection(mut stream: TcpStream, state: &ServerState) -> std::io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(Duration::from_secs(30)))?;
    let request_id = new_request_id();
    let mut reader = BufReader::new(stream.try_clone()?);

    let mut request_line = String::new();
    if reader.read_line(&mut request_line)? == 0 {
        return Ok(()); // client closed before sending anything
    }
    let mut parts = request_line.trim_end().split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let path = parts.next().unwrap_or("").to_string();

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
        reader.read_exact(&mut body)?;
    }

    let response = dispatch(&method, &path, &body, &request_id, state);
    write_response(&mut stream, &request_id, &response)
}

/// Dispatch a request to the right handler.
fn dispatch(
    method: &str,
    path: &str,
    body: &[u8],
    request_id: &str,
    state: &ServerState,
) -> Response {
    match (method, path) {
        ("GET", "/health") => Response::json(
            200,
            with_request_id(
                json!({
                    "status": "ok",
                    "backend": state.config.backend,
                    "model": state.config.model,
                }),
                request_id,
            ),
        ),
        ("GET", "/") | ("GET", "/info") => Response::json(
            200,
            with_request_id(
                json!({
                    "name": "Cow Weight Estimator",
                    "version": crate::config::VERSION,
                    "endpoints": ["POST /estimate-weight", "GET /health", "GET /"],
                }),
                request_id,
            ),
        ),
        ("OPTIONS", _) => Response {
            status: 204,
            content_type: "application/json",
            body: Vec::new(),
        },
        ("POST", "/estimate-weight") => handle_estimate(body, request_id, state),
        (_, _) => Response::json(404, error_json(CODE_NOT_FOUND, "Not found", request_id)),
    }
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
    let image_url = payload.get("image_url").and_then(|v| v.as_str());
    let image_base64 = payload.get("image_base64").and_then(|v| v.as_str());
    let prompt = payload
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or(crate::config::DEFAULT_PROMPT);

    let image_reference = image_url
        .filter(|s| !s.is_empty())
        .or_else(|| image_base64.filter(|s| !s.is_empty()));
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

    let result = if state.config.backend == "none" {
        estimate_fallback(image_reference, prompt)
    } else {
        match estimate_via_ollama(&state.config, &state.cache, image_reference, prompt) {
            Ok(r) => r,
            Err(e) => {
                if e.is::<ImageValidationError>() {
                    eprintln!("invalid_image [{}]: {}", request_id, e);
                    return Response::json(
                        400,
                        error_json(CODE_INVALID_IMAGE, &e.to_string(), request_id),
                    );
                }
                eprintln!("estimation failed [{}]: {}", request_id, e);
                return Response::json(
                    502,
                    error_json(CODE_ESTIMATION_FAILED, &e.to_string(), request_id),
                );
            }
        }
    };

    Response::json(200, with_request_id(result, request_id))
}
