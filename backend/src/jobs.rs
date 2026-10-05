//! Durable estimate jobs (roadmap item 9): queue, process, recover.
//!
//! A logged-in caller can submit an estimate payload as a background job
//! (`POST /api/jobs`) and poll its status (`GET /api/jobs/{id}`) instead of
//! holding a mobile connection open. One worker thread per process drains
//! the queue in FIFO order through the same [`estimate_one`] pipeline as
//! foreground estimates, so quotas, pause, idempotency, and history rules
//! are identical.
//!
//! Lifecycle: `queued` → `active` → `success` | `failed` | `cancelled`,
//! plus `expired` for stale queued jobs. Transient failures (429/502/503)
//! requeue with bounded backoff (at most [`MAX_JOB_ATTEMPTS`] tries);
//! client errors fail permanently. A restart requeues `active` jobs back
//! to `queued`; because history saves are idempotency-keyed, a rerun
//! replays the stored row instead of duplicating it — each completed
//! result is saved exactly once.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;

use crate::db::MAX_JOB_ATTEMPTS;
use crate::http::ServerState;
use crate::time_util::{rfc3339, unix_now};

/// Maintenance every N idle worker iterations (prune/expire/photo sweep).
const MAINTENANCE_EVERY_IDLE: u32 = 60;
/// Stale queued jobs expire after 24 h without running.
const QUEUED_EXPIRY_SECS: u64 = 24 * 3600;

/// Start the background worker thread (process lifetime). No-op when jobs
/// or the worker are disabled in config.
pub fn spawn_worker(state: Arc<ServerState>) {
    if !state.config.jobs_enabled || !state.config.job_worker {
        return;
    }
    // Crash recovery first: jobs left `active` at shutdown run again.
    let now = rfc3339(unix_now());
    match state.db.requeue_active_jobs(&now) {
        Ok(n) if n > 0 => eprintln!("jobs: requeued {} interrupted job(s)", n),
        Ok(_) => {}
        Err(e) => eprintln!("jobs: requeue failed: {}", e),
    }
    std::thread::spawn(move || {
        let mut idle = 0u32;
        loop {
            if process_one(&state) {
                idle = 0;
                continue;
            }
            idle += 1;
            if idle % MAINTENANCE_EVERY_IDLE == 0 {
                run_maintenance(&state);
            }
            std::thread::sleep(Duration::from_millis(500));
        }
    });
}

/// Boot-time recovery + periodic maintenance: prune old terminal jobs,
/// expire stale queued jobs, and sweep retained photos. Best-effort.
pub fn run_maintenance(state: &ServerState) {
    let now_secs = unix_now();
    let now = rfc3339(now_secs);
    let prune_before = rfc3339(now_secs.saturating_sub(state.config.job_ttl_hours * 3600));
    let expire_before = rfc3339(now_secs.saturating_sub(QUEUED_EXPIRY_SECS));
    let _ = state.db.prune_jobs(&prune_before);
    let _ = state.db.expire_stale_queued(&expire_before, &now);
    let _ = crate::photos::sweep_photos(&state.db, &state.config.data_dir, &now);
}

