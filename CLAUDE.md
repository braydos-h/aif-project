# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.
After every session add to the commits.md file with stuff you have done and the date and time when you finished working.
For conventions, repository layout, and step-by-step guides for common changes (new backend, new endpoint, GUI work), read [CONTRIBUTING.md](CONTRIBUTING.md).
## What this is

A project that estimates a cow's weight from an image payload, shipped as a Rust HTTP service (`backend/`, crate `aif-backend`) and a Python Tkinter desktop GUI (`aif/gui.py`). The Python package (`aif/`) is GUI + estimator only — the HTTP server is Rust. The Python side is stdlib-only (`urllib`, `hashlib`, `base64`, `re`, `logging`, `tkinter`); the Rust side has three dependencies (`ureq` with rustls TLS, `serde_json`, `sha2`) — no async runtime. Configuration is loaded from a `.env` file at startup via a stdlib-only loader (`aif/config.py:_load_env_file`); environment variables already set take precedence over `.env`. Project metadata and the ruff config live in `pyproject.toml`. `app.py` at the repo root is a thin backward-compatible launcher that spawns the Rust binary, so `python app.py` keeps working; `gui.py` stays a Python wrapper over `aif.gui`.

Module layout:

- `aif/config.py` — constants, defaults, `.env` loader (`_load_env_file`), `setup_logging`.
- `aif/estimator.py` — `CowWeightEstimator`, `ImageValidationError`, image validation helpers (used in-process by the GUI only).
- `aif/gui.py` — `CowWeightApp` (Tkinter).
- `backend/` — the Rust crate: `src/config.rs` (.env loader + defaults), `src/validate.rs` (base64 + magic bytes), `src/parse.rs` (weight extraction), `src/fallback.rs` (deterministic estimate), `src/cache.rs` (TTL cache), `src/ollama.rs` (Ollama client + retry), `src/http.rs` (threaded HTTP/1.1 server), `src/main.rs` (entry point).

## Backend selection

