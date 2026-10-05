//! Durable SQLite storage for the two-user service.
//!
//! Holds users, invites, sessions, animals, estimate history, idempotency
//! keys, per-day usage counters, and a redacted audit log. One file
//! (`<AIF_DATA_DIR>/aif.db`), WAL mode, foreign keys on, versioned
//! migrations in [`MIGRATIONS`] so restarts and upgrades preserve data.
//!
//! Raw photos are never stored here (transient-only photo policy); only
//! estimate metadata is persisted.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::{params, Connection, OptionalExtension};

use crate::time_util::{rfc3339, unix_now};

/// Current schema version (length of [`MIGRATIONS`]).
pub const SCHEMA_VERSION: u32 = 2;

/// Versioned migrations, applied in order inside one transaction each.
/// New migrations must only add tables/columns/indexes — never drop or
/// reinterpret existing columns — so upgrades preserve existing data.
const MIGRATIONS: &[&str] = &[
    r#"
CREATE TABLE IF NOT EXISTS users (
    id TEXT PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL DEFAULT '',
    password_hash TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'user',
    status TEXT NOT NULL DEFAULT 'active',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    last_login_at TEXT
);
CREATE TABLE IF NOT EXISTS invites (
    id TEXT PRIMARY KEY,
    email TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'user',
    token_hash TEXT NOT NULL UNIQUE,
    created_by TEXT,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    used_at TEXT,
    used_by TEXT,
    revoked_at TEXT,
    invite_url_hint TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS sessions (
    token_hash TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    csrf_token TEXT NOT NULL,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL,
    revoked_at TEXT,
    rotated_to TEXT
);
CREATE TABLE IF NOT EXISTS recovery_tokens (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    used_at TEXT
);
CREATE TABLE IF NOT EXISTS animals (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    breed TEXT,
    sex TEXT,
    birth_year INTEGER,
    notes TEXT,
    archived INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_animals_user ON animals(user_id);
CREATE TABLE IF NOT EXISTS estimates (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    animal_id TEXT REFERENCES animals(id) ON DELETE SET NULL,
    created_at TEXT NOT NULL,
    measured_at TEXT,
    weight_kg REAL NOT NULL,
    weight_min_kg REAL NOT NULL,
    weight_max_kg REAL NOT NULL,
    source TEXT NOT NULL,
    method TEXT NOT NULL,
    model TEXT,
    provider TEXT,
    estimator_version TEXT NOT NULL DEFAULT '',
    prompt_version TEXT NOT NULL DEFAULT '',
    prompt_used TEXT NOT NULL DEFAULT '',
    heart_girth_cm REAL,
    body_length_cm REAL,
    confidence REAL,
    breed TEXT,
    body_condition_score REAL,
    animal_breed TEXT,
    animal_sex TEXT,
    animal_age_years REAL,
    scale_weight_kg REAL,
    placeholder INTEGER NOT NULL DEFAULT 0,
    disclaimer TEXT NOT NULL DEFAULT '',
    idempotency_key TEXT,
    request_id TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_estimates_user_created ON estimates(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_estimates_animal ON estimates(animal_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_estimates_idem ON estimates(user_id, idempotency_key);
CREATE TABLE IF NOT EXISTS usage_counters (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    day TEXT NOT NULL,
    image_estimates INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, day)
);
CREATE TABLE IF NOT EXISTS audit_log (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    ts TEXT NOT NULL,
    actor_user_id TEXT,
    action TEXT NOT NULL,
    detail TEXT NOT NULL DEFAULT '',
    request_id TEXT NOT NULL DEFAULT ''
);
"#,
    // v2: optional private photo retention + durable estimate jobs.
    r#"
CREATE TABLE IF NOT EXISTS photos (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    estimate_id TEXT REFERENCES estimates(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    expires_at TEXT NOT NULL,
    bytes INTEGER NOT NULL,
    mime TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_photos_user ON photos(user_id);
CREATE INDEX IF NOT EXISTS idx_photos_estimate ON photos(estimate_id);
CREATE TABLE IF NOT EXISTS jobs (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    run_after TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued',
    attempts INTEGER NOT NULL DEFAULT 0,
    idempotency_key TEXT,
    payload TEXT NOT NULL,
    result TEXT,
    error_code TEXT,
    error_message TEXT,
    history_id TEXT
);
CREATE INDEX IF NOT EXISTS idx_jobs_user_status ON jobs(user_id, status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_jobs_claim ON jobs(status, run_after, created_at);
CREATE UNIQUE INDEX IF NOT EXISTS idx_jobs_idem ON jobs(user_id, idempotency_key);
"#,
];

/// Maximum active (non-deleted) users the service supports.
pub const MAX_ACTIVE_USERS: i64 = 2;

/// Thread-safe handle: one mutex-guarded connection, shared across the
/// threaded HTTP server. All multi-step writes use explicit transactions
/// so a failure never leaves partial records.
pub struct Db {
    conn: Mutex<Connection>,
    pub path: PathBuf,
}

impl Db {
    /// Open (creating parent dirs) and migrate.
    pub fn open(data_dir: &str) -> Result<Db, String> {
        let dir = Path::new(data_dir);
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("cannot create data dir {}: {}", dir.display(), e))?;
        let path = dir.join("aif.db");
        let conn = Connection::open(&path)
            .map_err(|e| format!("cannot open database {}: {}", path.display(), e))?;
        let db = Db {
            conn: Mutex::new(conn),
            path: path.clone(),
        };
        db.with_conn(|conn| {
            conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")
                .map_err(|e| format!("pragma failed: {}", e))?;
            conn.execute_batch(
                "CREATE TABLE IF NOT EXISTS schema_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL)",
            )
            .map_err(|e| format!("migrations table failed: {}", e))?;
            let applied: u32 = conn
                .query_row("SELECT COALESCE(MAX(version), 0) FROM schema_migrations", [], |r| {
                    r.get(0)
                })
                .map_err(|e| format!("migration query failed: {}", e))?;
            for (index, sql) in MIGRATIONS.iter().enumerate() {
                let version = index as u32 + 1;
                if version <= applied {
                    continue;
                }
                let tx = conn
                    .transaction()
                    .map_err(|e| format!("migration tx failed: {}", e))?;
                tx.execute_batch(sql)
                    .map_err(|e| format!("migration {} failed: {}", version, e))?;
                tx.execute(
                    "INSERT INTO schema_migrations (version, applied_at) VALUES (?, ?)",
                    params![version, rfc3339(unix_now())],
                )
                .map_err(|e| format!("migration {} record failed: {}", version, e))?;
                tx.commit()
                    .map_err(|e| format!("migration {} commit failed: {}", version, e))?;
            }
            Ok(())
        })?;
        Ok(db)
    }

    /// Open an isolated throwaway database (tests).
    pub fn open_temp(name: &str) -> Result<Db, String> {
        let dir = std::env::temp_dir().join(format!(
            "aif-db-test-{}-{}",
            std::process::id(),
            name.replace('/', "_")
        ));
        Db::open(dir.to_str().unwrap_or("/tmp/aif-db-test"))
    }

    fn with_conn<T>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<T, String>,
    ) -> Result<T, String> {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        f(&mut guard)
    }

    /// Current schema version recorded in the database.
    pub fn schema_version(&self) -> Result<u32, String> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())
        })
    }

    /// Append a redacted audit entry. Never call with secrets, tokens,
    /// prompts, or image data in `detail`.
    pub fn audit(
        &self,
        actor_user_id: Option<&str>,
        action: &str,
        detail: &str,
        request_id: &str,
    ) -> Result<(), String> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO audit_log (ts, actor_user_id, action, detail, request_id) VALUES (?, ?, ?, ?, ?)",
                params![rfc3339(unix_now()), actor_user_id, action, detail, request_id],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        })
    }

    /// Recent audit entries, newest first (operator diagnostics; capped).
    pub fn recent_audit(&self, limit: i64) -> Result<Vec<AuditEntry>, String> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT ts, actor_user_id, action, detail, request_id FROM audit_log ORDER BY id DESC LIMIT ?",
                )
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![limit.clamp(1, 500)], |r| {
                    Ok(AuditEntry {
                        ts: r.get(0)?,
                        actor_user_id: r.get(1)?,
                        action: r.get(2)?,
                        detail: r.get(3)?,
                        request_id: r.get(4)?,
                    })
                })
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
        })
    }

    /// Number of active (non-deleted) users.
    pub fn active_user_count(&self) -> Result<i64, String> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM users WHERE status = 'active'",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())
        })
    }
}

