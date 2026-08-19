# Cow Weight Estimator

Estimate a cow's weight from a photo. Ships as a dependency-free Python HTTP
API (`app.py`) and a Tkinter desktop app (`gui.py`). Standard library only —
no `pip install` required to run.

New here? Read [CONTRIBUTING.md](CONTRIBUTING.md) for conventions, the
repository layout, and step-by-step guides for common changes.

The default backend is **Ollama Cloud**, which sends the image to a vision
model and extracts the weight from its reply. A deterministic offline fallback
is included for testing and air-gapped use.

---

## Highlights

- **Two interfaces, one estimator.** A JSON HTTP API for automation and a
  double-clickable Windows GUI for interactive use. Both share the same
  `CowWeightEstimator` and `.env` configuration.
- **Zero runtime dependencies.** Pure standard library (`http.server`,
  `urllib`, `hashlib`, `base64`, `re`, `logging`, `tkinter`). Loads its own
  `.env` at startup via a stdlib-only loader, so no `python-dotenv` either.
  Pillow is optional — if installed, the GUI shows an image preview; if not,
  it degrades to showing the filename and byte size.
- **Pluggable backend.** `ollama` (vision model via Ollama Cloud, default) or
  `none` (stable SHA-256-derived local estimate, no network). The GUI exposes
  a backend/model selector so you can switch at runtime without editing `.env`.
- **Robust input handling.** Accepts `image_url` or raw/base64 `image_base64`,
  including `data:` URIs. Strips WebP data-URI prefixes even when Windows labels
  them `application/octet-stream`.
- **Structured errors.** Every error response carries a machine-readable `code`
  field (`missing_image`, `invalid_image`, `estimation_failed`, …) alongside the
  human message. Every response (success or error) also carries a `request_id`
  (8-char hex) echoed in both the `x-request-id` header and the JSON body, and
  flows through the `aif` logger so you can trace a request end to end.
- **CORS-friendly.** All responses include `Access-Control-Allow-Origin: *`,
  and `OPTIONS` preflight returns `204 No Content` — a browser page can call the
  API directly.
- **Smarter estimates.** The default prompt asks the model for a JSON object
  with `weight_kg`, `confidence` (0–1), `breed`, and `body_condition_score`
  (1–9). When the model returns JSON, those fields are added to the response;
  when it doesn't, the estimator falls back to the existing `<n> kg` / first-
  number text extraction. Both backends also return `estimated_weight_lbs`
  alongside `estimated_weight_kg`.
- **In-memory cache.** Repeated requests for the same image return instantly
  from cache (keyed by `sha256` of the base64 image, TTL configurable via
  `AIF_CACHE_TTL`, default 300 s, `0` disables).
- **Retry with backoff.** Transient Ollama failures (5xx, network/timeout
  errors) are retried once after a 1 s backoff. 4xx errors and unparseable
  responses are not retried.
- **Image validation.** Image bytes are checked against JPEG/PNG/GIF/BMP/WebP
  magic bytes before being sent to the model; non-images are rejected with
  `400 invalid_image` instead of wasting a model call.
- **Tested.** Full HTTP request/response suite (real server on a background
  thread, not in-process calls) plus unit tests for weight extraction, backend
  selection, bearer-token auth, structured-response parsing, caching, retry,
  and a GUI smoke test.
- **One-file Windows `.exe`.** A GitHub Actions workflow builds a standalone
  `CowWeightEstimator.exe` with PyInstaller on every published release.

---

## Requirements

- **Python 3.8+** (tested on 3.12 in CI).
- No third-party packages. `tkinter` ships with standard Python installers on
  Windows; on some Linux distros you may need `python3-tk`.
