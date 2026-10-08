//! Durable estimate-job routes (roadmap item 9).
//!
//! `POST /api/jobs` queues an estimate payload for background processing
//! (HTTP 202); `GET /api/jobs` lists the caller's jobs; `GET
//! /api/jobs/{id}` reports status and, on success, the estimate result;
//! `POST /api/jobs/{id}/cancel` cancels a queued job. All routes are
//! owner-scoped: other users' job ids 404.

use serde_json::{json, Value};

use super::json_util::need_json;
use super::response::{error_json, with_request_id, Response};
use super::server::RequestHead;
use super::session::{check_csrf, require_auth};
use super::ServerState;
use crate::auth::new_id;
use crate::time_util::{rfc3339, unix_now};

/// Max open (queued + active) jobs per user: bounds queued work.
pub(crate) const MAX_OPEN_JOBS_PER_USER: i64 = 20;

fn job_json(job: &crate::db::Job) -> Value {
    let result: Value = job
        .result
        .as_deref()
        .and_then(|text| serde_json::from_str(text).ok())
        .unwrap_or(Value::Null);
    json!({
        "id": job.id,
        "status": job.status,
        "attempts": job.attempts,
        "created_at": job.created_at,
        "updated_at": job.updated_at,
        "error_code": job.error_code,
        "error_message": job.error_message,
        "history_id": job.history_id,
        "batch_id": job.batch_id,
        "batch_index": job.batch_index,
        "filename": job.filename,
        "result": result,
    })
}

fn not_found(state: &ServerState, request_id: &str) -> Response {
    Response::json(
        404,
        error_json(super::response::CODE_NOT_FOUND, "Not found", request_id),
    )
    .with_policy(state.config.production)
}

fn owned_job(
    state: &ServerState,
    user_id: &str,
    id: &str,
    request_id: &str,
) -> Result<crate::db::Job, Response> {
    if id.len() > 64 {
        return Err(not_found(state, request_id));
    }
    match state.db.job_by_id(id) {
        Ok(Some(job)) if job.user_id == user_id => Ok(job),
        _ => Err(not_found(state, request_id)),
    }
}