/// Redacted audit entry (no secrets by construction).
#[derive(Debug, Clone)]
pub struct AuditEntry {
    pub ts: String,
    pub actor_user_id: Option<String>,
    pub action: String,
    pub detail: String,
    pub request_id: String,
}

/// Public user record (never includes the password hash).
#[derive(Debug, Clone)]
pub struct User {
    pub id: String,
    pub email: String,
    pub display_name: String,
    pub role: String,
    pub status: String,
    pub created_at: String,
    pub last_login_at: Option<String>,
}

impl Db {
    /// Insert a user. Enforces the two-user cap for active accounts.
    pub fn create_user(
        &self,
        id: &str,
        email: &str,
        display_name: &str,
        password_hash: &str,
        role: &str,
    ) -> Result<User, String> {
        self.with_conn(|conn| {
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            let active: i64 = tx
                .query_row("SELECT COUNT(*) FROM users WHERE status = 'active'", [], |r| {
                    r.get(0)
                })
                .map_err(|e| e.to_string())?;
            if active >= MAX_ACTIVE_USERS {
                return Err("user_limit: the service is limited to two active accounts".to_string());
            }
            let now = rfc3339(unix_now());
            let rows = tx
                .execute(
                    "INSERT INTO users (id, email, display_name, password_hash, role, status, created_at, updated_at) VALUES (?, ?, ?, ?, ?, 'active', ?, ?)",
                    params![id, email, display_name, password_hash, role, now, now],
                )
                .map_err(|e| {
                    if e.to_string().contains("UNIQUE") {
                        "email_taken: an account with that email already exists".to_string()
                    } else {
                        e.to_string()
                    }
                })?;
            if rows != 1 {
                return Err("user insert failed".to_string());
            }
            tx.commit().map_err(|e| e.to_string())?;
            Ok(User {
                id: id.to_string(),
                email: email.to_string(),
                display_name: display_name.to_string(),
                role: role.to_string(),
                status: "active".to_string(),
                created_at: now.clone(),
                last_login_at: None,
            })
        })
    }

    /// Fetch a user by normalized email (includes hash, for login only).
    pub fn user_by_email_with_hash(&self, email: &str) -> Result<Option<(User, String)>, String> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, email, display_name, password_hash, role, status, created_at, last_login_at FROM users WHERE email = ?",
                params![email],
                |r| {
                    Ok((
                        User {
                            id: r.get(0)?,
                            email: r.get(1)?,
                            display_name: r.get(2)?,
                            role: r.get(4)?,
                            status: r.get(5)?,
                            created_at: r.get(6)?,
                            last_login_at: r.get(7)?,
                        },
                        r.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| e.to_string())
        })
    }

    /// Fetch a user by id (public fields).
    pub fn user_by_id(&self, id: &str) -> Result<Option<User>, String> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, email, display_name, role, status, created_at, last_login_at FROM users WHERE id = ?",
                params![id],
                |r| {
                    Ok(User {
                        id: r.get(0)?,
                        email: r.get(1)?,
                        display_name: r.get(2)?,
                        role: r.get(3)?,
                        status: r.get(4)?,
                        created_at: r.get(5)?,
                        last_login_at: r.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(|e| e.to_string())
        })
    }

    /// List all users (operator view; no hashes).
    pub fn list_users(&self) -> Result<Vec<User>, String> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT id, email, display_name, role, status, created_at, last_login_at FROM users ORDER BY created_at")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(User {
                        id: r.get(0)?,
                        email: r.get(1)?,
                        display_name: r.get(2)?,
                        role: r.get(3)?,
                        status: r.get(4)?,
                        created_at: r.get(5)?,
                        last_login_at: r.get(6)?,
                    })
                })
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
        })
    }

    /// Record a login timestamp.
    pub fn record_login(&self, user_id: &str) -> Result<(), String> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE users SET last_login_at = ?, updated_at = ? WHERE id = ?",
                params![rfc3339(unix_now()), rfc3339(unix_now()), user_id],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        })
    }

    /// Change a password hash.
    pub fn set_password(&self, user_id: &str, new_hash: &str) -> Result<(), String> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE users SET password_hash = ?, updated_at = ? WHERE id = ?",
                params![new_hash, rfc3339(unix_now()), user_id],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        })
    }

    /// Update display name (validated by caller).
    pub fn set_display_name(&self, user_id: &str, name: &str) -> Result<(), String> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE users SET display_name = ?, updated_at = ? WHERE id = ?",
                params![name, rfc3339(unix_now()), user_id],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        })
    }

    /// Soft-delete an account: mark deleted and revoke all its sessions in
    /// one transaction so no active session survives deletion.
    pub fn delete_account(&self, user_id: &str) -> Result<(i64, i64), String> {
        self.with_conn(|conn| {
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            let now = rfc3339(unix_now());
            let users = tx
                .execute(
                    "UPDATE users SET status = 'deleted', updated_at = ? WHERE id = ? AND status = 'active'",
                    params![now, user_id],
                )
                .map_err(|e| e.to_string())?;
            let sessions = tx
                .execute(
                    "UPDATE sessions SET revoked_at = ? WHERE user_id = ? AND revoked_at IS NULL",
                    params![now, user_id],
                )
                .map_err(|e| e.to_string())?;
            tx.execute("DELETE FROM estimates WHERE user_id = ?", params![user_id])
                .map_err(|e| e.to_string())?;
            tx.execute("DELETE FROM animals WHERE user_id = ?", params![user_id])
                .map_err(|e| e.to_string())?;
            tx.execute(
                "DELETE FROM usage_counters WHERE user_id = ?",
                params![user_id],
            )
            .map_err(|e| e.to_string())?;
            tx.execute(
                "DELETE FROM recovery_tokens WHERE user_id = ?",
                params![user_id],
            )
            .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            Ok((users as i64, sessions as i64))
        })
    }
}

