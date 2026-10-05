# Security and configuration reference

## Modes

| Mode | How | Behavior |
| --- | --- | --- |
| Local open | defaults (no `AIF_REQUIRE_AUTH`) | Historical behavior: anonymous estimates, public info/metrics, `Access-Control-Allow-Origin: *`. For trusted local use only. |
| Authenticated local | `AIF_REQUIRE_AUTH=1` | Estimation, history, animals, exports, and metrics need sessions; provider overrides still allowed (single-user flexibility). |
| Production | `AIF_PRODUCTION=1` (implies auth) | Full hardening below. Startup aborts unless `AIF_PUBLIC_ORIGIN` is `https://…` and (with `backend=ollama`) `OLLAMA_API_KEY` is set. |

## Production hardening (all tested)

- Per-request `backend`/`model`/`ollama_url`/`ollama_api_key` overrides
  rejected (`400 invalid_options`); provider secrets stay server-side.
- Cookie sessions: HttpOnly, `SameSite=Lax`, `Secure`; CSRF synchronizer
  tokens on every state-changing route (cookie-only auth, no bearer
  tokens in JS-reachable storage).
- Login/invite/recovery rate-limited per IP and per account; generic
  failures (no enumeration oracle); 256-bit invite/recovery/session
  tokens, hashes only in the DB; single-use invites with expiry,
  revocation, email binding, and a two-active-account cap.
- Same-origin CORS (no `*`), HSTS, `X-Frame-Options: DENY`,
  `X-Content-Type-Options: nosniff`, tight CSP; reduced `/info` + `/health`
  (no model/backend/URL/prompt); `/metrics` operator-only.
- Request framing preserved: 8 KiB request line, 100-header / 32 KiB
  caps, duplicate/invalid `Content-Length` → 400, chunked refused,
  20 MiB bodies refused from headers, `take()`-bounded reads, atomic
  pre-spawn connection cap (503 `server_busy`), inference concurrency
  gate + operator pause + per-user daily quotas + bounded single provider
  retry (no infinite loops).
- SSRF: `image_url` and per-request `ollama_url` refuse loopback,
  private, link-local, and `localhost` hosts; no redirects; 20 MiB fetch
  cap; magic-byte validation; the custom stack was audited and kept
  (no framework migration needed — behavior and tests are preserved).
- Model replies are untrusted: finite-value, 20–2500 kg range, 128-char
  breed, confidence/BCS filtering, 5 MiB upstream cap; prompt/image
  content cannot change auth, config, credentials, routing, or policy.
- `X-Forwarded-For` honored only from `AIF_TRUSTED_PROXIES`
  (default loopbacks for the local Caddy).

## Configuration

| Variable | Default | Purpose |
| --- | --- | --- |
| `AIF_AI_BACKEND` | `ollama` | `ollama` or deterministic `none` |
| `AIF_AI_MODEL` | `gemma4:31b-cloud` | Ollama model name |
| `AIF_OLLAMA_URL` | `https://ollama.com/api/generate` | Ollama-compatible endpoint |
| `OLLAMA_API_KEY` | empty | Server-side bearer token; never returned/logged |
| `AIF_CACHE_TTL` | `300` | Result cache TTL seconds; `0` disables; ≤30 days, ≤512 entries |
| `AIF_REQUIRE_AUTH` | `0` | Require sessions for estimation/history/account/operator/metrics |
| `AIF_PRODUCTION` | `0` | Full hardening profile (implies auth + secure cookies + HSTS) |
| `AIF_DATA_DIR` | `data` | SQLite directory (`aif.db`); created on startup |
| `AIF_PUBLIC_ORIGIN` | `http://127.0.0.1:8080` | HTTPS origin used to build invite/recovery links (never the Host header) |
| `AIF_COOKIE_SECURE` | production | `Secure` cookie flag (override for HTTPS-terminated tests) |
| `AIF_SESSION_DAYS` | `30` | Session lifetime, 1–365 days |
| `AIF_INVITE_DAYS` | `7` | Invite lifetime, 1–30 days |
| `AIF_DAILY_LIMIT` | `200` | Photo estimates per user per UTC day (tape free) |
| `AIF_MAX_INFERENCE` | `4` | Concurrent provider inferences, 1–64 |
| `AIF_INFERENCE_PAUSED` | `0` | Start with provider inference paused |
| `AIF_TRUSTED_PROXIES` | `127.0.0.1,::1` | Peers allowed to supply `X-Forwarded-For` |
| `AIF_RETAIN_PHOTOS` | `0` | `1` enables opt-in private photo retention |
| `AIF_PHOTO_TTL_DAYS` | `30` | Retained photo lifetime in days (1–365) |
| `AIF_PHOTO_QUOTA_MB` | `50` | Retained photo quota per user in MiB (1–1024) |
| `AIF_JOBS_ENABLED` | `1` | Durable background estimate jobs |
| `AIF_JOB_WORKER` | `1` | Run the background worker in this process (`0` to disable) |
| `AIF_JOB_TTL_HOURS` | `72` | Completed-job retention in hours (1–720) |
| `AIF_DISK_ALERT_MB` | `1024` | Operator alert threshold for data-dir size in MiB |
| `AIF_OPERATOR_EMAIL` | empty | First-run bootstrap: mint an operator invite when the DB is empty |
| `AIF_BACKUP_PASSPHRASE` | empty | Backup encryption passphrase (env only, never a file) |

## API error contract (stable)

Every JSON response carries `request_id` in body and `x-request-id`
header. Codes: `missing_body`, `bad_request`, `invalid_json`,
`missing_image`, `invalid_image`, `invalid_options`, `not_found`,
`estimation_failed`, `server_busy`, `unauthorized`, `forbidden`,
`csrf_invalid`, `rate_limited`, `quota_exceeded`, `user_limit`,
`invalid_credentials`, `invite_invalid/expired/used/revoked`,
`recovery_invalid`, `inference_paused`.

## Decisions recorded

- **SQLite** (bundled): fits two users, single file, WAL, versioned
  migrations, encrypted snapshot backups.
- **Argon2id + random 256-bit tokens** via maintained crates; no custom
  crypto; token hashes only in the DB.
- **Transient-only photos by default**; optional opt-in retention
  (`AIF_RETAIN_PHOTOS=1`) stores metadata-stripped bytes under opaque ids
  with owner-only downloads, quotas, TTL expiry, and sweep cleanup.
- **Durable job queue for background estimates**: FIFO worker, bounded
  backoff retries, cancel/expire/prune lifecycle, restart requeue with
  idempotent exactly-once history saves.
- **Custom HTTP stack kept**: audited framing/timeouts/limits/proxy
  behavior meet the production bar; migrating would churn the tested
  contract for no security gain.
