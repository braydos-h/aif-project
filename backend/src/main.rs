//! Entry point: build the config from env / `.env`, then serve HTTP.

use std::sync::Arc;

use aif_backend::args::{parse_args, USAGE};
use aif_backend::config::{load_env_file, validate_production_config, Config};
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
    if let Err(message) = validate_production_config(&config) {
        eprintln!("production configuration error: {}\n{}", message, USAGE);
        std::process::exit(2);
    }
    // Operator administration runs against the database and exits.
    if let Some(command) = parsed.command {
        let code = aif_backend::cli::run_admin(&command, &config);
        std::process::exit(code);
    }
    // First-run bootstrap: empty database + AIF_OPERATOR_EMAIL prints a
    // one-time operator invite link for private relay.
    aif_backend::cli::maybe_bootstrap(&config);
    eprintln!(
        "aif-backend {} starting: backend={} model={} auth_required={} production={} data_dir={}",
        aif_backend::config::VERSION,
        config.backend,
        config.model,
        config.require_auth,
        config.production,
        config.data_dir,
    );
    let state = Arc::new(
        ServerState::new(config)
            .map_err(|e| std::io::Error::other(format!("cannot start server: {}", e)))?,
    );
    serve(state, &parsed.host, parsed.port)
}