/// Invite record (token hashes only; raw tokens never touch the database).
#[derive(Debug, Clone)]
pub struct Invite {
    pub id: String,
    pub email: String,
    pub role: String,
    pub created_at: String,
    pub expires_at: String,
    pub used_at: Option<String>,
    pub revoked_at: Option<String>,
}

impl Db {
    /// Create an invite with a pre-hashed token.
    pub fn create_invite(
        &self,
        id: &str,
        email: &str,
        role: &str,
        token_hash: &str,
        created_by: Option<&str>,
        expires_at: &str,
    ) -> Result<Invite, String> {
        self.with_conn(|conn| {
            let now = rfc3339(unix_now());
            conn.execute(
                "INSERT INTO invites (id, email, role, token_hash, created_by, created_at, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
                params![id, email, role, token_hash, created_by, now, expires_at],
            )
            .map_err(|e| e.to_string())?;
            Ok(Invite {
                id: id.to_string(),
                email: email.to_string(),
                role: role.to_string(),
                created_at: now,
                expires_at: expires_at.to_string(),
                used_at: None,
                revoked_at: None,
            })
        })
    }

    /// Look up an invite by token hash.
    pub fn invite_by_token_hash(&self, token_hash: &str) -> Result<Option<Invite>, String> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, email, role, created_at, expires_at, used_at, revoked_at FROM invites WHERE token_hash = ?",
                params![token_hash],
                |r| {
                    Ok(Invite {
                        id: r.get(0)?,
                        email: r.get(1)?,
                        role: r.get(2)?,
                        created_at: r.get(3)?,
                        expires_at: r.get(4)?,
                        used_at: r.get(5)?,
                        revoked_at: r.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(|e| e.to_string())
        })
    }

    /// Mark an invite used (single-use) and bind it to the new user.
    pub fn mark_invite_used(&self, invite_id: &str, user_id: &str) -> Result<(), String> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE invites SET used_at = ?, used_by = ? WHERE id = ?",
                params![rfc3339(unix_now()), user_id, invite_id],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        })
    }

    /// Revoke an invite (unused invites only take effect; used ones stay used).
    pub fn revoke_invite(&self, invite_id: &str) -> Result<bool, String> {
        self.with_conn(|conn| {
            let rows = conn
                .execute(
                    "UPDATE invites SET revoked_at = ? WHERE id = ? AND revoked_at IS NULL AND used_at IS NULL",
                    params![rfc3339(unix_now()), invite_id],
                )
                .map_err(|e| e.to_string())?;
            Ok(rows == 1)
        })
    }

    /// Operator invite list (no token material).
    pub fn list_invites(&self) -> Result<Vec<Invite>, String> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT id, email, role, created_at, expires_at, used_at, revoked_at FROM invites ORDER BY created_at DESC")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| {
                    Ok(Invite {
                        id: r.get(0)?,
                        email: r.get(1)?,
                        role: r.get(2)?,
                        created_at: r.get(3)?,
                        expires_at: r.get(4)?,
                        used_at: r.get(5)?,
                        revoked_at: r.get(6)?,
                    })
                })
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
        })
    }
}

/// Session record (token hashes only).
#[derive(Debug, Clone)]
pub struct Session {
    pub user_id: String,
    pub csrf_token: String,
    pub expires_at: String,
}

impl Db {
    /// Create a session for a user.
    pub fn create_session(
        &self,
        token_hash: &str,
        user_id: &str,
        csrf_token: &str,
        expires_at: &str,
    ) -> Result<(), String> {
        self.with_conn(|conn| {
            let now = rfc3339(unix_now());
            conn.execute(
                "INSERT INTO sessions (token_hash, user_id, csrf_token, created_at, expires_at, last_seen_at) VALUES (?, ?, ?, ?, ?, ?)",
                params![token_hash, user_id, csrf_token, now, expires_at, now],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        })
    }

    /// Validate a session token hash: live, unexpired, user still active.
    /// Refreshes `last_seen_at` on success.
    pub fn check_session(&self, token_hash: &str, now: &str) -> Result<Option<Session>, String> {
        self.with_conn(|conn| {
            let row: Option<(String, String, String, String)> = conn
                .query_row(
                    "SELECT s.user_id, s.csrf_token, s.expires_at, u.status FROM sessions s JOIN users u ON u.id = s.user_id WHERE s.token_hash = ? AND s.revoked_at IS NULL",
                    params![token_hash],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()
                .map_err(|e| e.to_string())?;
            match row {
                Some((user_id, csrf_token, expires_at, status))
                    if expires_at.as_str() > now && status == "active" =>
                {
                    conn.execute(
                        "UPDATE sessions SET last_seen_at = ? WHERE token_hash = ?",
                        params![now, token_hash],
                    )
                    .map_err(|e| e.to_string())?;
                    Ok(Some(Session {
                        user_id,
                        csrf_token,
                        expires_at,
                    }))
                }
                _ => Ok(None),
            }
        })
    }

    /// Revoke one session (logout).
    pub fn revoke_session(&self, token_hash: &str) -> Result<(), String> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE sessions SET revoked_at = ? WHERE token_hash = ?",
                params![rfc3339(unix_now()), token_hash],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        })
    }

    /// Revoke all sessions for a user (password change, recovery, deletion).
    /// Returns the number of sessions revoked.
    pub fn revoke_all_sessions(&self, user_id: &str) -> Result<i64, String> {
        self.with_conn(|conn| {
            let rows = conn
                .execute(
                    "UPDATE sessions SET revoked_at = ? WHERE user_id = ? AND revoked_at IS NULL",
                    params![rfc3339(unix_now()), user_id],
                )
                .map_err(|e| e.to_string())?;
            Ok(rows as i64)
        })
    }

    /// Rotate a session: revoke the old token, record its replacement.
    pub fn rotate_session(
        &self,
        old_hash: &str,
        new_hash: &str,
        user_id: &str,
        csrf_token: &str,
        expires_at: &str,
    ) -> Result<(), String> {
        self.with_conn(|conn| {
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            let now = rfc3339(unix_now());
            tx.execute(
                "UPDATE sessions SET revoked_at = ?, rotated_to = ? WHERE token_hash = ? AND revoked_at IS NULL",
                params![now, new_hash, old_hash],
            )
            .map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT INTO sessions (token_hash, user_id, csrf_token, created_at, expires_at, last_seen_at) VALUES (?, ?, ?, ?, ?, ?)",
                params![new_hash, user_id, csrf_token, now, expires_at, now],
            )
            .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            Ok(())
        })
    }
}

/// Recovery token record (hashes only).
#[derive(Debug, Clone)]
pub struct RecoveryToken {
    pub id: String,
    pub user_id: String,
    pub expires_at: String,
    pub used_at: Option<String>,
}