/// `POST /api/jobs` — queue an estimate payload. Answers 202 with the job;
/// duplicate idempotency keys return the existing job instead of queueing.
pub(crate) fn handle_create(
    body: &[u8],
    request_id: &str,
    state: &ServerState,
    head: &RequestHead,
    peer_ip: &str,
) -> Response {
    let requester = match require_auth(state, head, request_id, peer_ip) {
        Ok(r) => r,
        Err(r) => return r,
    };
    if let Err(r) = check_csrf(state, head, &requester, request_id) {
        return r;
    }
    if !state.config.jobs_enabled {
        return Response::json(
            400,
            error_json(
                super::response::CODE_INVALID_OPTIONS,
                "Background jobs are disabled on this server",
                request_id,
            ),
        )
        .with_policy(state.config.production);
    }
    let mut payload = match need_json(state, body, request_id) {
        Ok(p) => p,
        Err(r) => return r,
    };
    if !payload.is_object() {
        return Response::json(
            400,
            error_json(
                super::response::CODE_INVALID_OPTIONS,
                "Job body must be a JSON estimate object",
                request_id,
            ),
        )
        .with_policy(state.config.production);
    }
    let key = payload
        .get("idempotency_key")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty());
    let key = key.map(str::to_string);
    if let Some(k) = key.as_deref() {
        if let Err(message) = super::estimate::validate_idempotency_key(k) {
            return Response::json(
                400,
                error_json(super::response::CODE_INVALID_OPTIONS, &message, request_id),
            )
            .with_policy(state.config.production);
        }
    }
    let batch_id = payload
        .get("batch_id")
        .and_then(Value::as_str)
        .map(str::to_string);
    let batch_index = payload.get("batch_index").and_then(Value::as_i64);
    let filename = payload
        .get("filename")
        .and_then(Value::as_str)
        .map(str::to_string);
    if batch_id.is_some() != batch_index.is_some() || batch_id.is_some() != filename.is_some() {
        return Response::json(
            400,
            error_json(
                super::response::CODE_INVALID_OPTIONS,
                "batch_id, batch_index, and filename must be supplied together",
                request_id,
            ),
        )
        .with_policy(state.config.production);
    }
    let filename = if let Some(name) = filename {
        let base = name.rsplit(['/', '\\']).next().unwrap_or("").trim();
        if base.is_empty() || base.chars().count() > 128 || base.chars().any(char::is_control) {
            return Response::json(
                400,
                error_json(
                    super::response::CODE_INVALID_OPTIONS,
                    "Invalid batch filename",
                    request_id,
                ),
            )
            .with_policy(state.config.production);
        }
        Some(base.to_string())
    } else {
        None
    };
    if let Some(key) = key.as_deref() {
        match state.db.job_by_idempotency_key(&requester.user.id, key) {
            Ok(Some(job)) => {
                let same_item = match (batch_id.as_deref(), batch_index, filename.as_deref()) {
                    (Some(batch), Some(index), Some(name)) => {
                        job.batch_id.as_deref() == Some(batch)
                            && job.batch_index == Some(index)
                            && job.filename.as_deref() == Some(name)
                    }
                    (None, None, None) => job.batch_id.is_none() && job.batch_index.is_none(),
                    _ => false,
                };
                if !same_item {
                    return Response::json(
                        400,
                        error_json(
                            super::response::CODE_INVALID_OPTIONS,
                            "Idempotency key is already used for another job item",
                            request_id,
                        ),
                    )
                    .with_policy(state.config.production);
                }
                let mut out = job_json(&job);
                out["replayed"] = Value::from(true);
                return Response::json(202, with_request_id(out, request_id))
                    .with_policy(state.config.production);
            }
            Ok(None) => {}
            Err(_) => {
                return Response::json(
                    502,
                    error_json(
                        super::response::CODE_ESTIMATION_FAILED,
                        "Could not check the job idempotency key",
                        request_id,
                    ),
                )
                .with_policy(state.config.production);
            }
        }
    }
    match state.db.open_job_count(&requester.user.id) {
        Ok(n) if n >= MAX_OPEN_JOBS_PER_USER => {
            return Response::json(
                429,
                error_json(
                    super::response::CODE_RATE_LIMITED,
                    "Too many queued jobs; wait for some to finish",
                    request_id,
                ),
            )
            .with_policy(state.config.production)
        }
        Err(_) => {
            return Response::json(
                502,
                error_json(
                    super::response::CODE_ESTIMATION_FAILED,
                    "Could not queue the job",
                    request_id,
                ),
            )
            .with_policy(state.config.production)
        }
        _ => {}
    }
    let now = rfc3339(unix_now());
    let object = payload.as_object_mut().expect("object checked above");
    object.remove("batch_id");
    object.remove("batch_index");
    object.remove("filename");
    let payload_text = payload.to_string();
    let created = match (batch_id.as_deref(), batch_index, filename.as_deref()) {
        (Some(batch), Some(index), Some(name)) => state.db.create_batch_job(
            &new_id(),
            &requester.user.id,
            crate::db::BatchJobMetadata {
                batch_id: batch,
                batch_index: index,
                filename: name,
            },
            key.as_deref(),
            &payload_text,
            &now,
        ),
        (None, None, None) => state.db.create_job(
            &new_id(),
            &requester.user.id,
            key.as_deref(),
            &payload_text,
            &now,
        ),
        _ => unreachable!("batch metadata checked above"),
    };
    match created {
        Ok((id, replayed)) => {
            state.audit(
                Some(&requester.user.id),
                "job_created",
                &format!("job={}", id),
                request_id,
            );
            let mut out = match state.db.job_by_id(&id) {
                Ok(Some(job)) => job_json(&job),
                _ => json!({"id": id, "status": "queued"}),
            };
            out["replayed"] = Value::from(replayed);
            Response::json(202, with_request_id(out, request_id))
                .with_policy(state.config.production)
        }
        Err(error) => Response::json(
            if error.contains("not found")
                || error.contains("another item")
                || error.contains("already submitted")
                || error.contains("out of range")
            {
                400
            } else {
                502
            },
            error_json(
                if error.contains("not found")
                    || error.contains("another item")
                    || error.contains("already submitted")
                    || error.contains("out of range")
                {
                    super::response::CODE_INVALID_OPTIONS
                } else {
                    super::response::CODE_ESTIMATION_FAILED
                },
                if error.contains("not found") {
                    "Upload batch is unavailable"
                } else {
                    &error
                },
                request_id,
            ),
        )
        .with_policy(state.config.production),
    }
}

/// `GET /api/jobs` — list the caller's jobs, newest first.
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
    match state.db.list_jobs(&requester.user.id, 50) {
        Ok(jobs) => {
            let items: Vec<Value> = jobs.iter().map(job_json).collect();
            Response::json(200, with_request_id(json!({"jobs": items}), request_id))
                .with_policy(state.config.production)
        }
        Err(_) => Response::json(
            502,
            error_json(
                super::response::CODE_ESTIMATION_FAILED,
                "Could not load jobs",
                request_id,
            ),
        )
        .with_policy(state.config.production),
    }
}

/// `GET /api/jobs/{id}` — one owned job with its result when successful.
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
    match owned_job(state, &requester.user.id, id, request_id) {
        Ok(job) => Response::json(200, with_request_id(job_json(&job), request_id))
            .with_policy(state.config.production),
        Err(r) => r,
    }
}

/// `POST /api/jobs/{id}/cancel` — cancel a queued job. Active jobs run to
/// completion (documented); terminal jobs answer 400.
pub(crate) fn handle_cancel(
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
    match owned_job(state, &requester.user.id, id, request_id) {
        Ok(job) => {
            if job.status != "queued" {
                return Response::json(
                    400,
                    error_json(
                        super::response::CODE_INVALID_OPTIONS,
                        "Only queued jobs can be cancelled",
                        request_id,
                    ),
                )
                .with_policy(state.config.production);
            }
            match state.db.cancel_job(&job.id, &rfc3339(unix_now())) {
                Ok(true) => {
                    state.audit(
                        Some(&requester.user.id),
                        "job_cancelled",
                        &format!("job={}", job.id),
                        request_id,
                    );
                    Response::json(200, with_request_id(json!({"ok": true}), request_id))
                        .with_policy(state.config.production)
                }
                _ => Response::json(
                    400,
                    error_json(
                        super::response::CODE_INVALID_OPTIONS,
                        "Only queued jobs can be cancelled",
                        request_id,
                    ),
                )
                .with_policy(state.config.production),
            }
        }
        Err(r) => r,
    }
}
