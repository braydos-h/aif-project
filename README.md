# AIF-PROJECT

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
backend\target\release\aif-backend.exe --host 127.0.0.1 --port 8080
```

Open <http://127.0.0.1:8080/>. The server prints the listening URL on stdout.

For a shortcut that starts the server and opens the browser automatically,
double-click `start_gui.bat` (or run `start_gui.ps1`); they launch the same
binary and open the WebUI. The supported UI is the Rust-served WebUI.

CLI usage: `aif-backend [--host HOST] [--port PORT]`. Both `--flag value`
and `--flag=value` forms work; `--help` prints usage and exits 0, while
unknown flags or missing/invalid values print usage and exit nonzero instead
of silently using defaults.

## WebUI

The browser app supports:

- batch image selection (JPEG, PNG, WebP, BMP, and GIF) with type and size
  checks; large photos are resized in the browser before upload; files can
  also be added by drag-and-drop anywhere on the page or by pasting images,
  and the selection can be cleared before estimating;
- one **Estimate Weight** button that sends the batch in one `POST
  /estimate-batch` call (chunked to stay under the 20 MB body cap),
  showing per-image progress and a final succeeded/failed count, with
  per-item **Retry** for failed photos and one automatic retry on
  `503 server_busy`;
- optional animal details (breed, sex, age) sent with every estimate to
  sharpen the AI guess;
- tape measurements double as a photo cross-check: fill them in before
  estimating and each photo result carries the tape comparison (with a
  warning when photo and tape disagree by over 20%);
- a latest-answer area plus a session-only history of the last 20 successful
  estimates, each with its weight range and source, removable one by one,
  with a weight-trend chart and CSV export;
- a tape-measure section (heart girth + body length, 50–300 cm) for offline
  Schaeffer estimates with no photo needed;
- weight ranges on every result (±10% photo, ±5% tape) and a dosing warning
  on every result and in the footer;
- live status updates, backend health badge polled every 30 seconds with
  offline detection, dark-mode support, and a responsive single-column
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

The Rust implementation is the whole application server and keeps the
deterministic fallback, image validation, caching, retry behavior, request
IDs, and error codes. There is no Python runtime: the backend, the WebUI,
and the tests are all Rust (plus plain browser HTML/CSS/JavaScript).

## API reference

### Static and information routes

| Method | Path | Result |
| --- | --- | --- |
| `GET` | `/` | WebUI HTML |
| `GET` | `/styles.css` | WebUI stylesheet |
| `GET` | `/app.js` | WebUI JavaScript |
| `GET` | `/info` | Safe JSON application/configuration information |
| `GET` | `/health` | Liveness, effective backend/model, uptime, live connections |
| `GET` | `/metrics` | Operator counters: uptime, totals, live/rejected connections, cache size |
| `GET` | `/demo-cows` | Controlled list of bundled demo images |
| `GET` | `/demo-cows/{id}` | One approved bundled demo image (`1`, `2`, or `3`) |
| `POST` | `/estimate-weight` | Single estimate (photo, tape, or both) |
| `POST` | `/estimate-batch` | Up to 20 estimates in one request |

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

A successful photo response includes:

```json
{
  "estimated_weight_kg": 612.0,
  "estimated_weight_lbs": 1349.2,
  "weight_min_kg": 550.8,
  "weight_max_kg": 673.2,
  "weight_min_lbs": 1214.3,
  "weight_max_lbs": 1484.1,
  "source": "ollama",
  "model": "gemma4:31b-cloud",
  "prompt_used": "Estimate this cow in kilograms and return JSON.",
  "model_response": "{\"weight_kg\":612,\"confidence\":0.82}",
  "confidence": 0.82,
  "breed": "Angus",
  "body_condition_score": 6.0,
  "disclaimer": "Estimate only — verify with a scale. Do not dose medication from this estimate.",
  "request_id": "bce5028c"
}
```

`model`, `confidence`, `breed`, and `body_condition_score` are present when
the selected backend returns them. `source` is `ollama`,
`local_fallback`, or `tape_measure`. Photo estimates carry a ±10% range
(`weight_min_kg`/`weight_max_kg` plus lbs); tape estimates carry a ±5%
range. Every success also carries a `disclaimer`: verify with a scale and
never dose medication from an estimate. Every JSON response also sends the
same request ID in the `x-request-id` header and includes
`Access-Control-Allow-Origin: *` for existing API clients.

Tape-measure estimates need no image and no network (Schaeffer's formula:
`girth_cm² × length_cm / 10838`). Send both fields together; both must be
numbers between 50 and 300 cm. When image and tape are sent together, the
photo estimate stays primary and the tape cross-check is merged in as
`tape_weight_kg`/`tape_weight_lbs`/`tape_min_kg`/`tape_max_kg` plus the
echoed `heart_girth_cm`/`body_length_cm`.

```json
{
  "heart_girth_cm": 180,
  "body_length_cm": 150
}
```

Measure heart girth just behind the front legs and body length from chest
to tail head, with the animal standing square.

Optional animal details sharpen the AI guess. `animal_breed` is free text
(letters, spaces, hyphens, max 64); `animal_sex` is one of `cow`, `bull`,
`steer`, `heifer`, `calf`, or `unknown`; `animal_age_years` is 0–30. They
are folded into the model prompt, echoed back as `animal_breed`/
`animal_sex`/`animal_age_years` (never overwriting the model's own `breed`
guess), and omitted fields leave old clients untouched. Bad hints return
`400 invalid_options`.

```json
{
  "image_base64": "iVBORw0KGgoAAAANSUhEUgAA...",
  "animal_breed": "Angus",
  "animal_sex": "cow",
  "animal_age_years": 4.5
}
```

### `POST /estimate-batch`

Send up to 20 single-estimate payloads in one request (same fields as
above per item, sharing the 20 MB body limit). Items run sequentially;
one bad item never fails the batch:

```json
{ "items": [{ "image_base64": "..." }, { "heart_girth_cm": 180, "body_length_cm": 150 }] }
```

The response carries per-item `{status, body}` pairs, each body with its
own `{parent}-{index}` request id:

```json
{ "results": [{ "status": 200, "body": { "...": "..." } }], "request_id": "bce5028c" }
```

Status codes are `200` for success, `400` for missing/malformed input,
invalid images, or invalid runtime options, `404` for unknown routes, `502`
for an estimator/Ollama failure, and `503` (`server_busy`) when more than 64
connections arrive at once — retry shortly. Error bodies contain `error`,
`code`, and `request_id`; the WebUI maps these to user-friendly messages.

`image_url` downloads are SSRF-guarded: only public `http(s)` hosts are
fetched (loopback, private, link-local, and `localhost`-style names are
refused with `400 invalid_image`), redirects are not followed, and downloads
are capped at 20 MiB.

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

Requirements are Rust stable for the backend. The frontend is plain
HTML/CSS/JavaScript and has no package install step.

```powershell
cargo fmt --manifest-path backend/Cargo.toml
cargo test --manifest-path backend/Cargo.toml
cargo build --release --manifest-path backend/Cargo.toml
```

The integration tests (`backend/tests/server.rs`) spawn the backend binary
on a free local port over real sockets. Set `AIF_BACKEND_BIN` to override
the binary path when needed.

## Repository layout

```text
backend/src/       Rust HTTP server, config, validation, parsing, Ollama, cache
backend/tests/     Real-HTTP integration tests + static WebUI guards
web/               Rust-served WebUI assets
cows/              Approved bundled demo images
start_gui.bat/.ps1 Launch the release binary and open the WebUI in a browser
```