impl Db {
    /// Create a recovery token for a user.
    pub fn create_recovery(
        &self,
        id: &str,
        user_id: &str,
        token_hash: &str,
        expires_at: &str,
    ) -> Result<(), String> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO recovery_tokens (id, user_id, token_hash, created_at, expires_at) VALUES (?, ?, ?, ?, ?)",
                params![id, user_id, token_hash, rfc3339(unix_now()), expires_at],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        })
    }

    /// Look up a live recovery token by hash.
    pub fn live_recovery(
        &self,
        token_hash: &str,
        now: &str,
    ) -> Result<Option<RecoveryToken>, String> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, user_id, expires_at, used_at FROM recovery_tokens WHERE token_hash = ? AND used_at IS NULL",
                params![token_hash],
                |r| {
                    Ok(RecoveryToken {
                        id: r.get(0)?,
                        user_id: r.get(1)?,
                        expires_at: r.get(2)?,
                        used_at: r.get(3)?,
                    })
                },
            )
            .optional()
            .map_err(|e| e.to_string())
            .map(|opt| opt.filter(|t| t.expires_at.as_str() > now))
        })
    }

    /// Consume a recovery token and set the new password hash atomically.
    pub fn consume_recovery(
        &self,
        token_id: &str,
        user_id: &str,
        new_hash: &str,
    ) -> Result<(), String> {
        self.with_conn(|conn| {
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            let now = rfc3339(unix_now());
            tx.execute(
                "UPDATE recovery_tokens SET used_at = ? WHERE id = ? AND used_at IS NULL",
                params![now, token_id],
            )
            .map_err(|e| e.to_string())?;
            tx.execute(
                "UPDATE users SET password_hash = ?, updated_at = ? WHERE id = ?",
                params![new_hash, now, user_id],
            )
            .map_err(|e| e.to_string())?;
            tx.execute(
                "UPDATE sessions SET revoked_at = ? WHERE user_id = ? AND revoked_at IS NULL",
                params![now, user_id],
            )
            .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            Ok(())
        })
    }

    /// Count recent recovery requests for rate limiting.
    pub fn recent_recovery_count(&self, user_id: &str, since: &str) -> Result<i64, String> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM recovery_tokens WHERE user_id = ? AND created_at > ?",
                params![user_id, since],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())
        })
    }
}

/// Animal record owned by one user.
#[derive(Debug, Clone)]
pub struct Animal {
    pub id: String,
    pub user_id: String,
    pub name: String,
    pub breed: Option<String>,
    pub sex: Option<String>,
    pub birth_year: Option<i64>,
    pub notes: Option<String>,
    pub archived: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl Db {
    /// Create an animal for a user.
    #[allow(clippy::too_many_arguments)]
    pub fn create_animal(
        &self,
        id: &str,
        user_id: &str,
        name: &str,
        breed: Option<&str>,
        sex: Option<&str>,
        birth_year: Option<i64>,
        notes: Option<&str>,
    ) -> Result<Animal, String> {
        self.with_conn(|conn| {
            let now = rfc3339(unix_now());
            conn.execute(
                "INSERT INTO animals (id, user_id, name, breed, sex, birth_year, notes, archived, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?, ?)",
                params![id, user_id, name, breed, sex, birth_year, notes, now, now],
            )
            .map_err(|e| e.to_string())?;
            Ok(Animal {
                id: id.to_string(),
                user_id: user_id.to_string(),
                name: name.to_string(),
                breed: breed.map(str::to_string),
                sex: sex.map(str::to_string),
                birth_year,
                notes: notes.map(str::to_string),
                archived: false,
                created_at: now.clone(),
                updated_at: now,
            })
        })
    }

    /// List a user's animals (optionally including archived).
    pub fn list_animals(
        &self,
        user_id: &str,
        include_archived: bool,
    ) -> Result<Vec<Animal>, String> {
        self.with_conn(|conn| {
            let sql = if include_archived {
                "SELECT id, user_id, name, breed, sex, birth_year, notes, archived, created_at, updated_at FROM animals WHERE user_id = ? ORDER BY created_at"
            } else {
                "SELECT id, user_id, name, breed, sex, birth_year, notes, archived, created_at, updated_at FROM animals WHERE user_id = ? AND archived = 0 ORDER BY created_at"
            };
            let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![user_id], |r| {
                    Ok(Animal {
                        id: r.get(0)?,
                        user_id: r.get(1)?,
                        name: r.get(2)?,
                        breed: r.get(3)?,
                        sex: r.get(4)?,
                        birth_year: r.get(5)?,
                        notes: r.get(6)?,
                        archived: r.get::<_, i64>(7)? != 0,
                        created_at: r.get(8)?,
                        updated_at: r.get(9)?,
                    })
                })
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
        })
    }

    /// Fetch one animal (ownership checked by caller via `user_id` match).
    pub fn animal_by_id(&self, id: &str) -> Result<Option<Animal>, String> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, user_id, name, breed, sex, birth_year, notes, archived, created_at, updated_at FROM animals WHERE id = ?",
                params![id],
                |r| {
                    Ok(Animal {
                        id: r.get(0)?,
                        user_id: r.get(1)?,
                        name: r.get(2)?,
                        breed: r.get(3)?,
                        sex: r.get(4)?,
                        birth_year: r.get(5)?,
                        notes: r.get(6)?,
                        archived: r.get::<_, i64>(7)? != 0,
                        created_at: r.get(8)?,
                        updated_at: r.get(9)?,
                    })
                },
            )
            .optional()
            .map_err(|e| e.to_string())
        })
    }

    /// Update an animal's editable fields (ownership checked by caller).
    #[allow(clippy::too_many_arguments)]
    pub fn update_animal(
        &self,
        id: &str,
        name: &str,
        breed: Option<&str>,
        sex: Option<&str>,
        birth_year: Option<i64>,
        notes: Option<&str>,
        archived: bool,
    ) -> Result<(), String> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE animals SET name = ?, breed = ?, sex = ?, birth_year = ?, notes = ?, archived = ?, updated_at = ? WHERE id = ?",
                params![name, breed, sex, birth_year, notes, archived as i64, rfc3339(unix_now()), id],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        })
    }

    /// Delete an animal and its estimate links (estimates keep their rows
    /// with `animal_id` nulled via `ON DELETE SET NULL`; here we delete
    /// explicitly for clarity). Ownership checked by caller.
    pub fn delete_animal(&self, id: &str) -> Result<(), String> {
        self.with_conn(|conn| {
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            tx.execute(
                "UPDATE estimates SET animal_id = NULL WHERE animal_id = ?",
                params![id],
            )
            .map_err(|e| e.to_string())?;
            tx.execute("DELETE FROM animals WHERE id = ?", params![id])
                .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            Ok(())
        })
    }
}

