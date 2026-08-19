//! Entry point: build the config from env / `.env`, then serve HTTP.
//!
//! Flags:
//! - `--port N` (default 8080, `0` = ephemeral, printed to stdout)
//! - `--host H` (default 127.0.0.1)

use std::sync::Arc;

use aif_backend::config::{load_env_file, Config};
use aif_backend::http::{ServerState, serve};

fn main() -> std::io::Result<()> {
    // Same .env contract as the Python package: values already in the
    // environment win; otherwise read .env from the repository root.
    load_env_file(".env");

    let args: Vec<String> = std::env::args().collect();
    let mut host = "127.0.0.1".to_string();
    let mut port: u16 = 8080;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--host" if i + 1 < args.len() => {
                host = args[i + 1].clone();
                i += 1;
            }
            "--port" if i + 1 < args.len() => {
                port = args[i + 1].parse().unwrap_or(8080);
                i += 1;
            }
            _ => {}
        }
        i += 1;
    }

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
    serve(state, &host, port)
}
