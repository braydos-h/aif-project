//! CLI argument parsing for the backend binary.
//!
//! Flags:
//! - `--port N` (default 8080, `0` = ephemeral, printed to stdout)
//! - `--host H` (default 0.0.0.0, all interfaces)
//!
//! Operator administration (run against `AIF_DATA_DIR`, then exit):
//! - `--create-invite EMAIL [--role user|operator]`
//! - `--list-users`, `--list-invites`
//! - `--revoke-invite ID`, `--delete-user EMAIL`, `--create-recovery EMAIL`

/// Parsed CLI arguments.
#[derive(Debug, PartialEq)]
pub struct Args {
    pub host: String,
    pub port: u16,
    pub command: Option<AdminCommand>,
}

/// Operator administration command (runs, then the process exits).
#[derive(Debug, PartialEq, Clone)]
pub enum AdminCommand {
    CreateInvite { email: String, role: String },
    ListUsers,
    ListInvites,
    RevokeInvite { id: String },
    DeleteUser { email: String },
    CreateRecovery { email: String },
}

pub const USAGE: &str = "Usage: aif-backend [--host HOST] [--port PORT] [--create-invite EMAIL [--role user|operator]] [--list-users] [--list-invites] [--revoke-invite ID] [--delete-user EMAIL] [--create-recovery EMAIL]\n";

/// Parse CLI args. `Ok(None)` means `--help`/`-h` was requested.
/// Supports `--flag value` and `--flag=value` forms.
pub fn parse_args(args: &[String]) -> Result<Option<Args>, String> {
    let mut host = "0.0.0.0".to_string();
    let mut port: u16 = 8080;
    let mut command: Option<AdminCommand> = None;
    let mut pending_role: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        let (flag, inline): (&str, Option<&str>) = match args[i].split_once('=') {
            Some((f, v)) => (f, Some(v)),
            None => (args[i].as_str(), None),
        };
        // `--flag value` helper: inline first, else consume the next arg.
        let mut take_value = |inline: Option<&str>| -> Result<String, String> {
            if let Some(v) = inline {
                return Ok(v.to_string());
            }
            i += 1;
            args.get(i)
                .cloned()
                .ok_or_else(|| format!("Missing value for {}", args[i - 1]))
        };
        match flag {
            "--host" => {
                host = take_value(inline)?;
            }
            "--port" => {
                let value = take_value(inline)?;
                port = value
                    .parse::<u16>()
                    .map_err(|_| format!("Invalid --port value: {}", value))?;
            }
            "--create-invite" => {
                let email = take_value(inline)?;
                command = Some(AdminCommand::CreateInvite {
                    email,
                    role: pending_role.take().unwrap_or_else(|| "user".to_string()),
                });
            }
            "--role" => {
                let role = take_value(inline)?;
                match &mut command {
                    Some(AdminCommand::CreateInvite { role: slot, .. }) => *slot = role,
                    _ => pending_role = Some(role),
                }
            }
            "--list-users" => command = Some(AdminCommand::ListUsers),
            "--list-invites" => command = Some(AdminCommand::ListInvites),
            "--revoke-invite" => {
                let id = take_value(inline)?;
                command = Some(AdminCommand::RevokeInvite { id });
            }
            "--delete-user" => {
                let email = take_value(inline)?;
                command = Some(AdminCommand::DeleteUser { email });
            }
            "--create-recovery" => {
                let email = take_value(inline)?;
                command = Some(AdminCommand::CreateRecovery { email });
            }
            "-h" | "--help" => return Ok(None),
            other => return Err(format!("Unknown argument: {}", other)),
        }
        i += 1;
    }
    Ok(Some(Args {
        host,
        port,
        command,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn invalid_port_is_an_error_not_silent_8080() {
        assert!(parse_args(&args(&["aif-backend", "--port", "abc"])).is_err());
        assert!(parse_args(&args(&["aif-backend", "--port", "99999"])).is_err());
    }

    #[test]
    fn missing_value_unknown_flag_and_equals_form() {
        assert!(parse_args(&args(&["aif-backend", "--port"])).is_err());
        assert!(parse_args(&args(&["aif-backend", "--hots", "x"])).is_err());
        let got = parse_args(&args(&["aif-backend", "--port=9000"]))
            .unwrap()
            .unwrap();
        assert_eq!((got.host, got.port), ("0.0.0.0".to_string(), 9000));
    }

    #[test]
    fn help_returns_none() {
        assert_eq!(parse_args(&args(&["aif-backend", "--help"])), Ok(None));
    }

    #[test]
    fn admin_commands_parse() {
        let got = parse_args(&args(&["aif-backend", "--create-invite", "a@example.com"]))
            .unwrap()
            .unwrap();
        assert_eq!(
            got.command,
            Some(AdminCommand::CreateInvite {
                email: "a@example.com".to_string(),
                role: "user".to_string(),
            })
        );
        let got = parse_args(&args(&[
            "aif-backend",
            "--create-invite",
            "a@example.com",
            "--role",
            "operator",
        ]))
        .unwrap()
        .unwrap();
        assert_eq!(
            got.command,
            Some(AdminCommand::CreateInvite {
                email: "a@example.com".to_string(),
                role: "operator".to_string(),
            })
        );
        let got = parse_args(&args(&["aif-backend", "--list-users"]))
            .unwrap()
            .unwrap();
        assert_eq!(got.command, Some(AdminCommand::ListUsers));
    }
}
