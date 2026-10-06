# AIF-PROJECT

Cow Weight Estimator is a small local web application that turns a cow image
into a rough weight estimate. The Rust backend serves the browser UI and the
JSON API from one process. No Node.js, npm, Python web framework, or external
CDN is required.

This is an AI estimate, not a replacement for a scale, veterinary advice, or
an official livestock record.

## Quick start

Prerequisites: Rust (pinned to 1.89.0 by `rust-toolchain.toml`; minimum
supported version 1.75 per `backend/Cargo.toml`; Windows also needs the MSVC
Build Tools linker, or use the GNU toolchain), plus a browser.
`node` is only needed for the `node --check web/app.js` syntax check that CI
runs. `install.ps1` automates the Windows setup (installs Rust via winget,
builds the release binary, creates a desktop shortcut).

Build the Rust server, then start the application:

```powershell
cargo build --release --manifest-path backend/Cargo.toml
backend\target\release\aif-backend.exe --host 0.0.0.0 --port 8080
```

Open <http://127.0.0.1:8080/>. The server prints the listening URL on stdout.
The default `--host 0.0.0.0` listens on all interfaces so other machines on
the network can reach it; use `--host 127.0.0.1` to restrict to localhost
only. Exposed instances should set `AIF_REQUIRE_AUTH=1` (always in
production).

For a shortcut that starts the server and opens the browser automatically,
double-click `start_gui.bat` (or run `start_gui.ps1`); they launch the same
binary and open the WebUI. `start.sh` is the Linux/macOS equivalent and
`install.ps1` is the Windows first-time setup script. All launchers use
the fixed default `0.0.0.0:8080` and take no port argument; if the port is
taken, start the binary manually with a different `--port` (e.g. `--port
8081`). The supported UI is the Rust-served WebUI. Production (`deploy/`)
stays loopback-only behind Caddy and is unchanged.

CLI usage: `aif-backend [--host HOST] [--port PORT]`. Both `--flag value`
and `--flag=value` forms work; `--help` prints usage and exits 0, while
unknown flags or missing/invalid values print usage and exit nonzero instead
of silently using defaults.

## WebUI

The browser app supports:

- batch image selection (JPEG, PNG, WebP, BMP, and GIF) with type and size
  checks; large photos are resized in the browser before upload; files can
  also be added by drag-and-drop anywhere on the page or by pasting images,
  and individual files can be removed or the whole selection cleared before
  estimating;
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