Both the GUI estimator and the Rust server pick a backend (constructor arg / env / `AIF_AI_BACKEND`, default `ollama`):
- **`ollama`** (default) — POSTs to Ollama Cloud (`AIF_OLLAMA_URL`, default `https://ollama.com/api/generate`) with the `OLLAMA_API_KEY` bearer token and model `AIF_AI_MODEL` (default `gemma4:31b-cloud` — the direct-cloud tag; the local-runtime tag `gemma4:31b` is a 20GB download and will not work against the cloud endpoint), sending the image as base64 and a text prompt. The default `DEFAULT_PROMPT` asks the model for a JSON object `{weight_kg, confidence, breed, body_condition_score}`; the parser extracts the JSON when present (and falls back to `<n> kg` / first-bare-number text extraction when not). The result includes `estimated_weight_kg`, `estimated_weight_lbs`, `source == "ollama"`, the full `model_response`, and `confidence`/`breed`/`body_condition_score` when the model returned JSON. Results are cached keyed by `sha256(base64 image)` with TTL `AIF_CACHE_TTL` (default 300 s, `0` disables). Transient failures (5xx `HTTPError`, network/timeout errors) are retried once after `OLLAMA_RETRY_BACKOFF` (1.0 s); 4xx and non-JSON bodies are not retried. Image bytes are validated against JPEG/PNG/GIF/BMP/WebP magic bytes before being sent to the model, rejected as non-images otherwise.
- **`none`** — deterministic local estimate derived from `sha256(image_reference)` → range 250–900 kg (Rust: `u32::from_be_bytes(digest[..4]) / u32::MAX`, matching Python's `int(hexdigest[:8], 16) / 0xFFFFFFFF`). Stable for a given input, which tests rely on. Reports `source == "local_fallback"` and an empty `model_response`. Also includes `estimated_weight_lbs`.

When both exist, the Rust and Python implementations of the same logic must stay behaviorally identical (parity tests exist for the `none` backend's math).

## Commands

Run the server (listens on `127.0.0.1:8080`; `python app.py` spawns the Rust binary — build it first):
```bash
cargo build --release --manifest-path backend/Cargo.toml
python app.py
```

Run the GUI:
```bash
python gui.py
```

Run Rust tests:
```bash
cargo test --manifest-path backend/Cargo.toml
```

Run the full test suite (the HTTP tests spawn the release binary — build it first):
```bash
python -m unittest discover -s tests -v
```

Run a single test:
```bash
python -m unittest tests.test_estimator.OllamaEstimatorTests.test_cloud_endpoint_uses_bearer_token -v
```

Lint with ruff (configured in `pyproject.toml`):
```bash
ruff check .
ruff check --fix .
```

## Architecture

The core has two layers:

- **`CowWeightEstimator`** (`aif/estimator.py`) — the GUI's estimation logic, decoupled from HTTP. `estimate()` dispatches on the configured backend: `ollama` (default) or `none`. Returns a dict including `estimated_weight_kg`, `estimated_weight_lbs`, `source`, `prompt_used`, and `model_response` (raw model text, empty for the fallback), plus `confidence`/`breed`/`body_condition_score` when the model returned JSON.

- **Rust `aif-backend`** (`backend/`) — replaces the old `aif/server.py` HTTP API. A hand-rolled HTTP/1.1 server on `std::net::TcpListener` with one thread per connection, so slow Ollama requests never block each other (the old Python `http.server` was single-threaded). Valid routes: `POST /estimate-weight` (the estimator), `GET /health` (liveness: status/backend/model/request_id), `GET /` or `/info` (name/version/endpoints), `OPTIONS` (CORS preflight → 204); anything else returns 404. The POST handler accepts `image_url` **or** `image_base64` plus an optional `prompt` (defaults to `DEFAULT_PROMPT`). `ImageValidationError` surfaces as `400 invalid_image`; other estimator errors surface as `502 Bad Gateway`; bad input as `400`. Every error response carries a machine-readable `code` field (`missing_body`, `invalid_json`, `missing_image`, `invalid_image`, `not_found`, `estimation_failed`) alongside the human `error` message. Every response (success or error) includes a per-request `request_id` (8-hex) in the JSON body and the `x-request-id` header, and CORS headers (`Access-Control-Allow-Origin: *`). Requests are read as a single Content-Length-delimited body (no keep-alive, no chunked encoding — intentional, matching the old server's contract). Logs go to stderr, tagged with the request id.

The Rust binary is launched by `app.py` (`find_binary` in `app.py`, override with `AIF_BACKEND_BIN`); flags are `--host` and `--port` (`--port 0` prints the bound port to stdout, used by the test suite). The GUI does not use the Rust server — it constructs a fresh `CowWeightEstimator` per request from the UI's backend/model/URL fields.

`aif/gui.py` is a Tkinter desktop app (`CowWeightApp`) that lets the user pick an image file, edit the prompt, switch backend/model at runtime, and view the weight, the model's full reply, and a session history. Image preview uses Pillow if importable, otherwise degrades to showing the filename and byte size. Keyboard: `Enter` estimates (in the prompt box, use `Ctrl+Enter` to keep newlines free). The `cows/` folder holds demo images used by the "Test demo cows" button.

## Tests

Tests live in `tests/` (Python) and `backend/src/` (Rust `#[cfg(test)]` units), split by concern:

- `tests/test_server.py` spawns the compiled Rust binary on a free port (env `AIF_BACKEND_BIN` overrides the path) and exercises it over real HTTP, so it validates the full request/response path including status codes and JSON serialization. The HTTP tests force `backend="none"` so they use the deterministic fallback (no running Ollama needed) and assert `source == "local_fallback"`. A 502 test forces `backend="ollama"` with no key and asserts `code == "estimation_failed"`; a 400 test forces `backend="ollama"` with a key but non-image bytes and asserts `code == "invalid_image"`. Other HTTP tests cover `GET /health`, `GET /`, 404 on unknown GET, `OPTIONS` preflight → 204, CORS header on success, `request_id` in header+body, and concurrent requests.
- `tests/test_estimator.py` unit-tests the Python estimator: the Ollama-specific helpers (`_extract_weight_from_text`, `_to_base64_image` incl. magic-byte validation, `_parse_structured_response`), backend selection logic, `CacheTests` (cache hit/disabled/expiry), and `RetryTests` (retry-on-URL-error-then-succeed, no-retry-on-4xx, retry-on-5xx-then-raise). The live Ollama network path is not exercised.
- Rust unit tests live next to each module in `backend/src/` (config parsing, base64/magic-byte validation, structured/text weight parsing, fallback determinism, cache hit/disabled/expiry, retry details).
- `tests/test_gui.py` has `GuiSmokeTests`, which builds the Tk root + `CowWeightApp` and destroys it, catching import/layout regressions in `aif/gui.py` without an interactive display, plus an import check for the `gui.py` entry wrapper.

Tests use a minimal real PNG (`_png_bytes()`) for any input that must pass image validation. When mocking module-level names, patch `aif.estimator.logger` (not the root `aif` logger).