- For the Ollama Cloud backend: an Ollama API key (see [Configuration](#configuration)).

---

## Quick start

### Desktop app (Windows)

Double-click `start_gui.bat`, or run:

```powershell
python gui.py
```

Pick a cow image, tweak the prompt if you like, pick a backend and model, and
select **Estimate weight** (or press `Enter`). The estimate runs on a
background thread so the window stays responsive; a progress bar indicates
activity. The result, the model's full reply, and a session-only history of
the last 20 estimates are shown in the window. A **Copy result** button puts
the weight on the clipboard. No command window, no separate server to keep
running — the GUI calls the estimator directly.

### HTTP API server

```powershell
python app.py
```

Listens on `http://127.0.0.1:8080`. See [API reference](#api-reference).

---

## API reference

### `GET /health`

Liveness probe.

```json
{ "status": "ok", "backend": "ollama", "model": "gemma4:31b-cloud", "request_id": "bce5028c" }
```

### `GET /`

Service info — name, version, and the list of endpoints.

```json
{
  "name": "Cow Weight Estimator",
  "version": "0.1.0",
  "endpoints": ["POST /estimate-weight", "GET /health", "GET /"],
  "request_id": "bce5028c"
}
```

### `OPTIONS /estimate-weight` (and any path)

CORS preflight. Returns `204 No Content` with
`Access-Control-Allow-Origin: *`, `Access-Control-Allow-Methods: POST, GET, OPTIONS`,
and `Access-Control-Allow-Headers: Content-Type`.

### `POST /estimate-weight`

Estimate a cow's weight from an image. Send **either** `image_url` **or**
`image_base64`; `prompt` is optional and defaults to a sensible built-in prompt.

#### Request body

| Field          | Type   | Required                  | Description                                                                            |
| -------------- | ------ | ------------------------- | -------------------------------------------------------------------------------------- |
| `image_url`    | string | one of `image_*` required | URL the server will fetch (HTTPS or HTTP) and encode.                                  |
| `image_base64` | string | one of `image_*` required | Raw base64 image bytes, or a `data:<mime>;base64,...` URI.                              |
| `prompt`       | string | no                        | Override the estimation prompt. Defaults to `DEFAULT_PROMPT` from `app.py`.           |

#### Example: URL

```bash
curl -s http://127.0.0.1:8080/estimate-weight \
  -H "Content-Type: application/json" \
  -d "{\"image_url\": \"https://example.com/cow.jpg\", \"prompt\": \"Estimate this cow's weight in kg.\"}"
```

```json
{
  "estimated_weight_kg": 612.0,
  "estimated_weight_lbs": 1349.2,
  "source": "ollama",
  "model": "gemma4:31b-cloud",
  "prompt_used": "Estimate this cow's weight in kg.",
  "model_response": "{\"weight_kg\": 612, \"confidence\": 0.82, \"breed\": \"Angus\", \"body_condition_score\": 6}",
  "confidence": 0.82,
  "breed": "Angus",
  "body_condition_score": 6.0,
  "request_id": "bce5028c"
}
```

#### Example: base64

```json
{
  "image_base64": "iVBORw0KGgoAAAANSUhEUgAA..."
}
```

#### Response

| Field                  | Type    | Always present | Description                                                              |
| ---------------------- | ------- | -------------- | ------------------------------------------------------------------------ |
| `estimated_weight_kg`  | number  | yes            | The estimated weight in kilograms.                                       |
| `estimated_weight_lbs` | number  | yes            | The estimated weight in pounds (`kg * 2.20462`, rounded to 1 dp).        |
| `source`               | string  | yes            | `ollama` or `local_fallback`.                                            |
| `model`                | string  | ollama only    | The model name used.                                                     |
| `prompt_used`          | string  | yes            | The prompt actually sent (useful when the default was applied).          |
| `model_response`       | string  | yes            | Raw model text (empty string for the local fallback).                    |
| `confidence`           | number  | ollama, JSON   | Model's confidence in the estimate (0–1). Present when the model returns the structured JSON. |
| `breed`                | string  | ollama, JSON   | Model's breed guess. Present when the model returns the structured JSON. |
| `body_condition_score` | number  | ollama, JSON   | 1–9 body condition score. Present when the model returns the structured JSON. |
| `request_id`           | string  | yes            | 8-char hex id, also sent in the `x-request-id` response header.          |

Every response (success or error) also includes the `Access-Control-Allow-Origin: *`
CORS header.

#### Status codes

| Status | Meaning                                                              |
| ------ | -------------------------------------------------------------------- |
| `200`  | Success.                                                             |
| `400`  | Bad request — missing image, invalid JSON, or no body.               |
| `400`  | Bad request — missing image, invalid JSON, no body, or non-image bytes (`invalid_image`). |
| `404`  | Unknown path.                                                         |
| `502`  | Estimator failure — backend unreachable, no key, or unparseable reply. |

Every error response includes a machine-readable `code` field alongside the
human `error` message: `missing_body`, `invalid_json`, `missing_image`,
`invalid_image`, `not_found`, or `estimation_failed`. Every error response also
includes the `request_id` (matching the `x-request-id` header). Any other path
returns `404`.

---

## Configuration

All settings are read from environment variables, with `.env` used as a
fallback at startup. Values already in the environment take precedence over
`.env`. The estimator is constructed once at import time, so changes after the
server has started have no effect — restart to pick up new config.

| Variable           | Default                          | Used by          | Description                                                                 |
| ------------------ | -------------------------------- | ---------------- | --------------------------------------------------------------------------- |
| `AIF_AI_BACKEND`   | `ollama`                         | both             | `ollama` (Ollama Cloud) or `none` (deterministic local fallback).          |
| `AIF_OLLAMA_URL`   | `https://ollama.com/api/generate` | ollama           | Ollama Cloud generate endpoint.                                            |
| `AIF_AI_MODEL`     | `gemma4:31b-cloud`               | ollama           | Model name sent in the request.                                            |
| `OLLAMA_API_KEY`   | _(none)_                         | ollama (cloud)   | Bearer token. Required when `AIF_OLLAMA_URL` points at `ollama.com`.       |
| `AIF_CACHE_TTL`    | `300`                            | ollama           | In-memory cache TTL in seconds. `0` disables caching.                      |

### Setting up Ollama Cloud

1. Create an API key at <https://ollama.com/settings/keys>.
2. Put it in `.env` (or export it in your shell):

   ```ini
   OLLAMA_API_KEY=your-key-here
   ```

3. Ensure `AIF_AI_BACKEND=ollama` (it's the default).

The default model is `gemma4:31b-cloud`, the direct-cloud API name. The
`gemma4:31b` tag is a 20GB local-runtime download and will not work against the
cloud endpoint — use `gemma4:31b-cloud` for this service.

> Do not commit a real API key. `.env` is gitignored; a sanitized
> `.env.example` is committed as a template. Copy it to `.env` and fill in your
> key.

### Offline / no-network mode

Set `AIF_AI_BACKEND=none` for a deterministic, SHA-256-derived estimate in the
range 250–900 kg. Stable for a given input — useful for tests, demos, and
air-gapped environments. Reports `source == "local_fallback"`.

---

## Testing

```powershell
python -m unittest discover -s tests -v
```

The HTTP-level tests boot a real `create_server(port=0, estimator=...)` on a
background thread and exercise it over real sockets (not in-process handler
calls), so they validate status codes, JSON serialization, CORS headers,
request IDs, and the full request path. They force `backend="none"` to stay
deterministic and avoid a running LLM. A 502 test forces `backend="ollama"`
with no key and asserts `code == "estimation_failed"`; a 400 test forces
`backend="ollama"` with a key but non-image bytes and asserts
`code == "invalid_image"`. `OllamaEstimatorTests` covers weight extraction,
data-URI stripping, image magic-byte validation, backend selection, and
bearer-token auth via `unittest.mock`. `StructuredResponseTests` covers the
JSON-vs-text parsing path. `CacheTests` covers cache hit / disabled / expiry.
`RetryTests` covers the retry-once-on-transient-failure policy (URL errors,
5xx retried; 4xx not). `GuiSmokeTests` builds the Tk root + `CowWeightApp`
and destroys it, catching import/layout regressions in `gui.py` without an
interactive display. The live Ollama network path is not exercised.

Run a single test:

```powershell
python -m unittest tests.test_app.EstimateApiTests.test_estimate_weight_with_image_url -v
```

Lint with ruff (configured in `pyproject.toml`):

```powershell
ruff check .
ruff check --fix .
```

---

## Project structure

```
.
├── app.py                       # HTTP API + CowWeightEstimator (the core)
├── gui.py                       # Tkinter desktop app, reuses app.CowWeightEstimator
├── start_gui.bat                # Double-click launcher (pythonw, no console window)
├── pyproject.toml               # Project metadata + ruff config
├── CONTRIBUTING.md             # Conventions + step-by-step guides for changes
├── .env.example                # Template for local config (committed)
├── .env                         # Local config (gitignored, not committed)
├── tests/
│   └── test_app.py               # HTTP + estimator + GUI smoke tests
└── .github/workflows/
    └── build-windows.yml         # Release build: tests + PyInstaller .exe upload
```

---

## Architecture

Two layers, both in `app.py`:

- **`CowWeightEstimator`** — the estimation logic, decoupled from HTTP.
  `estimate()` dispatches on the configured backend.
- **`EstimateHandler`** — a `BaseHTTPRequestHandler` subclass. Only
  `POST /estimate-weight` is valid; anything else returns `404`. The estimator
  is injected via `create_server(host, port, estimator=None)` and read off
  `self.server.estimator` — no shared class attribute. Access and error logs
  go through the `aif` logger.

The GUI (`gui.py`) does not start the HTTP server. It instantiates
`CowWeightEstimator` directly (with the backend/model selected in the UI) and
runs the call on a background thread so the Tk event loop never blocks. Image
preview uses Pillow if importable, otherwise degrades to showing the filename
and byte size. Keyboard: `Enter` estimates (in the prompt box, use `Ctrl+Enter`
to keep newlines free). A session-only history panel keeps the last 20
estimates.

---

## Releases (Windows `.exe`)

Publishing a GitHub release triggers `.github/workflows/build-windows.yml`,
which runs the test suite on `windows-latest`, builds a one-file windowed
`CowWeightEstimator.exe` with PyInstaller, and attaches it to the release and
as a workflow artifact. Download it from the release page and run it directly —
no Python installation needed on the target machine.

---

## Notes

- The estimator is created at import time. To change backends or keys, edit
  `.env` (or env vars) and restart the server/GUI.
- WebP uploads on Windows are handled even when the OS reports the image as
  `application/octet-stream`: any `data:<mime>;base64,` prefix is stripped.
- Image bytes are validated against JPEG/PNG/GIF/BMP/WebP magic bytes before
  being sent to the model; non-images are rejected with `400 invalid_image`.
- Weight is extracted from model output by first looking for a JSON object
  with a `weight_kg` field (the default prompt asks for this), and falling back
  to an explicit `<number> kg` (case-insensitive), then the first bare number.
- The in-memory cache is per-estimator. The HTTP server uses one long-lived
  estimator, so repeat requests for the same image hit the cache; the GUI
  builds a fresh estimator per request, so it does not share the cache. Set
  `AIF_CACHE_TTL=0` to disable caching.
- Transient Ollama failures (5xx, network/timeout errors) are retried once
  after a 1 s backoff; 4xx errors and unparseable responses are not retried.
- Every response includes a `request_id` (8-char hex) in the JSON body and the
  `x-request-id` header, and the `aif` logger tags access/error lines with it.