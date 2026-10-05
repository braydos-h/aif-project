//! Operator command-line administration (invites, users, recovery).
//!
//! These commands run against the same SQLite database as the server and
//! are the secure operator-delivery channel when email is not configured:
//! the operator mints invite/recovery links here and relays them over a
//! private channel. Tokens are printed once to stdout (never logged).

use crate::auth::{new_id, normalize_email, random_hex, token_hash, valid_email, TOKEN_BYTES};
use crate::config::Config;
use crate::db::Db;
use crate::time_util::{rfc3339, unix_now};

/// Run an operator command against the database. Returns the process exit
/// code (0 = success).
pub fn run_admin(command: &crate::args::AdminCommand, config: &Config) -> i32 {
    let db = match Db::open(&config.data_dir) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("cannot open database: {}", e);
            return 1;
        }
    };
    match command {
        crate::args::AdminCommand::CreateInvite { email, role } => {
            create_invite(&db, config, email, role)
        }
        crate::args::AdminCommand::ListUsers => list_users(&db),
        crate::args::AdminCommand::ListInvites => list_invites(&db),
        crate::args::AdminCommand::RevokeInvite { id } => revoke_invite(&db, id),
        crate::args::AdminCommand::DeleteUser { email } => delete_user(&db, email),
        crate::args::AdminCommand::CreateRecovery { email } => create_recovery(&db, config, email),
    }
}

fn invite_url(config: &Config, token: &str) -> String {
    format!(
        "{}/#invite={}",
        config.public_origin.trim_end_matches('/'),
        token
    )
}

fn recovery_url(config: &Config, token: &str) -> String {
    format!(
        "{}/#recovery={}",
        config.public_origin.trim_end_matches('/'),
        token
    )
}

fn create_invite(db: &Db, config: &Config, email: &str, role: &str) -> i32 {
    let email = normalize_email(email);
    if !valid_email(&email) {
        eprintln!("invalid email address");
        return 2;
    }
    if role != "user" && role != "operator" {
        eprintln!("role must be user or operator");
        return 2;
    }
    let token = random_hex(TOKEN_BYTES);
    let expires_at = rfc3339(unix_now() + config.invite_days * 86_400);
    let id = new_id();
    match db.create_invite(&id, &email, role, &token_hash(&token), None, &expires_at) {
        Ok(_) => {
            let _ = db.audit(
                None,
                "invite_created_cli",
                &format!("invite={} email={}", id, email),
                "cli",
            );
            println!(
                "Invite for {} (role {}, expires {}):",
                email, role, expires_at
            );
            println!("{}", invite_url(config, &token));
            println!("Deliver this link over a private channel. It is shown once and expires.");
            0
        }
        Err(e) => {
            eprintln!("cannot create invite: {}", e);
            1
        }
    }
}

fn list_users(db: &Db) -> i32 {
    match db.list_users() {
        Ok(users) => {
            if users.is_empty() {
                println!("No accounts yet.");
            }
            for u in users {
                println!(
                    "{} | {} | {} | {} | created {} | last login {}",
                    u.id,
                    u.email,
                    u.role,
                    u.status,
                    u.created_at,
                    u.last_login_at.as_deref().unwrap_or("never"),
                );
            }
            0
        }
        Err(e) => {
            eprintln!("cannot list users: {}", e);
            1
        }
    }
}

fn list_invites(db: &Db) -> i32 {
    match db.list_invites() {
        Ok(invites) => {
            if invites.is_empty() {
                println!("No invites yet.");
            }
            let now = rfc3339(unix_now());
            for inv in invites {
                let status = if inv.used_at.is_some() {
                    "used"
                } else if inv.revoked_at.is_some() {
                    "revoked"
                } else if inv.expires_at.as_str() < now.as_str() {
                    "expired"
                } else {
                    "active"
                };
                println!(
                    "{} | {} | {} | {} | expires {}",
                    inv.id, inv.email, inv.role, status, inv.expires_at
                );
            }
            0
        }
        Err(e) => {
            eprintln!("cannot list invites: {}", e);
            1
        }
    }
}

