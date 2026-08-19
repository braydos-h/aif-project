# Cow Weight Estimator

Estimate a cow's weight from a photo. Ships as a **Rust HTTP API**
(`backend/`, crate `aif-backend`) and a Tkinter desktop app (`aif/gui.py`).
The Python package (`aif/`) is GUI + estimator only — the HTTP server is
Rust. The Python side is stdlib-only (no `pip install` required); the Rust
side has three dependencies (`ureq` with rustls TLS, `serde_json`, `sha2`).

New here? Read [CONTRIBUTING.md](CONTRIBUTING.md) for conventions, the
repository layout, and step-by-step guides for common changes.

The default backend is **Ollama Cloud**, which sends the image to a vision
model and extracts the weight from its reply. A deterministic offline fallback
is included for testing and air-gapped use.

---

## Highlights

- **Two interfaces, one contract.** A JSON HTTP API for automation (Rust) and
  a double-clickable Windows GUI for interactive use (Python/Tkinter). Both
  share the same `.env` configuration and the same estimate shape
  (`estimated_weight_kg`, `estimated_weight_lbs`, `source`, `model_response`,
  …).
- **A threaded Rust server.** The API runs on a hand-rolled HTTP/1.1 server
  (`std::net::TcpListener`, one thread per connection), so slow Ollama
  requests never block each other — concurrent requests run in parallel.
- **Few moving parts.** Python is pure standard library (`urllib`, `hashlib`,
  `base64`, `re`, `logging`, `tkinter`) with its own stdlib-only `.env`
  loader; Rust pulls in only `ureq` (TLS via rustls), `serde_json`, and
  `sha2` — no async runtime, no web framework. Pillow is optional — if
  installed, the GUI shows an image preview; if not, it degrades to showing
  the filename and byte size.
- **Pluggable backend.** `ollama` (vision model via Ollama Cloud, default) or
  `none` (stable SHA-256-derived local estimate, no network), selectable via
  `AIF_AI_BACKEND` in both the server and the GUI's runtime selector.
- **Robust input handling.** Accepts `image_url` or raw/base64 `image_base64`,
  including `data:` URIs. Strips WebP data-URI prefixes even when Windows labels
  them `application/octet-stream`.
- **Structured errors.** Every error response carries a machine-readable `code`
  field (`missing_image`, `invalid_image`, `estimation_failed`, …) alongside the
  human message. Every response (success or error) also carries a `request_id`
  (8-char hex) echoed in both the `x-request-id` header and the JSON body, and
  flows through the server's stderr logs so you can trace a request end to end.
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
- **Tested.** The Python HTTP suite spawns the real Rust binary and exercises
  it over real sockets (status codes, JSON serialization, CORS, request IDs,
  concurrency), Rust unit tests cover each module, and Python unit tests cover
  the estimator's parsing/cache/retry logic plus a GUI smoke test.
- **One-file Windows `.exe`s.** A GitHub Actions workflow builds both the Rust
  backend and a standalone `CowWeightEstimator.exe` (PyInstaller) on every
  published release.

---

## Requirements

