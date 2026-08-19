# AGENTS.md

This file provides guidance to Codex (Codex.ai/code) when working with code in this repository.
After every session add to the commits.md file with stuff you have done and the date and time when you finished working.
For conventions, repository layout, and step-by-step guides for common changes (new backend, new endpoint, GUI work), read [CONTRIBUTING.md](CONTRIBUTING.md).
## What this is

A single-file Python HTTP service (`app.py`) plus a Tkinter desktop GUI (`gui.py`) that estimate a cow's weight from an image payload. No external runtime dependencies — standard library only (`http.server`, `urllib`, `hashlib`, `base64`, `re`, `logging`, `tkinter`, plus `time`/`uuid`/`json`). Configuration is loaded from a `.env` file at startup via a stdlib-only loader (`_load_env_file`); environment variables already set take precedence over `.env`. Project metadata and the ruff config live in `pyproject.toml`.

## Backend selection

`CowWeightEstimator` picks a backend (constructor arg or `AIF_AI_BACKEND` env, default `ollama`):
- **`ollama`** (default) — POSTs to Ollama Cloud (`AIF_OLLAMA_URL`, default `https://ollama.com/api/generate`) with the `OLLAMA_API_KEY` bearer token and model `AIF_AI_MODEL` (default `gemma4:31b-cloud` — the direct-cloud tag; the local-runtime tag `gemma4:31b` is a 20GB download and will not work against the cloud endpoint), sending the image as base64 and a text prompt. The default `DEFAULT_PROMPT` asks the model for a JSON object `{weight_kg, confidence, breed, body_condition_score}`; `_parse_structured_response` extracts the JSON when present (and falls back to `<n> kg` / first-bare-number text extraction when not). The result includes `estimated_weight_kg`, `estimated_weight_lbs`, `source == "ollama"`, the full `model_response`, and `confidence`/`breed`/`body_condition_score` when the model returned JSON. Results are cached per-estimator keyed by `sha256(base64 image)` with TTL `AIF_CACHE_TTL` (default 300 s, `0` disables). Transient failures (5xx `HTTPError`, `URLError`, `TimeoutError`) are retried once after `OLLAMA_RETRY_BACKOFF` (1.0 s); 4xx and `JSONDecodeError` are not retried. Image bytes are validated against JPEG/PNG/GIF/BMP/WebP magic bytes in `_to_base64_image`, raising `ImageValidationError` for non-images.
- **`none`** — deterministic local estimate derived from `sha256(image_reference)` → range 250–900 kg. The fallback is stable for a given input, which tests rely on. Reports `source == "local_fallback"` and an empty `model_response`. Also includes `estimated_weight_lbs`.

## Commands

Run the server (listens on `127.0.0.1:8080`):
```bash
python app.py
```

Run the GUI:
```bash
python gui.py
```

Run the full test suite:
```bash
python -m unittest discover -s tests -v
```

Run a single test:
```bash
python -m unittest tests.test_app.EstimateApiTests.test_estimate_weight_with_image_url -v
```

Lint with ruff (configured in `pyproject.toml`):
```bash
ruff check .
ruff check --fix .
```

## Architecture

The service has two layers, both in `app.py`:

- **`CowWeightEstimator`** — the estimation logic, decoupled from HTTP. `estimate()` dispatches on the configured backend (see "Backend selection" above): `ollama` (default) or `none`. Returns a dict including `estimated_weight_kg`, `estimated_weight_lbs`, `source`, `prompt_used`, and `model_response` (raw model text, empty for the fallback), plus `confidence`/`breed`/`body_condition_score` when the model returned JSON.

- **`EstimateHandler`** — `BaseHTTPRequestHandler` subclass. Valid routes: `POST /estimate-weight` (the estimator), `GET /health` (liveness: status/backend/model/request_id), `GET /` or `/info` (name/version/endpoints), `OPTIONS` (CORS preflight → 204); anything else returns 404. The POST handler accepts `image_url` **or** `image_base64` plus an optional `prompt` (defaults to `DEFAULT_PROMPT`). `ImageValidationError` surfaces as `400 invalid_image`; other estimator `ValueError`s surface as `502 Bad Gateway`; bad input as `400`. Every error response carries a machine-readable `code` field (`missing_body`, `invalid_json`, `missing_image`, `invalid_image`, `not_found`, `estimation_failed`) alongside the human `error` message. Every response (success or error) includes a per-request `request_id` (8-char uuid hex) in the JSON body and the `x-request-id` header, and CORS headers (`Access-Control-Allow-Origin: *`). Access and error logs go through the `aif` logger, tagged with the request id.

The estimator is injected into the server via `create_server(host, port, estimator=None)` and read off `self.server.estimator` in the handler — no shared class attribute. Default estimator (when `None`) is built once from env vars / `.env` present at startup; changing them after launch has no effect on a running server.

`gui.py` is a Tkinter desktop app (`CowWeightApp`) that lets the user pick an image file, edit the prompt, switch backend/model at runtime, and view the weight, the model's full reply, and a session history. It constructs a fresh `CowWeightEstimator` per request from the UI-selected backend/model. Image preview uses Pillow if importable, otherwise degrades to showing the filename and byte size. Keyboard: `Enter` estimates (in the prompt box, use `Ctrl+Enter` to keep newlines free).

## Tests

`tests/test_app.py` boots a real `create_server(port=0, estimator=...)` on a background thread and exercises it over real HTTP (not in-process handler calls), so it validates the full request/response path including status codes and JSON serialization. The HTTP tests force `backend="none"` so they use the deterministic fallback (no running Ollama needed) and assert `source == "local_fallback"`. A 502 test forces `backend="ollama"` with no key and asserts `code == "estimation_failed"`; a 400 test forces `backend="ollama"` with a key but non-image bytes and asserts `code == "invalid_image"`. Other HTTP tests cover `GET /health`, `GET /`, 404 on unknown GET, `OPTIONS` preflight → 204, CORS header on success, and `request_id` in header+body. A separate `OllamaEstimatorTests` class unit-tests the Ollama-specific helpers (`_extract_weight_from_text`, `_to_base64_image` incl. magic-byte validation, `_parse_structured_response`) and backend selection logic; the live Ollama network path is not exercised. `CacheTests` covers cache hit/disabled/expiry; `RetryTests` covers retry-on-URL-error-then-succeed, no-retry-on-4xx, and retry-on-5xx-then-raise. `GuiSmokeTests` builds the Tk root + `CowWeightApp` and destroys it, catching import/layout regressions in `gui.py` without an interactive display. Tests use a minimal real PNG (`_png_bytes()`) for any input that must pass image validation.