//! Owner-scoped batch records grouping the existing durable estimate jobs.
//! A batch stores counts and safe filenames only; image payloads live in each
//! queued job until processing ends, then are erased by the database layer.

use serde_json::{json, Value};

use super::json_util::need_json;
use super::response::{error_json, with_request_id, Response};
use super::server::RequestHead;
use super::session::{check_csrf, require_auth};
use super::ServerState;
use crate::auth::new_id;
use crate::time_util::rfc3339;

fn batch_json(state: &ServerState, batch: &crate::db::UploadBatch) -> Result<Value, String> {
    let jobs = state.db.upload_batch_jobs(&batch.id)?;
    let succeeded = jobs.iter().filter(|j| j.status == "success").count();
    let failed = jobs
        .iter()
        .filter(|j| j.status == "failed" || j.status == "expired")
        .count();
    let cancelled = jobs.iter().filter(|j| j.status == "cancelled").count();
    let active = jobs
        .iter()
        .any(|j| j.status == "queued" || j.status == "active");
    let status = if active {
        "processing"
    } else if jobs.is_empty() {
        "pending"
    } else if jobs.len() < batch.expected_count as usize || failed > 0 || cancelled > 0 {
        "partial"
    } else {
        "complete"
    };
    let items: Vec<Value> = jobs
        .iter()
        .map(|job| {
            let result = job
                .result
                .as_deref()
                .and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or(Value::Null);
            json!({
                "id": job.id, "index": job.batch_index, "filename": job.filename,
                "status": job.status, "attempts": job.attempts, "created_at": job.created_at,
                "error_code": job.error_code, "error_message": job.error_message,
                "history_id": job.history_id, "result": result,
            })
        })
        .collect();
    Ok(json!({
        "id": batch.id, "created_at": batch.created_at, "status": status,
        "expected_count": batch.expected_count, "submitted_count": jobs.len(),
        "succeeded_count": succeeded, "failed_count": failed,
        "cancelled_count": cancelled, "items": items,
    }))
}

fn not_found(state: &ServerState, request_id: &str) -> Response {
    Response::json(
        404,
        error_json(super::response::CODE_NOT_FOUND, "Not found", request_id),
    )
    .with_policy(state.config.production)
}

/// `POST /api/upload-batches` — create an owner-scoped metadata record.
pub(crate) fn handle_create(
    body: &[u8],
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let requester = match require_auth(state, head, request_id, peer_ip) {
        Ok(v) => v,
        Err(r) => return r,
    };
    if let Err(r) = check_csrf(state, head, &requester, request_id) {
        return r;
    }
    let payload = match need_json(state, body, request_id) {
        Ok(v) => v,
        Err(r) => return r,
    };
    let count = payload
        .get("item_count")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    if !(1..=20).contains(&count) {
        return Response::json(
            400,
            error_json(
                super::response::CODE_INVALID_OPTIONS,
                "Batch item_count must be between 1 and 20",
                request_id,
            ),
        )
        .with_policy(state.config.production);
    }
    let id = new_id();
    let now = rfc3339(crate::time_util::unix_now());
    match state
        .db
        .create_upload_batch(&id, &requester.user.id, count, &now)
    {
        Ok(()) => {
            let batch = state.db.upload_batch_by_id(&id).ok().flatten();
            let out = batch
                .as_ref()
                .and_then(|b| batch_json(state, b).ok())
                .unwrap_or(json!({"id":id,"status":"pending","expected_count":count,"items":[]}));
            Response::json(201, with_request_id(out, request_id))
                .with_policy(state.config.production)
        }
        Err(_) => Response::json(
            502,
            error_json(
                super::response::CODE_ESTIMATION_FAILED,
                "Could not create upload batch",
                request_id,
            ),
        )
        .with_policy(state.config.production),
    }
}

/// `GET /api/upload-batches` — list the caller's batch records.
pub(crate) fn handle_list(
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let requester = match require_auth(state, head, request_id, peer_ip) {
        Ok(v) => v,
        Err(r) => return r,
    };
    match state.db.list_upload_batches(&requester.user.id, 100) {
        Ok(batches) => {
            let items: Vec<Value> = batches
                .iter()
                .filter_map(|b| batch_json(state, b).ok())
                .collect();
            Response::json(200, with_request_id(json!({"batches":items}), request_id))
                .with_policy(state.config.production)
        }
        Err(_) => Response::json(
            502,
            error_json(
                super::response::CODE_ESTIMATION_FAILED,
                "Could not load upload batches",
                request_id,
            ),
        )
        .with_policy(state.config.production),
    }
}

/// `GET /api/upload-batches/{id}` — detail and per-item status, owner only.
pub(crate) fn handle_get(
    id: &str,
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let requester = match require_auth(state, head, request_id, peer_ip) {
        Ok(v) => v,
        Err(r) => return r,
    };
    if id.len() > 64 {
        return not_found(state, request_id);
    }
    match state.db.upload_batch_by_id(id) {
        Ok(Some(batch)) if batch.user_id == requester.user.id => match batch_json(state, &batch) {
            Ok(out) => Response::json(200, with_request_id(out, request_id))
                .with_policy(state.config.production),
            Err(_) => Response::json(
                502,
                error_json(
                    super::response::CODE_ESTIMATION_FAILED,
                    "Could not load upload batch",
                    request_id,
                ),
            )
            .with_policy(state.config.production),
        },
        _ => not_found(state, request_id),
    }
}