- **Rust toolchain** (for the API server): [rustup](https://rustup.rs/).
  - If you're on Windows without Visual Studio Build Tools, install the GNU
    toolchain too: `rustup toolchain install stable-gnu`, then build with
    `cargo +stable-gnu build --release --manifest-path backend/Cargo.toml`.
- **Python 3.8+** (tested on 3.12 in CI) for the GUI. No third-party
  packages — `tkinter` ships with standard Python installers on Windows.
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

Build the Rust backend, then start it:

```powershell
cargo build --release --manifest-path backend/Cargo.toml
python app.py
```

`python app.py` launches the compiled `aif-backend` binary (override the path
with the `AIF_BACKEND_BIN` env var) and the API listens on
`http://127.0.0.1:8080`. See [API reference](#api-reference).

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
| `prompt`       | string | no                        | Override the estimation prompt. Defaults to `DEFAULT_PROMPT`. |

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
fallback at startup (both the Python package and the Rust binary use the same
rules: values already in the environment take precedence over `.env`). The
server reads config once at startup, so changes after it has started have no
effect — restart to pick up new config.

| Variable           | Default                          | Used by          | Description                                                                 |
| ------------------ | -------------------------------- | ---------------- | --------------------------------------------------------------------------- |
| `AIF_AI_BACKEND`   | `ollama`                         | both             | `ollama` (Ollama Cloud) or `none` (deterministic local fallback).          |
| `AIF_OLLAMA_URL`   | `https://ollama.com/api/generate` | both             | Ollama Cloud generate endpoint.                                            |
| `AIF_AI_MODEL`     | `gemma4:31b-cloud`               | both             | Model name sent in the request.                                            |
| `OLLAMA_API_KEY`   | _(none)_                         | both (cloud)     | Bearer token. Required when `AIF_OLLAMA_URL` points at `ollama.com`.       |
| `AIF_CACHE_TTL`    | `300`                            | server + GUI     | In-memory cache TTL in seconds. `0` disables caching.                      |

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
cargo test --manifest-path backend/Cargo.toml
python -m unittest discover -s tests -v
```

(the Python suite spawns the compiled Rust binary, so build the release
binary first).

- Rust unit tests live next to each module in `backend/src/`: config parsing,
  base64/magic-byte validation, structured/text weight parsing, fallback
  determinism, cache hit/disabled/expiry, and retry details.
- `tests/test_server.py` spawns the real binary on a free port and exercises it
  over real sockets, validating status codes, JSON serialization, CORS
  headers, request IDs, error codes, and concurrent requests. The HTTP tests
  force `backend="none"` to stay deterministic. A 502 test forces
  `backend="ollama"` with no key and asserts `code == "estimation_failed"`; a
  400 test forces `backend="ollama"` with a key but non-image bytes and asserts
  `code == "invalid_image"`.
- `OllamaEstimatorTests` covers the Python estimator's weight extraction,
  data-URI stripping, image magic-byte validation, backend selection, and
  bearer-token auth via `unittest.mock`. `StructuredResponseTests` covers the
  JSON-vs-text parsing path. `CacheTests` covers cache hit / disabled / expiry.
  `RetryTests` covers the retry-once-on-transient-failure policy. The live
  Ollama network path is not exercised.
- `GuiSmokeTests` builds the Tk root + `CowWeightApp` and destroys it,
  catching import/layout regressions in `aif/gui.py`.

Run a single test:

```powershell
python -m unittest tests.test_estimator.OllamaEstimatorTests.test_cloud_endpoint_uses_bearer_token -v
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
├── backend/                     # The Rust HTTP API (crate aif-backend)
│   ├── Cargo.toml              # 3 deps: ureq (rustls), serde_json, sha2
│   └── src/
│       ├── config.rs           # .env loader, defaults, env-var precedence
│       ├── validate.rs         # base64 decoding + image magic-byte validation
│       ├── parse.rs            # JSON-first / free-text weight extraction
│       ├── fallback.rs         # deterministic SHA-256-derived estimate
│       ├── cache.rs            # in-memory TTL result cache
│       ├── ollama.rs           # Ollama Cloud client (auth, retry policy)
│       ├── http.rs             # threaded HTTP/1.1 server (all routes)
│       ├── lib.rs              # module wiring
│       └── main.rs             # entry point (--host / --port flags)
├── aif/                         # The Python package (GUI + estimator)
│   ├── __init__.py             # Package exports (estimator, constants)
│   ├── config.py               # Constants, defaults, .env loader
│   ├── estimator.py            # CowWeightEstimator (GUI only)
│   └── gui.py                  # Tkinter desktop app, reuses aif.estimator
├── app.py                       # Launcher: python app.py spawns the Rust API
├── gui.py                       # Thin wrapper: python gui.py runs the GUI
├── start_gui.bat                # Double-click launcher (pythonw, no console window)
├── pyproject.toml               # Python metadata + ruff config
├── CONTRIBUTING.md             # Conventions + step-by-step guides for changes
├── .env.example                # Template for local config (committed)
├── .env                         # Local config (gitignored, not committed)
├── cows/                        # Demo images for the GUI's "Test demo cows" button
├── tests/
│   ├── test_server.py           # HTTP suite against the Rust binary (real sockets)
│   ├── test_estimator.py        # Python estimator unit tests
│   └── test_gui.py              # GUI smoke tests
└── .github/workflows/
    └── build-windows.yml         # Release build: Rust binary + PyInstaller .exe
```

## Architecture

Two layers:

- **Rust `aif-backend`** (`backend/`) — the HTTP API. A hand-rolled HTTP/1.1
  server on `std::net::TcpListener` with one thread per connection, so slow
  Ollama requests never block each other (the old Python `http.server` was
  single-threaded). Valid routes: `POST /estimate-weight`, `GET /health`,
  `GET /` or `/info`, `OPTIONS`; anything else returns `404`. Requests are
  read as a single Content-Length-delimited body (no keep-alive, no chunked
  encoding — intentional). The Ollama client (`ureq` + rustls) retries once on
  transient failures; results are cached in a TTL cache. `app.py` locates and
  spawns the binary (`AIF_BACKEND_BIN` overrides the path); `--port 0` prints
  the bound port (the test suite relies on this).
- **`CowWeightEstimator`** (`aif/estimator.py`) — the GUI's estimation logic,
  decoupled from HTTP. `estimate()` dispatches on the configured backend and
  returns the same dict shape the Rust server produces. The Rust and Python
  implementations of shared logic (fallback math, weight extraction, image
  validation, cache keying) must stay behaviorally identical.
- **`CowWeightApp`** (`aif/gui.py`) — the Tkinter desktop app.

The GUI does not start the HTTP server. It instantiates
`CowWeightEstimator` directly (with the backend/model selected in the UI) and
runs the call on a background thread so the Tk event loop never blocks. Image
preview uses Pillow if importable, otherwise degrades to showing the filename
and byte size. Keyboard: `Enter` estimates (in the prompt box, use `Ctrl+Enter`
to keep newlines free). A session-only history panel keeps the last 20
estimates.

---

## Releases (Windows)

Publishing a GitHub release triggers `.github/workflows/build-windows.yml`,
which runs `cargo test` and the Python suite on `windows-latest`, builds the
Rust backend, packages the GUI into a one-file windowed `CowWeightEstimator.exe`
with PyInstaller, and attaches **both** executables to the release and the
workflow artifact. Download and run directly — no Python or Rust installation
needed on the target machine.

---

## Notes

- The server reads config at startup (Python at import time, Rust at launch).
  To change backends or keys, edit `.env` (or env vars) and restart the
  server/GUI.
- WebP uploads on Windows are handled even when the OS reports the image as
  `application/octet-stream`: any `data:<mime>;base64,` prefix is stripped.
- Image bytes are validated against JPEG/PNG/GIF/BMP/WebP magic bytes before
  being sent to the model; non-images are rejected with `400 invalid_image`.
- Weight is extracted from model output by first looking for a JSON object
  with a `weight_kg` field (the default prompt asks for this), and falling back
  to an explicit `<number> kg` (case-insensitive), then the first bare number.
- The in-memory cache is per-process. The HTTP server uses one long-lived
  cache, so repeat requests for the same image hit it; the GUI builds a fresh
  estimator per request, so it does not share the cache. Set
  `AIF_CACHE_TTL=0` to disable caching.
- Transient Ollama failures (5xx, network/timeout errors) are retried once
  after a 1 s backoff; 4xx errors and unparseable responses are not retried.
- Every response includes a `request_id` (8-char hex) in the JSON body and the
  `x-request-id` header; the server tags stderr logs with it.
