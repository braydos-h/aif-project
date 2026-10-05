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
    let payload = match need_json(state, body, request_id) {
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
    if let Some(k) = key {
        if let Err(message) = super::estimate::validate_idempotency_key(k) {
            return Response::json(
                400,
                error_json(super::response::CODE_INVALID_OPTIONS, &message, request_id),
            )
            .with_policy(state.config.production);
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
    match state.db.create_job(
        &new_id(),
        &requester.user.id,
        key,
        &payload.to_string(),
        &now,
    ) {
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
        Err(_) => Response::json(
            502,
            error_json(
                super::response::CODE_ESTIMATION_FAILED,
                "Could not queue the job",
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
