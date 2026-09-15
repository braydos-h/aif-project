//! HTTP response construction: status lines, JSON envelopes, headers.
//!
//! Every response carries CORS headers, and JSON responses carry a request
//! id in both the body and the `x-request-id` header.

use std::io::Write;
use std::net::TcpStream;

use serde_json::{json, Value};

/// Error codes shared with the JSON API.
pub(crate) const CODE_MISSING_BODY: &str = "missing_body";
pub(crate) const CODE_INVALID_JSON: &str = "invalid_json";
pub(crate) const CODE_MISSING_IMAGE: &str = "missing_image";
pub(crate) const CODE_INVALID_IMAGE: &str = "invalid_image";
pub(crate) const CODE_INVALID_OPTIONS: &str = "invalid_options";
pub(crate) const CODE_NOT_FOUND: &str = "not_found";
pub(crate) const CODE_ESTIMATION_FAILED: &str = "estimation_failed";

/// A finished HTTP response.
pub(crate) struct Response {
    pub(crate) status: u16,
    pub(crate) content_type: &'static str,
    pub(crate) body: Vec<u8>,
}

impl Response {
    pub(crate) fn json(status: u16, payload: Value) -> Response {
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

    pub(crate) fn bytes(status: u16, content_type: &'static str, body: &[u8]) -> Response {
        Response {
            status,
            content_type,
            body: body.to_vec(),
        }
    }
}

pub(crate) fn status_text(status: u16) -> &'static str {
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
pub(crate) fn with_request_id(payload: Value, request_id: &str) -> Value {
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
pub(crate) fn error_json(code: &str, message: &str, request_id: &str) -> Value {
    json!({
        "error": message,
        "code": code,
        "request_id": request_id,
    })
}

pub(crate) fn write_response(
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
