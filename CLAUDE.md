# CLAUDE.md

This repository is a Rust-served local WebUI/API for estimating cow weight.
The browser assets live in `web/`; the threaded Rust server lives in
`backend/`; Python in `aif/` is a reusable estimator/configuration library.
`app.py` launches the backend and `gui.py` is only a browser-opening
compatibility launcher.

Follow [AGENTS.md](AGENTS.md) and [CONTRIBUTING.md](CONTRIBUTING.md). In
particular:

- keep the frontend dependency-free HTML/CSS/vanilla JavaScript;
- keep HTTP routes and configuration behavior in Rust;
- preserve existing API clients, image validation, body limits, request IDs,
  CORS, error codes, and deterministic fallback parity;
- derive per-request overrides from a cloned `Config`, never process env;
- never log/return API keys or persist them in browser storage/history;
- use explicit static/demo routes only, never arbitrary filesystem serving;
- render dynamic browser values with DOM APIs/textContent, not innerHTML;
- append a dated entry to `commits.md` at the end of every session.

Build and test with:

```bash
cargo build --release --manifest-path backend/Cargo.toml
cargo test --manifest-path backend/Cargo.toml
python -m unittest discover -s tests -v
ruff check .
```
