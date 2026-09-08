# AGENTS.md

This file provides guidance to Codex working in this repository.

After every session, append a dated summary to `commits.md`. Whenever an
agent adds or changes code, it **must append** a new entry with the date and
time. Only append; never overwrite, rewrite, or delete existing entries.

Read [CONTRIBUTING.md](CONTRIBUTING.md) for conventions and common changes.

## What this is

Cow Weight Estimator is a Rust HTTP server that also serves a static browser
WebUI. The Python package is a reusable, dependency-free estimator/config
library and is not the application UI. `app.py` starts the Rust binary;
`gui.py` is a compatibility launcher that opens the same WebUI.

Important paths:

- `web/index.html`, `web/styles.css`, `web/app.js` — browser UI.
- `backend/src/http.rs` — threaded HTTP/1.1 server, explicit static/demo
  routes, API dispatch, runtime option validation, and error responses.
- `backend/src/config.rs` — defaults, `.env` loading, and server config.
- `backend/src/validate.rs` — base64/data-URI and image magic-byte validation.
- `backend/src/parse.rs` — structured JSON and free-text weight parsing.
- `backend/src/fallback.rs` — deterministic SHA-256-derived offline estimate.
- `backend/src/cache.rs` — in-memory TTL cache.
- `backend/src/ollama.rs` — Ollama client, bearer auth, and retry policy.
- `aif/estimator.py` — reusable Python estimator and image helpers.
- `aif/config.py` — Python defaults and stdlib `.env` loader.
- `tests/test_server.py` — real HTTP tests against the Rust binary.

## Backend behavior

The supported backends are `ollama` and `none`. Ollama uses
`AIF_OLLAMA_URL`, `AIF_AI_MODEL`, and `OLLAMA_API_KEY`; the default model is
`gemma4:31b-cloud`. The `none` backend returns a deterministic 250–900 kg
placeholder with `source == "local_fallback"` and performs no network call.
Rust and Python fallback math must remain behaviorally identical.

The server routes are:

- `GET /` → WebUI HTML.
- `GET /styles.css`, `GET /app.js` → compile-time static assets.
- `GET /info` → safe application/configuration JSON; never return an API key.
- `GET /health` → liveness, backend, model, and safe configuration status.
- `GET /demo-cows`, `GET /demo-cows/{id}` → controlled bundled demo images.
- `POST /estimate-weight` → existing estimator API.
- `OPTIONS` → CORS preflight; unknown routes return structured 404 JSON.

Static and demo routes are explicit. Never add arbitrary filesystem serving or
build a filesystem path from a URL segment. Keep the existing body limit,
image validation, request IDs, CORS headers, meaningful error codes, and
header-safe responses.

The estimate request accepts the existing `image_url`, `image_base64`, and
`prompt` fields plus optional `backend`, `model`, `ollama_url`, and
`ollama_api_key`. Optional values must be type/size/allow-list validated. Use
a cloned request-specific `Config`; never mutate process environment variables
or shared server state. Never log API keys or return them in errors/info.

## Commands

```bash
cargo build --release --manifest-path backend/Cargo.toml
python app.py
```

Then visit `http://127.0.0.1:8080/`.

```bash
cargo test --manifest-path backend/Cargo.toml
python -m unittest discover -s tests -v
ruff check .
```

The Python HTTP tests require the release backend binary first and honor
`AIF_BACKEND_BIN`.

## Testing/security expectations

Dynamic browser values, including model output, filenames, errors, and request
IDs, must be rendered with `textContent`/DOM properties, never `innerHTML`.
Do not persist API keys or base64 images in browser storage/history. Preserve
focus states and live status messages, and keep the UI usable on mobile and
dark mode.

Append the session summary to `commits.md` before finishing.
