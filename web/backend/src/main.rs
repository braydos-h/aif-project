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
    // Boot recovery: requeue interrupted jobs (worker does this too, but
    // run it even when the worker is disabled so state never sticks), then
    // sweep expired/orphan retained photos.
    {
        let now = aif_backend::time_util::rfc3339(aif_backend::time_util::unix_now());
        match state.db.requeue_active_jobs(&now) {
            Ok(n) if n > 0 => eprintln!("requeued {} interrupted job(s)", n),
            Ok(_) => {}
            Err(e) => eprintln!("job requeue failed: {}", e),
        }
        match aif_backend::photos::sweep_photos(&state.db, &state.config.data_dir, &now) {
            Ok((expired, orphans, missing)) if expired + orphans + missing > 0 => eprintln!(
                "photo sweep: {} expired, {} orphans, {} missing",
                expired, orphans, missing
            ),
            Ok(_) => {}
            Err(e) => eprintln!("photo sweep failed: {}", e),
        }
    }
    aif_backend::jobs::spawn_worker(Arc::clone(&state));
    serve(state, &parsed.host, parsed.port)
}