/// Saved estimate row (server-side history).
#[derive(Debug, Clone)]
pub struct Estimate {
    pub id: String,
    pub user_id: String,
    pub animal_id: Option<String>,
    pub created_at: String,
    pub measured_at: Option<String>,
    pub weight_kg: f64,
    pub weight_min_kg: f64,
    pub weight_max_kg: f64,
    pub source: String,
    pub method: String,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub estimator_version: String,
    pub prompt_version: String,
    pub prompt_used: String,
    pub heart_girth_cm: Option<f64>,
    pub body_length_cm: Option<f64>,
    pub confidence: Option<f64>,
    pub breed: Option<String>,
    pub body_condition_score: Option<f64>,
    pub animal_breed: Option<String>,
    pub animal_sex: Option<String>,
    pub animal_age_years: Option<f64>,
    pub scale_weight_kg: Option<f64>,
    pub placeholder: bool,
    pub disclaimer: String,
    pub idempotency_key: Option<String>,
    pub request_id: String,
}

/// Filters for history listing (all optional).
#[derive(Debug, Default)]
pub struct HistoryFilter {
    pub animal_id: Option<String>,
    pub source: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
    pub page: i64,
    pub per_page: i64,
}

impl Db {
    /// Insert an estimate row. Returns `Ok(false)` when the idempotency key
    /// was already used (caller should return the existing row instead).
    pub fn insert_estimate(&self, e: &Estimate) -> Result<bool, String> {
        self.with_conn(|conn| {
            let rows = conn
                .execute(
                    "INSERT INTO estimates (id, user_id, animal_id, created_at, measured_at, weight_kg, weight_min_kg, weight_max_kg, source, method, model, provider, estimator_version, prompt_version, prompt_used, heart_girth_cm, body_length_cm, confidence, breed, body_condition_score, animal_breed, animal_sex, animal_age_years, scale_weight_kg, placeholder, disclaimer, idempotency_key, request_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    params![
                        e.id, e.user_id, e.animal_id, e.created_at, e.measured_at,
                        e.weight_kg, e.weight_min_kg, e.weight_max_kg, e.source, e.method,
                        e.model, e.provider, e.estimator_version, e.prompt_version, e.prompt_used,
                        e.heart_girth_cm, e.body_length_cm, e.confidence, e.breed,
                        e.body_condition_score, e.animal_breed, e.animal_sex, e.animal_age_years,
                        e.scale_weight_kg, e.placeholder as i64, e.disclaimer,
                        e.idempotency_key, e.request_id,
                    ],
                )
                .map_err(|e| e.to_string())?;
            Ok(rows == 1)
        })
    }

    /// Fetch the row stored under an idempotency key (replay path).
    pub fn estimate_by_idempotency(
        &self,
        user_id: &str,
        key: &str,
    ) -> Result<Option<Estimate>, String> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, user_id, animal_id, created_at, measured_at, weight_kg, weight_min_kg, weight_max_kg, source, method, model, provider, estimator_version, prompt_version, prompt_used, heart_girth_cm, body_length_cm, confidence, breed, body_condition_score, animal_breed, animal_sex, animal_age_years, scale_weight_kg, placeholder, disclaimer, idempotency_key, request_id FROM estimates WHERE user_id = ? AND idempotency_key = ?",
                params![user_id, key],
                row_to_estimate,
            )
            .optional()
            .map_err(|e| e.to_string())
        })
    }

    /// Fetch one estimate by id.
    pub fn estimate_by_id(&self, id: &str) -> Result<Option<Estimate>, String> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT id, user_id, animal_id, created_at, measured_at, weight_kg, weight_min_kg, weight_max_kg, source, method, model, provider, estimator_version, prompt_version, prompt_used, heart_girth_cm, body_length_cm, confidence, breed, body_condition_score, animal_breed, animal_sex, animal_age_years, scale_weight_kg, placeholder, disclaimer, idempotency_key, request_id FROM estimates WHERE id = ?",
                params![id],
                row_to_estimate,
            )
            .optional()
            .map_err(|e| e.to_string())
        })
    }

    /// Paginated, filtered history for one user (newest first).
    pub fn list_estimates(
        &self,
        user_id: &str,
        filter: &HistoryFilter,
    ) -> Result<(Vec<Estimate>, i64), String> {
        let page = filter.page.max(1);
        let per_page = filter.per_page.clamp(1, 100);
        self.with_conn(|conn| {
            let mut where_parts = vec!["user_id = ?".to_string()];
            let mut args: Vec<String> = vec![user_id.to_string()];
            if let Some(a) = &filter.animal_id {
                where_parts.push("animal_id = ?".to_string());
                args.push(a.clone());
            }
            if let Some(s) = &filter.source {
                where_parts.push("source = ?".to_string());
                args.push(s.clone());
            }
            if let Some(f) = &filter.from {
                where_parts.push("created_at >= ?".to_string());
                args.push(f.clone());
            }
            if let Some(t) = &filter.to {
                where_parts.push("created_at <= ?".to_string());
                args.push(t.clone());
            }
            let where_sql = where_parts.join(" AND ");
            let total: i64 = conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM estimates WHERE {}", where_sql),
                    rusqlite::params_from_iter(args.iter()),
                    |r| r.get(0),
                )
                .map_err(|e| e.to_string())?;
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT id, user_id, animal_id, created_at, measured_at, weight_kg, weight_min_kg, weight_max_kg, source, method, model, provider, estimator_version, prompt_version, prompt_used, heart_girth_cm, body_length_cm, confidence, breed, body_condition_score, animal_breed, animal_sex, animal_age_years, scale_weight_kg, placeholder, disclaimer, idempotency_key, request_id FROM estimates WHERE {} ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?",
                    where_sql
                ))
                .map_err(|e| e.to_string())?;
            let mut all: Vec<String> = args;
            all.push(per_page.to_string());
            all.push(((page - 1) * per_page).to_string());
            let rows = stmt
                .query_map(rusqlite::params_from_iter(all.iter()), row_to_estimate)
                .map_err(|e| e.to_string())?;
            let items = rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
            Ok((items, total))
        })
    }

    /// Delete one estimate (ownership checked by caller).
    pub fn delete_estimate(&self, id: &str) -> Result<bool, String> {
        self.with_conn(|conn| {
            let rows = conn
                .execute("DELETE FROM estimates WHERE id = ?", params![id])
                .map_err(|e| e.to_string())?;
            Ok(rows == 1)
        })
    }

    /// Count a user's estimates (for smoke assertions).
    pub fn estimate_count(&self, user_id: &str) -> Result<i64, String> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM estimates WHERE user_id = ?",
                params![user_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())
        })
    }
}

fn row_to_estimate(r: &rusqlite::Row<'_>) -> Result<Estimate, rusqlite::Error> {
    Ok(Estimate {
        id: r.get(0)?,
        user_id: r.get(1)?,
        animal_id: r.get(2)?,
        created_at: r.get(3)?,
        measured_at: r.get(4)?,
        weight_kg: r.get(5)?,
        weight_min_kg: r.get(6)?,
        weight_max_kg: r.get(7)?,
        source: r.get(8)?,
        method: r.get(9)?,
        model: r.get(10)?,
        provider: r.get(11)?,
        estimator_version: r.get(12)?,
        prompt_version: r.get(13)?,
        prompt_used: r.get(14)?,
        heart_girth_cm: r.get(15)?,
        body_length_cm: r.get(16)?,
        confidence: r.get(17)?,
        breed: r.get(18)?,
        body_condition_score: r.get(19)?,
        animal_breed: r.get(20)?,
        animal_sex: r.get(21)?,
        animal_age_years: r.get(22)?,
        scale_weight_kg: r.get(23)?,
        placeholder: r.get::<_, i64>(24)? != 0,
        disclaimer: r.get(25)?,
        idempotency_key: r.get(26)?,
        request_id: r.get(27)?,
    })
}