When the server requires authentication (`AIF_REQUIRE_AUTH=1`, always in
production), estimates are saved to your private server-side history with
source/model/version stamps; anonymous estimates are never saved. Accounts
require an operator invitation — there is no public registration. See
`docs/privacy.md` for data handling and `docs/recovery.md` for the
operator-relayed invite/recovery flow.

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
| `GET` | `/account.js` | Account/history/animals/operator JavaScript |
| `GET` | `/account.css` | Account stylesheet |
| `GET` | `/api/me` | Session probe (`authenticated`, `auth_required`, CSRF token) |
| `POST` | `/api/auth/login` | Password login (rate-limited, generic failures) |
| `POST` | `/api/auth/logout` | Revoke the current session |
| `POST` | `/api/auth/accept-invite` | Redeem a single-use invite (`token`, `password`, `display_name`) |
| `POST` | `/api/auth/recovery/request` | Generic-success recovery request (operator relays the link) |
| `POST` | `/api/auth/recovery/complete` | Redeem a recovery token for a new password |
| `POST` | `/api/auth/change-password` | Reauthenticated password change (logs out everywhere) |
| `POST` | `/api/auth/profile` | Update display name |
| `GET` | `/api/history` | Paginated history (`page`, `per_page`, `animal_id`, `source`, `from`, `to`) |
| `GET` | `/api/history/{id}` | One owned estimate (others' ids 404) |
| `DELETE` | `/api/history/{id}` | Delete one owned estimate |
| `GET` | `/api/history/export?format=csv` | Spreadsheet-safe CSV download (numeric columns numeric) |
| `GET` | `/api/animals` | List owned animals (`include_archived=1`) |
| `POST` | `/api/animals` | Create an animal (`name`, `breed`, `sex`, `birth_year`, `notes`) |
| `GET` | `/api/animals/{id}` | Animal detail with its estimate trend |
| `PUT` | `/api/animals/{id}` | Edit/archive an owned animal |
| `DELETE` | `/api/animals/{id}` | Delete an animal (estimates kept, unlinked) |
| `POST` | `/api/animals/{id}/measurements` | Record a verified scale weight (`scale_weight_kg`, `measured_at`) |
| `GET` | `/api/account` | Profile summary, usage, photo policy |
| `GET` | `/api/account/export` | Full owned-data JSON export |
| `DELETE` | `/api/account` | Password-confirmed self-deletion (revokes all sessions) |
| `GET` | `/api/photos` | List retained photo metadata (retention must be enabled) |
| `GET` | `/api/photos/{id}` | Owner-only private download |
| `DELETE` | `/api/photos/{id}` | Owner-only photo deletion |
| `POST` | `/api/jobs` | Queue a background estimate (202; same payload as `/estimate-weight`) |
| `GET` | `/api/jobs` | List your jobs, newest first |
| `GET` | `/api/jobs/{id}` | Job status + result when successful (others' ids 404) |
| `POST` | `/api/jobs/{id}/cancel` | Cancel a queued job |
| `POST` | `/api/operator/invites` | Mint an invite (operator; one-time link in response) |
| `GET` | `/api/operator/invites` | Invite list with status |
| `POST` | `/api/operator/invites/{id}/revoke` | Revoke an unused invite |
| `GET` | `/api/operator/users` | Account list (operator) |
| `DELETE` | `/api/operator/users/{id}` | Remove an account (operator) |
| `GET` | `/api/operator/usage` | Per-day inference usage and limits |
| `GET` | `/api/operator/status` | Backend/model/pause/user counters (no secrets) |
| `POST` | `/api/operator/pause` | Flip the inference pause switch (`{"paused": bool}`) |
| `GET` | `/api/operator/audit` | Recent redacted audit entries |
| `GET` | `/info` | Safe JSON application/configuration information |
| `GET` | `/health` | Liveness, effective backend/model, `ollama_configured`, uptime, live connections |
| `GET` | `/metrics` | Operator counters: version/backend/model, uptime, totals, live/rejected connections, cache size |
| `GET` | `/demo-cows` | Controlled list of bundled demo images |
| `GET` | `/demo-cows/{id}` | One approved bundled demo image (`1`, `2`, or `3`) |
| `POST` | `/estimate-weight` | Single estimate (photo, tape, or both) |
| `POST` | `/estimate-batch` | Up to 20 estimates in one request |

`/info` includes `backend`, `model`, `ollama_url`, `default_prompt`, version,
endpoints, and an `ollama_configured` boolean. It never returns
`OLLAMA_API_KEY`. In production the response is reduced (no
backend/model/URL/prompt), `/health` reports liveness only, and `/metrics`
is operator-only.

Authenticated estimates accept three extra fields alongside the photo/tape
inputs: `animal_id` (must be an owned animal, else 404), `measured_at`
(RFC 3339 UTC measurement time, default now), and `idempotency_key`
(1–128 chars; replays return the stored result with `replayed: true` and
`X-Idempotent-Replayed: true` instead of spending inference again).
Successful saves add `history_id` and `saved: true` to the response.
With `AIF_RETAIN_PHOTOS=1`, photo estimates also accept
`retain_photo: true` to store the processed photo privately (response
carries `photo_id`, or `photo_error` when storage fails); anonymous
callers get `400 invalid_options` for retention.

`POST /api/jobs` accepts any single-estimate payload and queues it for
the background worker (202 `{id, status}`); poll `GET /api/jobs/{id}`
until `success` (result embedded) or `failed` (`error_code` set).
Transient 429/502/503 outcomes retry with bounded backoff (3 attempts);
only queued jobs can be cancelled. A restart requeues interrupted jobs
and idempotency guarantees each completed result is saved once.

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

Optional animal details sharpen the AI guess on photo/demo estimates that
reach the `ollama` backend. `animal_breed` is free text
(letters, spaces, hyphens, apostrophes, max 64); `animal_sex` is one of `cow`, `bull`,
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
invalid images, or invalid runtime options, `401` for missing login (when
auth is required), `403` for forbidden operator routes or bad CSRF,
`404` for unknown routes, `429` for rate-limited logins or exhausted daily
quotas, `502` for an estimator/Ollama failure, and `503` (`server_busy`)
when more than 64 connections arrive at once — retry shortly — or
`inference_paused` when the operator has paused AI estimates (tape still
works). Oversize/truncated bodies return
`400 invalid_json`, and empty/non-array/oversize batch `items` return `400
invalid_options`. Error bodies contain `error`,
`code`, and `request_id`; the WebUI maps these to user-friendly messages.

`image_url` downloads are SSRF-guarded: only public `http(s)` hosts are
fetched (loopback, private, link-local, and `localhost`-style names are
refused with `400 invalid_image`), redirects are not followed, and downloads
are capped at 20 MiB. Per-request `ollama_url` overrides enforce the same
private-host refusal (`400 invalid_options`); point the server itself at a
local Ollama via `AIF_OLLAMA_URL` instead. As a precaution the server never
forwards its own `OLLAMA_API_KEY` to a different host unless the same request
supplies its own key.

Example using the deterministic backend:

```powershell
$body = @{ image_base64 = "iVBORw0KGgoAAAANSUhEUgAA..."; backend = "none" } | ConvertTo-Json
Invoke-RestMethod http://127.0.0.1:8080/estimate-weight `
  -Method Post -ContentType "application/json" -Body $body
```

## Configuration

Copy `.env.example` to `.env` for server defaults (`Copy-Item .env.example
.env`). The server probes the current directory first, then the binary's
directory, then the source root. Environment variables that
are already set take precedence over `.env`.

| Variable | Default | Purpose |
| --- | --- | --- |
| `AIF_AI_BACKEND` | `ollama` | `ollama` or deterministic `none` |
| `AIF_AI_MODEL` | `gemma4:31b-cloud` | Ollama model name |
| `AIF_OLLAMA_URL` | `https://ollama.com/api/generate` | Ollama-compatible generate endpoint |
| `OLLAMA_API_KEY` | empty | Server-side Ollama bearer token; never returned |
| `AIF_CACHE_TTL` | `300` | Ollama result cache TTL in seconds; `0` disables; clamped to 30 days, at most 512 entries |
| `AIF_REQUIRE_AUTH` | `0` | `1` requires login for estimation/history/account/operator/metrics |
| `AIF_PRODUCTION` | `0` | `1` enables the production profile (implies auth, secure cookies, HSTS, same-origin CORS, private metrics, reduced info/health, no provider overrides) |
| `AIF_DATA_DIR` | `data` | SQLite directory (`aif.db`), created on startup |
| `AIF_PUBLIC_ORIGIN` | `http://127.0.0.1:8080` | HTTPS origin for invite/recovery links (never the Host header) |
| `AIF_OPERATOR_EMAIL` | empty | First-run bootstrap operator invite when the DB is empty |
| `AIF_COOKIE_SECURE` | production | `Secure` cookie flag |
| `AIF_SESSION_DAYS` | `30` | Session lifetime (1–365 days) |
| `AIF_INVITE_DAYS` | `7` | Invite lifetime (1–30 days) |
| `AIF_DAILY_LIMIT` | `200` | Photo estimates per user per UTC day (tape free) |
| `AIF_MAX_INFERENCE` | `4` | Concurrent provider inferences (1–64) |
| `AIF_INFERENCE_PAUSED` | `0` | Start with provider inference paused |
| `AIF_TRUSTED_PROXIES` | `127.0.0.1,::1` | Peers allowed to supply `X-Forwarded-For` |
| `AIF_RETAIN_PHOTOS` | `0` | `1` enables opt-in private photo retention |
| `AIF_PHOTO_TTL_DAYS` | `30` | Retained photo lifetime in days (1–365) |
| `AIF_PHOTO_QUOTA_MB` | `50` | Retained photo quota per user in MiB (1–1024) |
| `AIF_JOBS_ENABLED` | `1` | Durable background estimate jobs |
| `AIF_JOB_WORKER` | `1` | Run the background worker in this process (`0` to disable) |
| `AIF_JOB_TTL_HOURS` | `72` | Completed-job retention in hours (1–720) |
| `AIF_DISK_ALERT_MB` | `1024` | Operator alert threshold for data-dir size in MiB |

Ollama Cloud requires an API key. The `none` backend makes a deterministic
SHA-256-derived estimate in the range 250–900 kg and performs no network
request, making it suitable for local demos and tests.

Operator administration (invites, users, recovery) runs against the same
database and exits — point `AIF_DATA_DIR` at the service data dir:

```sh
aif-backend --create-invite user@example.com [--role user|operator]
aif-backend --list-users | --list-invites
aif-backend --revoke-invite <id> | --delete-user <email> | --create-recovery <email>
```

Estimate quality is gated by a repeatable evaluation (`eval/README.md`):

```sh
cargo run --manifest-path backend/Cargo.toml --bin aif-eval -- \
  --dataset eval/fixtures/example.json --out /tmp/opencode/eval-report.json
```

Production deployment, operations, security, privacy, and recovery flows
live in `docs/` (`deployment.md`, `operations.md`, `security.md`,
`privacy.md`, `recovery.md`); service units and proxy config in `deploy/`.

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

## Troubleshooting

- `cargo: command not found` — install Rust via [rustup](https://rustup.rs/)
  and open a new terminal so `PATH` refreshes.
- `link.exe not found` (Windows) — install the MSVC Build Tools, or build
  with the GNU toolchain instead.
- `Error: backend binary not found` — run `cargo build --release
  --manifest-path backend/Cargo.toml` first; the launchers never build it.
- Port 8080 in use — start with `--port 8081` and open that URL instead.
- `502 estimation_failed` mentioning an API key — set `OLLAMA_API_KEY` (create
  one at `https://ollama.com/settings/keys`), or use `"backend": "none"` for
  the offline placeholder.
- `400 invalid_image` — the file is not a supported image, exceeds 20 MiB, or
  the URL host is blocked (loopback/private/metadata hosts are refused).
- `503 server_busy` — more than 64 connections arrived at once; wait a moment
  and retry (the WebUI retries once automatically).
- `.env` not picked up — copy `.env.example` to `.env`
  (`Copy-Item .env.example .env`); the server probes the current directory,
  then the binary's directory, then the source root.

## Repository layout

```text
backend/src/       Rust HTTP server, config, validation, parsing, Ollama, cache
backend/src/bin/   aif-eval reference-dataset evaluation binary
backend/tests/     Real-HTTP integration tests + static WebUI guards
web/               Rust-served WebUI assets (estimator + account UI)
cows/              Approved bundled demo images
deploy/            systemd unit, Caddyfile, install/update/rollback/backup/restore
docs/              deployment, operations, security, privacy, recovery guides
eval/              estimate-quality framework, fixtures, evaluation README
start_gui.bat/.ps1 Windows launchers (release binary + browser)
start.sh           Linux/macOS launcher
install.ps1        Windows first-time setup (Rust install + build + shortcut)
.github/workflows/ CI (fmt/clippy/test/build + WebUI check) and release build
```