/// `POST /api/upload-batches/{id}/cancel` — cancel pending items; active work
/// may complete and remains visible in the batch detail.
pub(crate) fn handle_cancel(
    id: &str,
    body: &[u8],
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let requester = match require_auth(state, head, request_id, peer_ip) {
        Ok(v) => v,
        Err(r) => return r,
    };
    if let Err(r) = check_csrf(state, head, &requester, request_id) {
        return r;
    }
    if !body.is_empty() && need_json(state, body, request_id).is_err() {
        return Response::json(
            400,
            error_json(
                super::response::CODE_INVALID_JSON,
                "Invalid cancel request",
                request_id,
            ),
        )
        .with_policy(state.config.production);
    }
    let batch = match state.db.upload_batch_by_id(id) {
        Ok(Some(v)) if v.user_id == requester.user.id => v,
        _ => return not_found(state, request_id),
    };
    if state
        .db
        .cancel_upload_batch(
            id,
            &requester.user.id,
            &rfc3339(crate::time_util::unix_now()),
        )
        .is_err()
    {
        return Response::json(
            502,
            error_json(
                super::response::CODE_ESTIMATION_FAILED,
                "Could not cancel batch",
                request_id,
            ),
        )
        .with_policy(state.config.production);
    }
    let out = batch_json(state, &batch).unwrap_or(json!({"id":id,"status":"partial"}));
    Response::json(200, with_request_id(out, request_id)).with_policy(state.config.production)
}

/// `POST /api/upload-batches/{id}/items/{index}/retry` — retry one terminal
/// item with its original idempotency key and a fresh client-held photo.
pub(crate) fn handle_retry(
    id: &str,
    index: i64,
    body: &[u8],
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let requester = match require_auth(state, head, request_id, peer_ip) {
        Ok(v) => v,
        Err(r) => return r,
    };
    if let Err(r) = check_csrf(state, head, &requester, request_id) {
        return r;
    }
    let payload = match need_json(state, body, request_id) {
        Ok(v) => v,
        Err(r) => return r,
    };
    if !payload.is_object() {
        return Response::json(
            400,
            error_json(
                super::response::CODE_INVALID_OPTIONS,
                "Retry body must be an estimate object",
                request_id,
            ),
        )
        .with_policy(state.config.production);
    }
    let key = payload
        .get("idempotency_key")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty());
    if let Some(key) = key {
        if let Err(message) = super::estimate::validate_idempotency_key(key) {
            return Response::json(
                400,
                error_json(super::response::CODE_INVALID_OPTIONS, &message, request_id),
            )
            .with_policy(state.config.production);
        }
    }
    let batch = match state.db.upload_batch_by_id(id) {
        Ok(Some(value)) if value.user_id == requester.user.id => value,
        _ => return not_found(state, request_id),
    };
    let jobs = match state.db.upload_batch_jobs(id) {
        Ok(value) => value,
        Err(_) => {
            return Response::json(
                502,
                error_json(
                    super::response::CODE_ESTIMATION_FAILED,
                    "Could not load upload batch",
                    request_id,
                ),
            )
            .with_policy(state.config.production)
        }
    };
    let Some(job) = jobs
        .iter()
        .find(|job| job.batch_index == Some(index) && job.user_id == requester.user.id)
    else {
        return not_found(state, request_id);
    };
    if !matches!(job.status.as_str(), "failed" | "cancelled" | "expired") {
        return Response::json(
            409,
            error_json(
                super::response::CODE_INVALID_OPTIONS,
                "Only failed or cancelled batch items can be retried",
                request_id,
            ),
        )
        .with_policy(state.config.production);
    }
    if job.idempotency_key.as_deref() != key {
        return Response::json(
            400,
            error_json(
                super::response::CODE_INVALID_OPTIONS,
                "Retry must use the original item idempotency key",
                request_id,
            ),
        )
        .with_policy(state.config.production);
    }
    match state.db.open_job_count(&requester.user.id) {
        Ok(count) if count >= super::jobs::MAX_OPEN_JOBS_PER_USER => {
            return Response::json(
                429,
                error_json(
                    super::response::CODE_RATE_LIMITED,
                    "Too many queued jobs; wait for some to finish",
                    request_id,
                ),
            )
            .with_policy(state.config.production);
        }
        Err(_) => {
            return Response::json(
                502,
                error_json(
                    super::response::CODE_ESTIMATION_FAILED,
                    "Could not check the queue",
                    request_id,
                ),
            )
            .with_policy(state.config.production)
        }
        _ => {}
    }
    match state.db.retry_batch_job(
        &job.id,
        id,
        &requester.user.id,
        index,
        &payload.to_string(),
        &rfc3339(crate::time_util::unix_now()),
    ) {
        Ok(true) => match batch_json(state, &batch) {
            Ok(out) => Response::json(202, with_request_id(out, request_id))
                .with_policy(state.config.production),
            Err(_) => Response::json(
                202,
                with_request_id(json!({"id":id,"status":"processing"}), request_id),
            )
            .with_policy(state.config.production),
        },
        Ok(false) => Response::json(
            409,
            error_json(
                super::response::CODE_INVALID_OPTIONS,
                "That batch item is no longer retryable",
                request_id,
            ),
        )
        .with_policy(state.config.production),
        Err(_) => Response::json(
            502,
            error_json(
                super::response::CODE_ESTIMATION_FAILED,
                "Could not retry that batch item",
                request_id,
            ),
        )
        .with_policy(state.config.production),
    }
}
