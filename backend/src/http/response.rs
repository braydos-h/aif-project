//! HTTP response construction: status lines, JSON envelopes, headers.
//!
//! Every response carries CORS headers, and JSON responses carry a request
//! id in both the body and the `x-request-id` header.

use std::io::Write;
use std::net::TcpStream;

use serde_json::{json, Value};

use crate::hash::sha256_hex;

/// Error codes shared with the JSON API.
pub(crate) const CODE_MISSING_BODY: &str = "missing_body";
pub(crate) const CODE_BAD_REQUEST: &str = "bad_request";
pub(crate) const CODE_INVALID_JSON: &str = "invalid_json";
pub(crate) const CODE_MISSING_IMAGE: &str = "missing_image";
pub(crate) const CODE_INVALID_IMAGE: &str = "invalid_image";
pub(crate) const CODE_INVALID_OPTIONS: &str = "invalid_options";
pub(crate) const CODE_NOT_FOUND: &str = "not_found";
pub(crate) const CODE_ESTIMATION_FAILED: &str = "estimation_failed";
pub(crate) const CODE_SERVER_BUSY: &str = "server_busy";
pub(crate) const CODE_UNAUTHORIZED: &str = "unauthorized";
pub(crate) const CODE_FORBIDDEN: &str = "forbidden";
pub(crate) const CODE_RATE_LIMITED: &str = "rate_limited";
pub(crate) const CODE_CSRF: &str = "csrf_invalid";
pub(crate) const CODE_QUOTA: &str = "quota_exceeded";
pub(crate) const CODE_USER_LIMIT: &str = "user_limit";
pub(crate) const CODE_INVALID_CREDENTIALS: &str = "invalid_credentials";
pub(crate) const CODE_INVITE_INVALID: &str = "invite_invalid";
pub(crate) const CODE_INVITE_EXPIRED: &str = "invite_expired";
pub(crate) const CODE_INVITE_USED: &str = "invite_used";
pub(crate) const CODE_INVITE_REVOKED: &str = "invite_revoked";
pub(crate) const CODE_PAUSED: &str = "inference_paused";

/// A finished HTTP response.
pub(crate) struct Response {
    pub(crate) status: u16,
    pub(crate) content_type: &'static str,
    pub(crate) body: Vec<u8>,
    /// Extra headers (e.g. `Set-Cookie`). Names/values are sanitized at
    /// write time so token material cannot split headers.
    pub(crate) extra_headers: Vec<(String, String)>,
    /// Value for `Access-Control-Allow-Origin`, or `None` to omit it
    /// (production same-origin policy). Open local mode keeps `*` for
    /// existing API clients.
    pub(crate) cors_origin: Option<String>,
    /// Emit `Strict-Transport-Security` (production HTTPS only).
    pub(crate) hsts: bool,
    /// Filename for `Content-Disposition: attachment` downloads.
    pub(crate) attachment: Option<String>,
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
            extra_headers: Vec::new(),
            cors_origin: Some("*".to_string()),
            hsts: false,
            attachment: None,
        }
    }

    pub(crate) fn bytes(status: u16, content_type: &'static str, body: &[u8]) -> Response {
        Response {
            status,
            content_type,
            body: body.to_vec(),
            extra_headers: Vec::new(),
            cors_origin: Some("*".to_string()),
            hsts: false,
            attachment: None,
        }
    }

    pub(crate) fn csv(status: u16, body: Vec<u8>, filename: &str) -> Response {
        Response {
            status,
            content_type: "text/csv; charset=utf-8",
            body,
            extra_headers: Vec::new(),
            cors_origin: None,
            hsts: false,
            attachment: Some(filename.to_string()),
        }
    }

    /// Add an extra header. CR/LF are stripped so values carrying tokens
    /// or user text cannot split the response head.
    pub(crate) fn with_header(mut self, name: &str, value: &str) -> Response {
        let clean: String = value.chars().filter(|c| *c != '\r' && *c != '\n').collect();
        self.extra_headers.push((name.to_string(), clean));
        self
    }

    /// Apply the deployment CORS + HSTS policy from server config.
    pub(crate) fn with_policy(mut self, production: bool) -> Response {
        if production {
            self.cors_origin = None;
            self.hsts = true;
        }
        self
    }
}

pub(crate) fn status_text(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        429 => "Too Many Requests",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
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

/// Content hash for static-asset ETags: first 16 hex chars of SHA-256.
/// Length alone is not a hash — two different bodies with the same length
/// must never share an ETag.
pub(crate) fn static_etag(body: &[u8]) -> String {
    sha256_hex(body)[..16].to_string()
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
    // Compiled-in WebUI/demo bytes are immutable for a given binary build,
    // so browsers may cache them aggressively. JSON API responses stay
    // uncacheable by default (no Cache-Control/ETag emitted for them).
    let cache_headers = if !response.body.is_empty()
        && (response.content_type.starts_with("text/html")
            || response.content_type.starts_with("text/css")
            || response.content_type.contains("javascript")
            || response.content_type.starts_with("image/"))
    {
        // Content hash, not length: two different bodies with the same
        // length must never share an ETag.
        let digest = static_etag(&response.body);
        format!(
            "Cache-Control: public, max-age=3600, immutable\r\nETag: \"{}\"\r\n",
            digest
        )
    } else {
        String::new()
    };
    let mut head = format!(
        "HTTP/1.1 {} {}\r\n\
         Content-Type: {}\r\n\
         Content-Length: {}\r\n\
         x-request-id: {}\r\n\
         {}\
         Access-Control-Allow-Methods: POST, GET, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type, X-CSRF-Token\r\n\
         X-Content-Type-Options: nosniff\r\n\
         X-Frame-Options: DENY\r\n\
         Referrer-Policy: no-referrer\r\n\
         Content-Security-Policy: default-src 'self'; img-src 'self' blob: data:; style-src 'self'; script-src 'self'; connect-src 'self'; base-uri 'none'; form-action 'none'\r\n\
         {}{}{}",
        response.status,
        status_text(response.status),
        response.content_type,
        response.body.len(),
        request_id,
        response.cors_origin.as_ref().map(|origin| {
            // The origin is server-configured, never request-derived.
            let clean: String = origin.chars().filter(|c| *c != '\r' && *c != '\n').collect();
            format!("Access-Control-Allow-Origin: {}\r\n", clean)
        }).unwrap_or_default(),
        cache_headers,
        if response.hsts { "Strict-Transport-Security: max-age=63072000; includeSubDomains\r\n" } else { "" },
        response.attachment.as_ref().map(|name| {
            let clean: String = name.chars().filter(|c| *c != '\r' && *c != '\n' && *c != '"').collect();
            format!("Content-Disposition: attachment; filename=\"{}\"\r\n", clean)
        }).unwrap_or_default(),
    );
    for (name, value) in &response.extra_headers {
        // Names are server constants; values were sanitized at build time.
        // Re-check here so a future caller cannot split headers.
        if name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && !value.contains(['\r', '\n'])
        {
            head.push_str(&format!("{}: {}\r\n", name, value));
        }
    }
    head.push_str("Connection: close\r\n\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(&response.body)?;
    stream.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etags_differ_for_same_length_bodies() {
        assert_ne!(static_etag(b"abcd"), static_etag(b"abce"));
        assert_eq!(static_etag(b"abcd").len(), 16);
    }
}
