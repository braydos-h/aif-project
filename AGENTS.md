# AGENTS.md

This file provides guidance to Codex (Codex.ai/code) when working with code in this repository.
After every session add to the commits.md file with stuff you have done and the date and time when you finished working.
## What this is

A single-file Python HTTP service (`app.py`) that estimates a cow's weight from an image payload. No external dependencies — standard library only (`http.server`, `urllib`, `hashlib`, `base64`, `re`). Configuration is loaded from a `.env` file at startup via a stdlib-only loader (`_load_env_file`); environment variables already set take precedence over `.env`.

## Backend selection

`CowWeightEstimator` picks a backend (constructor arg or `AIF_AI_BACKEND` env, default `ollama`):
- **`ollama`** (default) — POSTs to Ollama Cloud (`AIF_OLLAMA_URL`, default `https://ollama.com/api/generate`) with the `OLLAMA_API_KEY` bearer token and model `AIF_AI_MODEL` (default `gemma4:31b`), sending the image as base64 and a text prompt. It extracts the weight from the model's free-form text reply (`<n> kg` preferred, else the first number) and reports `source == "ollama"`.
- **`none`** — deterministic local estimate derived from `sha256(image_reference)` → range 250–900 kg. The fallback is stable for a given input, which tests rely on. Reports `source == "local_fallback"`.

## Commands

Run the server (listens on `127.0.0.1:8080`):
```bash
python app.py
```

Run the full test suite:
```bash
python -m unittest discover -s tests -v
```

Run a single test:
```bash
python -m unittest tests.test_app.EstimateApiTests.test_estimate_weight_with_image_url -v
```

There is no linter/formatter configured.

## Architecture

The service has two layers, both in `app.py`:

- **`CowWeightEstimator`** — the estimation logic, decoupled from HTTP. `estimate()` dispatches on the configured backend (see "Backend selection" above): `ollama` (default) or `none`.

- **`EstimateHandler`** — `BaseHTTPRequestHandler` subclass. Only `POST /estimate-weight` is valid; anything else returns 404. Accepts `image_url` **or** `image_base64` plus an optional `prompt` (defaults to `DEFAULT_PROMPT`). Estimator failures surface as `502 Bad Gateway`; bad input as `400`.

The estimator is instantiated as a class attribute on the handler (`EstimateHandler.estimator`), so it's created once at import time using env vars / `.env` present at startup — changing them after launch has no effect on a running server.

## Tests

`tests/test_app.py` boots a real `create_server(port=0)` on a background thread and exercises it over real HTTP (not in-process handler calls), so it validates the full request/response path including status codes and JSON serialization. The HTTP tests force `backend="none"` so they use the deterministic fallback (no running Ollama needed) and assert `source == "local_fallback"`. A separate `OllamaEstimatorTests` class unit-tests the Ollama-specific helpers (`_extract_weight_from_text`, `_to_base64_image`) and backend selection logic; the live Ollama network path is not exercised.