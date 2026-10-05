//! Private retained-photo routes (roadmap item 8).
//!
//! Available when the operator enables `AIF_RETAIN_PHOTOS=1`. Downloads
//! are owner-only, served from opaque server-generated ids (URL ids are
//! allow-list validated, never used to build paths unchecked), and carry
//! `Cache-Control: private`. History and account deletion remove linked
//! photos; expiry and orphans are swept at startup and periodically.

use serde_json::{json, Value};

use super::response::{error_json, with_request_id, Response};
use super::server::RequestHead;
use super::session::{check_csrf, require_auth};
use super::ServerState;
use crate::photos::valid_photo_id;

fn photo_json(photo: &crate::db::Photo, state: &ServerState) -> Value {
    let _ = state;
    json!({
        "id": photo.id,
        "estimate_id": photo.estimate_id,
        "created_at": photo.created_at,
        "expires_at": photo.expires_at,
        "bytes": photo.bytes,
        "mime": photo.mime,
        "url": format!("/api/photos/{}", photo.id),
    })
}

fn not_found(state: &ServerState, request_id: &str) -> Response {
    Response::json(
        404,
        error_json(super::response::CODE_NOT_FOUND, "Not found", request_id),
    )
    .with_policy(state.config.production)
}

fn owned_photo(
    state: &ServerState,
    user_id: &str,
    id: &str,
    request_id: &str,
) -> Result<crate::db::Photo, Response> {
    if !valid_photo_id(id) {
        return Err(not_found(state, request_id));
    }
    match state.db.photo_by_id(id) {
        Ok(Some(photo)) if photo.user_id == user_id => Ok(photo),
        _ => Err(not_found(state, request_id)),
    }
}

/// `GET /api/photos` — list the caller's retained photo metadata.
pub(crate) fn handle_list(
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let requester = match require_auth(state, head, request_id, peer_ip) {
        Ok(r) => r,
        Err(r) => return r,
    };
    match state.db.list_photos(&requester.user.id, 200) {
        Ok(photos) => {
            let items: Vec<Value> = photos.iter().map(|p| photo_json(p, state)).collect();
            Response::json(200, with_request_id(json!({"photos": items}), request_id))
                .with_policy(state.config.production)
        }
        Err(_) => Response::json(
            502,
            error_json(
                super::response::CODE_ESTIMATION_FAILED,
                "Could not load photos",
                request_id,
            ),
        )
        .with_policy(state.config.production),
    }
}

/// `GET /api/photos/{id}` — owner-only download.
pub(crate) fn handle_get(
    id: &str,
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let requester = match require_auth(state, head, request_id, peer_ip) {
        Ok(r) => r,
        Err(r) => return r,
    };
    let photo = match owned_photo(state, &requester.user.id, id, request_id) {
        Ok(p) => p,
        Err(r) => return r,
    };
    match crate::photos::read_photo(&state.config.data_dir, &photo.id) {
        Some(bytes) => {
            let mut response = Response::bytes(200, photo_mime(&photo.mime), &bytes);
            response.cors_origin = None;
            response.cacheable = false;
            response = response.with_policy(state.config.production);
            response.with_header("Cache-Control", "private, max-age=86400")
        }
        None => not_found(state, request_id),
    }
}

fn photo_mime(mime: &str) -> &'static str {
    match mime {
        "image/png" => "image/png",
        "image/jpeg" => "image/jpeg",
        "image/webp" => "image/webp",
        "image/gif" => "image/gif",
        "image/bmp" => "image/bmp",
        _ => "application/octet-stream",
    }
}

/// `DELETE /api/photos/{id}` — owner-only deletion (row + file).
pub(crate) fn handle_delete(
    id: &str,
    body: &[u8],
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let _ = body;
    let requester = match require_auth(state, head, request_id, peer_ip) {
        Ok(r) => r,
        Err(r) => return r,
    };
    if let Err(r) = check_csrf(state, head, &requester, request_id) {
        return r;
    }
    match owned_photo(state, &requester.user.id, id, request_id) {
        Ok(photo) => {
            let _ = crate::photos::delete_photo(&state.db, &state.config.data_dir, &photo.id);
            state.audit(
                Some(&requester.user.id),
                "photo_deleted",
                &format!("photo={}", photo.id),
                request_id,
            );
            Response::json(200, with_request_id(json!({"ok": true}), request_id))
                .with_policy(state.config.production)
        }
        Err(r) => r,
    }
}
