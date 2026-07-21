# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.
After every session add to the commits.md file with stuff you have done and the date and time when you finished working.
## What this is

A single-file Python HTTP service (`app.py`) that estimates a cow's weight from an image payload. No external dependencies — standard library only (`http.server`, `urllib`, `hashlib`).

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

- **`CowWeightEstimator`** — the estimation logic, decoupled from HTTP. `estimate()` dispatches on whether an AI backend is configured:
  - If `api_url` is set (constructor arg or `AIF_AI_API_URL` env var), it POSTs `{image, prompt}` to that API and extracts a weight from one of `weight_kg` / `estimate_kg` / `estimated_weight_kg` in the response. An optional `AIF_AI_API_KEY` adds an `X-API-Key` header.
  - Otherwise it falls back to a **deterministic** local estimate derived from `sha256(image_reference)` → range 250–900 kg. The fallback is stable for a given input, which tests rely on.

- **`EstimateHandler`** — `BaseHTTPRequestHandler` subclass. Only `POST /estimate-weight` is valid; anything else returns 404. Accepts `image_url` **or** `image_base64` plus an optional `prompt` (defaults to `DEFAULT_PROMPT`). Estimator failures surface as `502 Bad Gateway`; bad input as `400`.

The estimator is instantiated as a class attribute on the handler (`EstimateHandler.estimator`), so it's created once at import time using env vars present at startup — changing `AIF_AI_API_URL` after launch has no effect on a running server.

## Tests

`tests/test_app.py` boots a real `create_server(port=0)` on a background thread and exercises it over real HTTP (not in-process handler calls), so it validates the full request/response path including status codes and JSON serialization. The fallback-path test asserts `source == "local_fallback"`; it does not test the AI path.