/// Claim one due job and run it to a terminal (or requeued) state.
/// Returns true when a job was picked up.
pub(crate) fn process_one(state: &ServerState) -> bool {
    if !state.config.jobs_enabled || state.inference_paused() {
        return false;
    }
    let now = rfc3339(unix_now());
    let job = match state.db.claim_next_job(&now) {
        Ok(Some(job)) => job,
        _ => return false,
    };
    // The owner must still exist and be active; otherwise fail closed.
    let active = state
        .db
        .user_by_id(&job.user_id)
        .ok()
        .flatten()
        .is_some_and(|u| u.status == "active");
    if !active {
        let _ = state.db.finish_job(
            &job.id,
            "failed",
            &rfc3339(unix_now()),
            None,
            None,
            Some("forbidden"),
            Some("account is no longer active"),
            None,
        );
        return true;
    }
    let payload: Value = match serde_json::from_str(&job.payload) {
        Ok(Value::Object(map)) => Value::Object(map),
        _ => Value::Null,
    };
    if !payload.is_object() {
        let _ = state.db.finish_job(
            &job.id,
            "failed",
            &rfc3339(unix_now()),
            None,
            None,
            Some("invalid_options"),
            Some("job payload must be a JSON estimate object"),
            None,
        );
        return true;
    }
    let head = crate::http::server::RequestHead {
        method: "POST".to_string(),
        path: "/estimate-weight".to_string(),
        query: None,
        content_length: 0,
        has_chunked_body: false,
        cookie: None,
        csrf_token: None,
        forwarded_for: None,
        idempotency_key: None,
    };
    let user = state.db.user_by_id(&job.user_id).ok().flatten().unwrap();
    let requester = crate::http::session::Requester {
        user,
        csrf_token: String::new(),
    };
    let request_id = crate::http::request_id::new_request_id();
    let (status, mut result) =
        crate::http::estimate::estimate_one(&payload, &request_id, state, &head, &Some(requester));
    let finished = rfc3339(unix_now());
    if status == 200 {
        result["job_id"] = Value::from(job.id.clone());
        let history_id = result
            .get("history_id")
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let body = result.to_string();
        let _ = state.db.finish_job(
            &job.id,
            "success",
            &finished,
            None,
            Some(&body),
            None,
            None,
            history_id.as_deref(),
        );
        return true;
    }
    let code = result
        .get("code")
        .and_then(|v| v.as_str())
        .unwrap_or("estimation_failed")
        .to_string();
    let transient = matches!(status, 429 | 502 | 503);
    if transient && job.attempts + 1 < MAX_JOB_ATTEMPTS {
        // Bounded backoff: 1, 2, 4 … minutes (capped at 1 h).
        let delay = (60u64 << job.attempts.min(6)).min(3600);
        let run_after = rfc3339(unix_now() + delay);
        let _ = state.db.finish_job(
            &job.id,
            "queued",
            &finished,
            Some(&run_after),
            None,
            None,
            None,
            None,
        );
    } else {
        let mut message = result
            .get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("estimate failed")
            .to_string();
        message.truncate(500);
        let _ = state.db.finish_job(
            &job.id,
            "failed",
            &finished,
            None,
            None,
            Some(&code),
            Some(&message),
            None,
        );
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_state(name: &str) -> ServerState {
        let dir = std::env::temp_dir().join(format!("aif-jobs-{}-{}", std::process::id(), name));
        let _ = std::fs::remove_dir_all(&dir);
        let config = crate::config::Config {
            backend: "none".to_string(),
            ollama_url: crate::config::DEFAULT_OLLAMA_URL.to_string(),
            ollama_api_key: None,
            model: crate::config::DEFAULT_OLLAMA_MODEL.to_string(),
            cache_ttl: 0,
            require_auth: true,
            production: false,
            data_dir: dir.to_str().unwrap().to_string(),
            public_origin: "http://127.0.0.1:8080".to_string(),
            cookie_secure: false,
            session_days: 30,
            invite_days: 7,
            daily_estimate_limit: 200,
            max_concurrent_inference: 4,
            inference_paused: false,
            trusted_proxies: vec!["127.0.0.1".to_string()],
            operator_email: None,
            retain_photos: false,
            photo_ttl_days: 30,
            photo_quota_mb: 50,
            jobs_enabled: true,
            job_ttl_hours: 72,
            job_worker: false,
            disk_alert_mb: 1024,
        };
        ServerState::new(config).unwrap()
    }

    #[test]
    fn worker_completes_tape_job_and_saves_once() {
        let state = test_state("tape");
        state
            .db
            .create_user("u1", "a@example.com", "A", "h", "user")
            .unwrap();
        let now = rfc3339(unix_now());
        let (id, _) = state
            .db
            .create_job(
                "j1",
                "u1",
                Some("k1"),
                r#"{"heart_girth_cm": 180, "body_length_cm": 150}"#,
                &now,
            )
            .unwrap();
        assert!(process_one(&state));
        let job = state.db.job_by_id(&id).unwrap().unwrap();
        assert_eq!(job.status, "success");
        let result: Value = serde_json::from_str(job.result.as_deref().unwrap()).unwrap();
        assert_eq!(result["estimated_weight_kg"], 448.4);
        assert_eq!(state.db.estimate_count("u1").unwrap(), 1);
        // Nothing left to do.
        assert!(!process_one(&state));
    }

    #[test]
    fn worker_fails_bad_payload_permanently() {
        let state = test_state("bad");
        state
            .db
            .create_user("u1", "a@example.com", "A", "h", "user")
            .unwrap();
        let now = rfc3339(unix_now());
        state
            .db
            .create_job("j1", "u1", None, r#"{"heart_girth_cm": 180}"#, &now)
            .unwrap();
        assert!(process_one(&state));
        let job = state.db.job_by_id("j1").unwrap().unwrap();
        assert_eq!(job.status, "failed");
        assert_eq!(job.error_code.as_deref(), Some("invalid_options"));
        assert_eq!(job.attempts, 1);
    }
}