fn revoke_invite(db: &Db, id: &str) -> i32 {
    match db.revoke_invite(id) {
        Ok(true) => {
            let _ = db.audit(None, "invite_revoked_cli", &format!("invite={}", id), "cli");
            println!("Invite {} revoked.", id);
            0
        }
        Ok(false) => {
            eprintln!("Invite {} not found, already used, or already revoked.", id);
            1
        }
        Err(e) => {
            eprintln!("cannot revoke invite: {}", e);
            1
        }
    }
}

fn delete_user(db: &Db, email: &str) -> i32 {
    let email = normalize_email(email);
    let user = match db.user_by_email_with_hash(&email) {
        Ok(Some((user, _))) => user,
        _ => {
            eprintln!("No account with that email.");
            return 1;
        }
    };
    match db.delete_account(&user.id) {
        Ok(_) => {
            let _ = db.audit(None, "user_deleted_cli", &format!("email={}", email), "cli");
            println!(
                "Account {} deleted (sessions revoked, data removed).",
                email
            );
            0
        }
        Err(e) => {
            eprintln!("cannot delete user: {}", e);
            1
        }
    }
}

/// Mint a recovery link for an active account (operator relay when email
/// delivery is not configured). Bounded like the API path: at most 3 live
/// requests per 24 h.
fn create_recovery(db: &Db, config: &Config, email: &str) -> i32 {
    let email = normalize_email(email);
    let user = match db.user_by_email_with_hash(&email) {
        Ok(Some((user, _))) if user.status == "active" => user,
        _ => {
            eprintln!("No active account with that email.");
            return 1;
        }
    };
    let since = rfc3339(unix_now().saturating_sub(86_400));
    if db.recent_recovery_count(&user.id, &since).unwrap_or(99) >= 3 {
        eprintln!("Recovery was already requested 3 times in the last 24 h; wait before retrying.");
        return 1;
    }
    let token = random_hex(TOKEN_BYTES);
    let expires = rfc3339(unix_now() + 24 * 3600);
    match db.create_recovery(&new_id(), &user.id, &token_hash(&token), &expires) {
        Ok(()) => {
            let _ = db.audit(
                Some(&user.id),
                "recovery_created_cli",
                &format!("email={}", email),
                "cli",
            );
            println!("Recovery for {} (expires {}):", email, expires);
            println!("{}", recovery_url(config, &token));
            println!("Deliver this link over a private channel. Single-use; all sessions are revoked on use.");
            0
        }
        Err(e) => {
            eprintln!("cannot create recovery: {}", e);
            1
        }
    }
}

/// First-run bootstrap: when the database has no accounts and
/// `AIF_OPERATOR_EMAIL` is set, mint the initial operator invite and print
/// its one-time link to stderr for private relay.
pub fn maybe_bootstrap(config: &Config) {
    let Some(email) = config.operator_email.clone() else {
        return;
    };
    let db = match Db::open(&config.data_dir) {
        Ok(db) => db,
        Err(e) => {
            eprintln!("bootstrap: cannot open database: {}", e);
            return;
        }
    };
    match db.active_user_count() {
        Ok(0) => {}
        Ok(_) => return,
        Err(e) => {
            eprintln!("bootstrap: cannot count users: {}", e);
            return;
        }
    }
    if !valid_email(&normalize_email(&email)) {
        eprintln!("bootstrap: AIF_OPERATOR_EMAIL is not a valid email; skipping invite creation");
        return;
    }
    let token = random_hex(TOKEN_BYTES);
    let expires_at = rfc3339(unix_now() + config.invite_days * 86_400);
    match db.create_invite(
        &new_id(),
        &normalize_email(&email),
        "operator",
        &token_hash(&token),
        None,
        &expires_at,
    ) {
        Ok(_) => {
            eprintln!(
                "bootstrap: no accounts exist; created an operator invite for {}",
                email
            );
            eprintln!(
                "bootstrap: one-time invite link (deliver privately): {}",
                invite_url(config, &token)
            );
        }
        Err(e) => eprintln!("bootstrap: cannot create operator invite: {}", e),
    }
}
