//! Entry point: build the config from env / `.env`, then serve HTTP.

use std::sync::Arc;

use aif_backend::args::{parse_args, USAGE};
use aif_backend::config::{load_env_file, Config};
use aif_backend::http::{serve, ServerState};

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
    let state = Arc::new(ServerState::new(config));
    serve(state, &parsed.host, parsed.port)
}
