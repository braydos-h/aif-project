# Cow Weight Estimator

Cow Weight Estimator is a small local web application that turns a cow image
into a rough weight estimate. The Rust backend serves the browser UI and the
JSON API from one process. No Node.js, npm, Python web framework, or external
CDN is required.

This is an AI estimate, not a replacement for a scale, veterinary advice, or
an official livestock record.

## Quick start

Build the Rust server, then start the application:

```powershell
cargo build --release --manifest-path backend/Cargo.toml
python app.py
```

Open <http://127.0.0.1:8080/>. The launcher prints the WebUI and API URLs.

For a compatibility shortcut that opens the browser automatically:

```powershell
python gui.py
```

`gui.py`, `start_gui.bat`, and `start_gui.ps1` are thin launchers only; the
supported UI is the Rust-served WebUI. The direct Rust command is also useful:

```powershell
backend\target\release\aif-backend.exe --host 127.0.0.1 --port 8080
```

CLI usage: `aif-backend [--host HOST] [--port PORT]`. Both `--flag value`
and `--flag=value` forms work; `--help` prints usage and exits 0, while
unknown flags or missing/invalid values print usage and exit nonzero instead
of silently using defaults.

## WebUI

The browser app supports:

- batch image selection (JPEG, PNG, WebP, BMP, and GIF) with type and size
  checks;
- one **Estimate Weight** button that estimates each selected image in turn,
  showing per-image progress and a final succeeded/failed count;
- a latest-answer area plus a session-only history of the last 20 successful
  estimates;
- live status updates, dark-mode support, and a responsive single-column
  layout.

Selected files are not uploaded until **Estimate Weight** is pressed. Backend,
model, and prompt come from the server defaults. The browser stores nothing —
no `localStorage`, no API keys, no base64 images in history. Dynamic text is
rendered as text, not HTML.

## Architecture

```text
web/index.html, styles.css, app.js
              │ same-origin fetch
              ▼
Rust aif-backend ── /estimate-weight ── Ollama Cloud or local fallback
       ├──────────── /health, /info
       └──────────── /demo-cows and compiled static assets
```

The Python package remains a dependency-free estimator/configuration library
for scripts and tests. It is no longer a desktop UI. The Rust implementation
is the main application server and keeps the existing deterministic fallback,
image validation, caching, retry behavior, request IDs, and error codes.

## API reference

### Static and information routes

| Method | Path | Result |
| --- | --- | --- |
| `GET` | `/` | WebUI HTML |
| `GET` | `/styles.css` | WebUI stylesheet |
| `GET` | `/app.js` | WebUI JavaScript |
| `GET` | `/info` | Safe JSON application/configuration information |
| `GET` | `/health` | Liveness and effective backend/model |
| `GET` | `/demo-cows` | Controlled list of bundled demo images |
| `GET` | `/demo-cows/{id}` | One approved bundled demo image (`1`, `2`, or `3`) |

`/info` includes `backend`, `model`, `ollama_url`, `default_prompt`, version,
endpoints, and an `ollama_configured` boolean. It never returns
`OLLAMA_API_KEY`.

### `POST /estimate-weight`

Send either an image URL or base64/data-URI image. `prompt` and all runtime
configuration fields are optional; omitted fields use the server's
environment/`.env` defaults.

```json
{
  "image_base64": "iVBORw0KGgoAAAANSUhEUgAA...",
  "prompt": "Estimate this cow in kilograms and return JSON.",
  "backend": "ollama",
  "model": "gemma4:31b-cloud",
  "ollama_url": "https://ollama.com/api/generate",
  "ollama_api_key": "request-only-key"
}
```

Supported runtime values are `ollama` and `none`. Runtime values are validated
without changing process environment variables or global server configuration.
The API key is used only for that request, is never logged, and is never
returned in a response.

A successful response includes:

```json
{
  "estimated_weight_kg": 612.0,
  "estimated_weight_lbs": 1349.2,
  "source": "ollama",
  "model": "gemma4:31b-cloud",
  "prompt_used": "Estimate this cow in kilograms and return JSON.",
  "model_response": "{\"weight_kg\":612,\"confidence\":0.82}",
  "confidence": 0.82,
  "breed": "Angus",
  "body_condition_score": 6.0,
  "request_id": "bce5028c"
}
```

`model`, `confidence`, `breed`, and `body_condition_score` are present when
the selected backend returns them. `source` is `ollama` or
`local_fallback`. Every JSON response also sends the same request ID in the
`x-request-id` header and includes `Access-Control-Allow-Origin: *` for
existing API clients.

Status codes are `200` for success, `400` for missing/malformed input,
invalid images, or invalid runtime options, `404` for unknown routes, and
`502` for an estimator/Ollama failure. Error bodies contain `error`, `code`,
and `request_id`; the WebUI maps these to user-friendly messages.

Example using the deterministic backend:

```powershell
$body = @{ image_base64 = "iVBORw0KGgoAAAANSUhEUgAA..."; backend = "none" } | ConvertTo-Json
Invoke-RestMethod http://127.0.0.1:8080/estimate-weight `
  -Method Post -ContentType "application/json" -Body $body
```

## Configuration

Copy `.env.example` to `.env` for server defaults. Environment variables that
are already set take precedence over `.env`.

| Variable | Default | Purpose |
| --- | --- | --- |
| `AIF_AI_BACKEND` | `ollama` | `ollama` or deterministic `none` |
| `AIF_AI_MODEL` | `gemma4:31b-cloud` | Ollama model name |
| `AIF_OLLAMA_URL` | `https://ollama.com/api/generate` | Ollama-compatible generate endpoint |
| `OLLAMA_API_KEY` | empty | Server-side Ollama bearer token; never returned |
| `AIF_CACHE_TTL` | `300` | Ollama result cache TTL in seconds; `0` disables |

Ollama Cloud requires an API key. The `none` backend makes a deterministic
SHA-256-derived estimate in the range 250–900 kg and performs no network
request, making it suitable for local demos and tests.

## Development and tests

Requirements are Python 3.11+ for the launcher/tests and Rust stable for the
backend. The frontend is plain HTML/CSS/JavaScript and has no package install
step.

```powershell
cargo fmt --manifest-path backend/Cargo.toml
cargo test --manifest-path backend/Cargo.toml
cargo build --release --manifest-path backend/Cargo.toml
python -m unittest discover -s tests -v
ruff check .
```

The HTTP tests start the release Rust binary on a free local port. Set
`AIF_BACKEND_BIN` to override the binary path when needed.

## Repository layout

```text
backend/src/       Rust HTTP server, config, validation, parsing, Ollama, cache
web/               Rust-served WebUI assets
cows/              Approved bundled demo images
aif/config.py      Python defaults and .env loader
aif/estimator.py   Reusable Python estimator and image helpers
app.py             Rust backend launcher
gui.py             Legacy launcher that opens the WebUI
tests/             Python HTTP, estimator, config, launcher, and WebUI tests
```
