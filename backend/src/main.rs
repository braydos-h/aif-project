//! Entry point: build the config from env / `.env`, then serve HTTP.
//!
//! Flags:
//! - `--port N` (default 8080, `0` = ephemeral, printed to stdout)
//! - `--host H` (default 127.0.0.1)

use std::sync::Arc;

use aif_backend::config::{load_env_file, Config};
use aif_backend::http::{serve, ServerState};

/// Parsed CLI arguments.
#[derive(Debug, PartialEq)]
struct Args {
    host: String,
    port: u16,
}

const USAGE: &str = "Usage: aif-backend [--host HOST] [--port PORT]\n";

/// Parse CLI args. `Ok(None)` means `--help`/`-h` was requested.
/// Supports `--flag value` and `--flag=value` forms.
fn parse_args(args: &[String]) -> Result<Option<Args>, String> {
    let mut host = "127.0.0.1".to_string();
    let mut port: u16 = 8080;
    let mut i = 1;
    while i < args.len() {
        let (flag, inline): (&str, Option<&str>) = match args[i].split_once('=') {
            Some((f, v)) => (f, Some(v)),
            None => (args[i].as_str(), None),
        };
        match flag {
            "--host" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| {
                        i += 1;
                        args.get(i).cloned()
                    })
                    .ok_or_else(|| "Missing value for --host".to_string())?;
                host = value;
            }
            "--port" => {
                let value = inline
                    .map(str::to_string)
                    .or_else(|| {
                        i += 1;
                        args.get(i).cloned()
                    })
                    .ok_or_else(|| "Missing value for --port".to_string())?;
                port = value
                    .parse::<u16>()
                    .map_err(|_| format!("Invalid --port value: {}", value))?;
            }
            "-h" | "--help" => return Ok(None),
            other => return Err(format!("Unknown argument: {}", other)),
        }
        i += 1;
    }
    Ok(Some(Args { host, port }))
}

fn main() -> std::io::Result<()> {
    load_env_file(".env");
    let args: Vec<String> = std::env::args().collect();
    let parsed = match parse_args(&args) {
        Ok(Some(p)) => p,
        Ok(None) => {
            print!("{}", USAGE);
            return Ok(());
        }
        Err(message) => {
            eprintln!("{}\n{}", message, USAGE);
            std::process::exit(2);
        }
    };
    let config = Config::from_env();
    eprintln!(
        "aif-backend {} starting: backend={} model={}",
        aif_backend::config::VERSION,
        config.backend,
        config.model
    );
    let state = Arc::new(ServerState {
        cache: aif_backend::cache::Cache::new(config.cache_ttl),
        config,
    });
    serve(state, &parsed.host, parsed.port)
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
        assert_eq!((got.host, got.port), ("127.0.0.1".to_string(), 9000));
    }

    #[test]
    fn help_returns_none() {
        assert_eq!(parse_args(&args(&["aif-backend", "--help"])), Ok(None));
    }
}