impl Db {
    /// Today's image-estimate usage for a user.
    pub fn daily_usage(&self, user_id: &str, day: &str) -> Result<i64, String> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT COALESCE((SELECT image_estimates FROM usage_counters WHERE user_id = ? AND day = ?), 0)",
                params![user_id, day],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())
        })
    }

    /// Increment today's image-estimate usage.
    pub fn bump_usage(&self, user_id: &str, day: &str) -> Result<(), String> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO usage_counters (user_id, day, image_estimates) VALUES (?, ?, 1) ON CONFLICT(user_id, day) DO UPDATE SET image_estimates = image_estimates + 1",
                params![user_id, day],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        })
    }

    /// Per-day usage rows for operator review (capped).
    pub fn usage_rows(&self, limit: i64) -> Result<Vec<(String, String, i64)>, String> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT user_id, day, image_estimates FROM usage_counters ORDER BY day DESC LIMIT ?")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![limit.clamp(1, 500)], |r| {
                    Ok((r.get(0)?, r.get(1)?, r.get(2)?))
                })
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
        })
    }
}

/// Retained photo record (bytes live in `<data_dir>/photos/<id>`).
#[derive(Debug, Clone)]
pub struct Photo {
    pub id: String,
    pub user_id: String,
    pub estimate_id: Option<String>,
    pub created_at: String,
    pub expires_at: String,
    pub bytes: i64,
    pub mime: String,
}

fn row_to_photo(r: &rusqlite::Row<'_>) -> Result<Photo, rusqlite::Error> {
    Ok(Photo {
        id: r.get(0)?,
        user_id: r.get(1)?,
        estimate_id: r.get(2)?,
        created_at: r.get(3)?,
        expires_at: r.get(4)?,
        bytes: r.get(5)?,
        mime: r.get(6)?,
    })
}

const PHOTO_COLS: &str =
    "id, user_id, estimate_id, created_at, expires_at, bytes, mime FROM photos";

impl Db {
    /// Insert a retained photo row.
    pub fn insert_photo(&self, photo: &Photo) -> Result<(), String> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO photos (id, user_id, estimate_id, created_at, expires_at, bytes, mime) VALUES (?, ?, ?, ?, ?, ?, ?)",
                params![
                    photo.id,
                    photo.user_id,
                    photo.estimate_id,
                    photo.created_at,
                    photo.expires_at,
                    photo.bytes,
                    photo.mime
                ],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        })
    }

    /// Fetch one photo by id.
    pub fn photo_by_id(&self, id: &str) -> Result<Option<Photo>, String> {
        self.with_conn(|conn| {
            conn.query_row(
                &format!("SELECT {} WHERE id = ?", PHOTO_COLS),
                params![id],
                row_to_photo,
            )
            .optional()
            .map_err(|e| e.to_string())
        })
    }

    /// List a user's photos, newest first (capped).
    pub fn list_photos(&self, user_id: &str, limit: i64) -> Result<Vec<Photo>, String> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {} WHERE user_id = ? ORDER BY created_at DESC LIMIT ?",
                    PHOTO_COLS
                ))
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![user_id, limit.clamp(1, 500)], row_to_photo)
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())
        })
    }

    /// Total retained bytes for quota accounting.
    pub fn photo_bytes_used(&self, user_id: &str) -> Result<i64, String> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT COALESCE(SUM(bytes), 0) FROM photos WHERE user_id = ?",
                params![user_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())
        })
    }

    /// Delete one photo row (file removal is the caller's job).
    pub fn delete_photo(&self, id: &str) -> Result<bool, String> {
        self.with_conn(|conn| {
            let rows = conn
                .execute("DELETE FROM photos WHERE id = ?", params![id])
                .map_err(|e| e.to_string())?;
            Ok(rows == 1)
        })
    }

    /// All photo ids (bounded: photo counts are quota-bounded per user, so
    /// the table stays small; used by the orphan sweep).
    pub fn all_photo_ids(&self) -> Result<Vec<String>, String> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT id FROM photos LIMIT 100000")
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map([], |r| r.get(0))
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())
        })
    }

    /// Photos linked to one estimate (history deletion removes them too).
    pub fn photos_for_estimate(&self, estimate_id: &str) -> Result<Vec<Photo>, String> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(&format!("SELECT {} WHERE estimate_id = ?", PHOTO_COLS))
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![estimate_id], row_to_photo)
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())
        })
    }

    /// Delete every photo row at/after expiry; returns the deleted ids so
    /// the caller can remove their files too.
    pub fn delete_expired_photos(&self, now: &str) -> Result<Vec<String>, String> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT id FROM photos WHERE expires_at <= ?")
                .map_err(|e| e.to_string())?;
            let ids: Vec<String> = stmt
                .query_map(params![now], |r| r.get(0))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;
            for id in &ids {
                conn.execute("DELETE FROM photos WHERE id = ?", params![id])
                    .map_err(|e| e.to_string())?;
            }
            Ok(ids)
        })
    }
}

/// Durable estimate job (background processing with restart recovery).
#[derive(Debug, Clone)]
pub struct Job {
    pub id: String,
    pub user_id: String,
    pub created_at: String,
    pub updated_at: String,
    pub run_after: String,
    pub status: String,
    pub attempts: i64,
    pub idempotency_key: Option<String>,
    pub payload: String,
    pub result: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub history_id: Option<String>,
}

fn row_to_job(r: &rusqlite::Row<'_>) -> Result<Job, rusqlite::Error> {
    Ok(Job {
        id: r.get(0)?,
        user_id: r.get(1)?,
        created_at: r.get(2)?,
        updated_at: r.get(3)?,
        run_after: r.get(4)?,
        status: r.get(5)?,
        attempts: r.get(6)?,
        idempotency_key: r.get(7)?,
        payload: r.get(8)?,
        result: r.get(9)?,
        error_code: r.get(10)?,
        error_message: r.get(11)?,
        history_id: r.get(12)?,
    })
}

const JOB_COLS: &str = "id, user_id, created_at, updated_at, run_after, status, attempts, idempotency_key, payload, result, error_code, error_message, history_id FROM jobs";

/// Maximum estimate attempts per job before it fails permanently.
pub const MAX_JOB_ATTEMPTS: i64 = 3;

impl Db {
    /// Create a queued job. Duplicate idempotency keys return the existing
    /// job id instead of a new row.
    pub fn create_job(
        &self,
        id: &str,
        user_id: &str,
        idempotency_key: Option<&str>,
        payload: &str,
        now: &str,
    ) -> Result<(String, bool), String> {
        if let Some(key) = idempotency_key {
            let existing: Option<String> = self.with_conn(|conn| {
                conn.query_row(
                    "SELECT id FROM jobs WHERE user_id = ? AND idempotency_key = ?",
                    params![user_id, key],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|e| e.to_string())
            })?;
            if let Some(job_id) = existing {
                return Ok((job_id, true));
            }
        }
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO jobs (id, user_id, created_at, updated_at, run_after, status, attempts, idempotency_key, payload) VALUES (?, ?, ?, ?, ?, 'queued', 0, ?, ?)",
                params![id, user_id, now, now, now, idempotency_key, payload],
            )
            .map_err(|e| e.to_string())?;
            Ok((id.to_string(), false))
        })
    }

    /// Fetch one job by id.
    pub fn job_by_id(&self, id: &str) -> Result<Option<Job>, String> {
        self.with_conn(|conn| {
            conn.query_row(
                &format!("SELECT {} WHERE id = ?", JOB_COLS),
                params![id],
                row_to_job,
            )
            .optional()
            .map_err(|e| e.to_string())
        })
    }

    /// List a user's jobs, newest first (capped).
    pub fn list_jobs(&self, user_id: &str, limit: i64) -> Result<Vec<Job>, String> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare(&format!(
                    "SELECT {} WHERE user_id = ? ORDER BY created_at DESC LIMIT ?",
                    JOB_COLS
                ))
                .map_err(|e| e.to_string())?;
            let rows = stmt
                .query_map(params![user_id, limit.clamp(1, 200)], row_to_job)
                .map_err(|e| e.to_string())?;
            rows.collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())
        })
    }

    /// Atomically claim the oldest due queued job (worker pickup).
    pub fn claim_next_job(&self, now: &str) -> Result<Option<Job>, String> {
        self.with_conn(|conn| {
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            let id: Option<String> = tx
                .query_row(
                    "SELECT id FROM jobs WHERE status = 'queued' AND run_after <= ? ORDER BY created_at LIMIT 1",
                    params![now],
                    |r| r.get(0),
                )
                .optional()
                .map_err(|e| e.to_string())?;
            let Some(id) = id else {
                return Ok(None);
            };
            tx.execute(
                "UPDATE jobs SET status = 'active', updated_at = ? WHERE id = ? AND status = 'queued'",
                params![now, id],
            )
            .map_err(|e| e.to_string())?;
            let job = tx
                .query_row(
                    &format!("SELECT {} WHERE id = ?", JOB_COLS),
                    params![id],
                    row_to_job,
                )
                .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            Ok(Some(job))
        })
    }

    /// Finish a job: success stores the result, failure stores the code.
    /// Requeue schedules another attempt with backoff.
    #[allow(clippy::too_many_arguments)]
    pub fn finish_job(
        &self,
        id: &str,
        status: &str,
        now: &str,
        run_after: Option<&str>,
        result: Option<&str>,
        error_code: Option<&str>,
        error_message: Option<&str>,
        history_id: Option<&str>,
    ) -> Result<(), String> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE jobs SET status = ?, updated_at = ?, run_after = COALESCE(?, run_after), attempts = attempts + 1, result = COALESCE(?, result), error_code = ?, error_message = ?, history_id = COALESCE(?, history_id) WHERE id = ?",
                params![status, now, run_after, result, error_code, error_message, history_id, id],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
        })
    }

    /// Cancel a queued job (active jobs run to completion).
    pub fn cancel_job(&self, id: &str, now: &str) -> Result<bool, String> {
        self.with_conn(|conn| {
            let rows = conn
                .execute(
                    "UPDATE jobs SET status = 'cancelled', updated_at = ? WHERE id = ? AND status = 'queued'",
                    params![now, id],
                )
                .map_err(|e| e.to_string())?;
            Ok(rows == 1)
        })
    }

    /// Crash recovery: jobs left `active` at shutdown go back to `queued`.
    /// Returns the number requeued.
    pub fn requeue_active_jobs(&self, now: &str) -> Result<i64, String> {
        self.with_conn(|conn| {
            let rows = conn
                .execute(
                    "UPDATE jobs SET status = 'queued', updated_at = ? WHERE status = 'active'",
                    params![now],
                )
                .map_err(|e| e.to_string())?;
            Ok(rows as i64)
        })
    }

    /// Prune terminal jobs older than `before` (history rows are untouched).
    /// Returns the number pruned.
    pub fn prune_jobs(&self, before: &str) -> Result<i64, String> {
        self.with_conn(|conn| {
            let rows = conn
                .execute(
                    "DELETE FROM jobs WHERE status IN ('success', 'failed', 'cancelled', 'expired') AND updated_at < ?",
                    params![before],
                )
                .map_err(|e| e.to_string())?;
            Ok(rows as i64)
        })
    }

    /// Expire stale queued jobs that never ran.
    pub fn expire_stale_queued(&self, before: &str, now: &str) -> Result<i64, String> {
        self.with_conn(|conn| {
            let rows = conn
                .execute(
                    "UPDATE jobs SET status = 'expired', updated_at = ? WHERE status = 'queued' AND created_at < ?",
                    params![now, before],
                )
                .map_err(|e| e.to_string())?;
            Ok(rows as i64)
        })
    }

    /// Count a user's non-terminal jobs (queue-depth guard).
    pub fn open_job_count(&self, user_id: &str) -> Result<i64, String> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM jobs WHERE user_id = ? AND status IN ('queued', 'active')",
                params![user_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db(name: &str) -> Db {
        let db = Db::open_temp(name).unwrap();
        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
        db
    }

    #[test]
    fn migrations_are_idempotent_and_versioned() {
        let dir = std::env::temp_dir().join(format!("aif-db-mig-{}", std::process::id()));
        let path = dir.to_str().unwrap();
        let first = Db::open(path).unwrap();
        first
            .create_user("u1", "a@example.com", "A", "hash", "operator")
            .unwrap();
        drop(first);
        let second = Db::open(path).unwrap();
        assert_eq!(second.schema_version().unwrap(), SCHEMA_VERSION);
        assert_eq!(second.active_user_count().unwrap(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn two_user_cap_is_enforced() {
        let db = test_db("cap");
        db.create_user("u1", "a@example.com", "A", "h", "operator")
            .unwrap();
        db.create_user("u2", "b@example.com", "B", "h", "user")
            .unwrap();
        let err = db
            .create_user("u3", "c@example.com", "C", "h", "user")
            .unwrap_err();
        assert!(err.contains("user_limit"));
    }

    #[test]
    fn failed_batch_insert_leaves_no_partial_rows() {
        let db = test_db("atomic");
        db.create_user("u1", "a@example.com", "A", "h", "user")
            .unwrap();
        // Duplicate primary key in a two-row transaction must roll back both.
        let result = db.with_conn(|conn| {
            let tx = conn.transaction().map_err(|e| e.to_string())?;
            tx.execute("INSERT INTO animals (id, user_id, name, created_at, updated_at) VALUES ('a1', 'u1', 'Bessie', 't', 't')", [])
                .map_err(|e| e.to_string())?;
            tx.execute("INSERT INTO animals (id, user_id, name, created_at, updated_at) VALUES ('a1', 'u1', 'Dupe', 't', 't')", [])
                .map_err(|e| e.to_string())?;
            tx.commit().map_err(|e| e.to_string())?;
            Ok(())
        });
        assert!(result.is_err());
        assert!(db.list_animals("u1", true).unwrap().is_empty());
    }

    #[test]
    fn migration_v2_preserves_v1_data() {
        // Simulate an old v1 database, then open with the current code and
        // prove existing rows survive the upgrade.
        let dir = std::env::temp_dir().join(format!("aif-db-v1-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("aif.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE schema_migrations (version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL);
                 INSERT INTO schema_migrations VALUES (1, '2026-01-01T00:00:00Z');
                 CREATE TABLE users (id TEXT PRIMARY KEY, email TEXT NOT NULL UNIQUE, display_name TEXT NOT NULL DEFAULT '', password_hash TEXT NOT NULL, role TEXT NOT NULL DEFAULT 'user', status TEXT NOT NULL DEFAULT 'active', created_at TEXT NOT NULL, updated_at TEXT NOT NULL, last_login_at TEXT);
                 INSERT INTO users VALUES ('u1', 'a@example.com', 'A', 'h', 'user', 'active', 't', 't', NULL);",
            )
            .unwrap();
        }
        let db = Db::open(dir.to_str().unwrap()).unwrap();
        assert_eq!(db.schema_version().unwrap(), SCHEMA_VERSION);
        assert_eq!(db.active_user_count().unwrap(), 1);
        assert!(db.user_by_id("u1").unwrap().is_some());
        // v2 tables exist and work.
        assert!(db.list_photos("u1", 10).unwrap().is_empty());
        assert!(db.list_jobs("u1", 10).unwrap().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn job_lifecycle_claim_finish_cancel() {
        let db = test_db("jobs");
        db.create_user("u1", "a@example.com", "A", "h", "user")
            .unwrap();
        let now = "2026-06-01T00:00:00Z";
        let (id, dup) = db
            .create_job("j1", "u1", Some("k1"), r#"{"a":1}"#, now)
            .unwrap();
        assert!(!dup);
        // Duplicate idempotency keys return the existing job, not a new row.
        let (id2, dup2) = db
            .create_job("j2", "u1", Some("k1"), r#"{"a":2}"#, now)
            .unwrap();
        assert!(dup2);
        assert_eq!(id, id2);
        let claimed = db.claim_next_job(now).unwrap().unwrap();
        assert_eq!(claimed.id, "j1");
        assert_eq!(claimed.status, "active");
        // Nothing else to claim; cancel only works on queued jobs.
        assert!(db.claim_next_job(now).unwrap().is_none());
        assert!(!db.cancel_job("j1", now).unwrap());
        db.finish_job(
            "j1",
            "success",
            now,
            None,
            Some(r#"{"ok":true}"#),
            None,
            None,
            Some("h1"),
        )
        .unwrap();
        let done = db.job_by_id("j1").unwrap().unwrap();
        assert_eq!(done.status, "success");
        assert_eq!(done.history_id.as_deref(), Some("h1"));
        assert_eq!(done.attempts, 1);
        // Crash recovery requeues interrupted work without duplicating it.
        db.create_job("j3", "u1", None, "{}", now).unwrap();
        let active = db.claim_next_job(now).unwrap().unwrap();
        assert_eq!(active.status, "active");
        assert_eq!(db.requeue_active_jobs(now).unwrap(), 1);
        assert_eq!(db.job_by_id("j3").unwrap().unwrap().status, "queued");
        // Cancel a queued job; prune only terminal rows.
        assert!(db.cancel_job("j3", now).unwrap());
        assert_eq!(db.prune_jobs("2026-07-01T00:00:00Z").unwrap(), 2);
        assert!(db.job_by_id("j1").unwrap().is_none());
    }

    #[test]
    fn photo_expiry_deletes_only_expired_rows() {
        let db = test_db("photoexp");
        db.create_user("u1", "a@example.com", "A", "h", "user")
            .unwrap();
        let mk = |id: &str, expires: &str| Photo {
            id: id.to_string(),
            user_id: "u1".to_string(),
            estimate_id: None,
            created_at: "2026-01-01T00:00:00Z".to_string(),
            expires_at: expires.to_string(),
            bytes: 10,
            mime: "image/jpeg".to_string(),
        };
        db.insert_photo(&mk("old", "2026-02-01T00:00:00Z")).unwrap();
        db.insert_photo(&mk("fresh", "2027-01-01T00:00:00Z"))
            .unwrap();
        assert_eq!(db.photo_bytes_used("u1").unwrap(), 20);
        let deleted = db.delete_expired_photos("2026-06-01T00:00:00Z").unwrap();
        assert_eq!(deleted, vec!["old".to_string()]);
        assert!(db.photo_by_id("fresh").unwrap().is_some());
    }

    #[test]
    fn expired_sessions_fail_closed() {
        let db = test_db("sessexp");
        db.create_user("u1", "a@example.com", "A", "h", "user")
            .unwrap();
        db.create_session("live", "u1", "csrf", "2999-01-01T00:00:00Z")
            .unwrap();
        db.create_session("dead", "u1", "csrf", "2020-01-01T00:00:00Z")
            .unwrap();
        assert!(db
            .check_session("live", "2026-01-01T00:00:00Z")
            .unwrap()
            .is_some());
        assert!(db
            .check_session("dead", "2026-01-01T00:00:00Z")
            .unwrap()
            .is_none());
        assert!(db
            .check_session("nope", "2026-01-01T00:00:00Z")
            .unwrap()
            .is_none());
        // Rotation revokes the old token and honors the new one.
        db.rotate_session("live", "next", "u1", "csrf2", "2999-01-01T00:00:00Z")
            .unwrap();
        assert!(db
            .check_session("live", "2026-01-01T00:00:00Z")
            .unwrap()
            .is_none());
        let rotated = db
            .check_session("next", "2026-01-01T00:00:00Z")
            .unwrap()
            .unwrap();
        assert_eq!(rotated.csrf_token, "csrf2");
    }

    #[test]
    fn account_deletion_revokes_sessions_and_data() {
        let db = test_db("delcascade");
        db.create_user("u1", "a@example.com", "A", "h", "user")
            .unwrap();
        db.create_session("sess", "u1", "csrf", "2999-01-01T00:00:00Z")
            .unwrap();
        db.create_animal("a1", "u1", "Bessie", None, None, None, None)
            .unwrap();
        let (users, sessions) = db.delete_account("u1").unwrap();
        assert_eq!((users, sessions), (1, 1));
        assert!(db
            .check_session("sess", "2026-01-01T00:00:00Z")
            .unwrap()
            .is_none());
        assert!(db.list_animals("u1", true).unwrap().is_empty());
    }
}
