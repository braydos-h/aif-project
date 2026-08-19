# Commits

## 2026-08-19 (session: commits.md append rule)
- Added a rule to `AGENTS.md`: whenever an agent adds or changes code, it MUST
  append a new entry to `commits.md` (with date and time) — only append, never
  overwrite, rewrite, or delete existing entries.

## 2026-08-19 12:42 (session: API polish + smarter estimator)
- **Bundle A — API polish (`app.py`):**
  - Added `GET /health` → `{"status":"ok","backend","model","request_id"}` and
    `GET /` / `/info` → `{"name","version","endpoints","request_id"}`. Unknown
    GET paths return 404 `not_found`. New `do_GET` and `do_OPTIONS` handlers.
  - Added `do_OPTIONS` CORS preflight → 204 with
    `Access-Control-Allow-Origin: *`, `-Methods: POST, GET, OPTIONS`,
    `-Headers: Content-Type`. `_send_json` now adds CORS headers to every
    response (success + error).
  - Added per-request `request_id` (8-char uuid hex) generated at the start of
    each `do_*`. Sent as `x-request-id` response header and included in every
    JSON body (success + error). `log_message` and `_error` tag logs with the
    id so a request can be traced end to end.
  - Added `estimated_weight_lbs = round(kg * 2.20462, 1)` to both backend
    results (`_kg_to_lbs` helper, `KG_TO_LBS` constant).
  - Added `ImageValidationError(ValueError)` and `_validate_image_bytes`:
    `_to_base64_image` now decodes and checks JPEG/PNG/GIF/BMP/WebP magic bytes
    (incl. RIFF….WEBP), raising `ImageValidationError` for non-images / empty /
    invalid-base64. The handler catches `ImageValidationError` → 400
    `code: "invalid_image"` (before the generic `ValueError`→502 path). The
    API-key check still runs first, so the existing 502 no-key test stays green.
- **Bundle B — Smarter estimator (`app.py`):**
  - Rewrote `DEFAULT_PROMPT` to ask the model for a JSON object
    `{weight_kg, confidence, breed, body_condition_score}`.
  - Added `_parse_structured_response(text)`: regex-extracts the first `{...}`,
    `json.loads` it, pulls `weight_kg` + optional `confidence`/`breed`/
    `body_condition_score`; falls back to `_extract_weight_from_text` (`<n> kg`
    then first bare number) when no usable JSON is present. Fully backward
    compatible — the `"612 kg"` FakeResponse test still passes via fallback.
  - Result now includes `confidence`/`breed`/`body_condition_score` when the
    model returned JSON (absent otherwise).
  - Added per-estimator in-memory cache keyed by `sha256(base64 image string)`,
    TTL from `AIF_CACHE_TTL` (default 300 s, `0` disables). `_cache_get` returns
    a shallow copy; `_cache_put` stores the full result incl. lbs + extras.
  - Added `_call_ollama_with_retry`: retries once on 5xx `HTTPError`,
    `URLError`, or `TimeoutError` after `OLLAMA_RETRY_BACKOFF` (1.0 s); 4xx and
    `JSONDecodeError` are not retried. Retries logged at WARNING.
- **GUI (`gui.py`):** `_show_result` and `_show_demo_result` now take the full
  result dict and display `kg / lbs`, plus `breed` and `confidence` (as a
  percentage) in the status line when the model returned them. Added
  `_format_result_text` helper. No new widgets.
- **Tests (`tests/test_app.py`):** added `_png_bytes()`/`_png_base64()`/
  `_png_data_uri()` helpers (minimal real PNG that passes magic-byte
  validation). Updated existing tests that sent non-image base64 to use the PNG
  helper where the bytes must pass validation. Added tests for: lbs in
  response, request_id in header+body (success and error), `GET /health`,
  `GET /`, 404 on unknown GET, `OPTIONS` → 204, CORS header on success,
  `invalid_image` 400 path, `_to_base64_image` rejection (non-image/empty/
  invalid-base64) and acceptance (JPEG/WebP magic bytes), `_parse_structured_
  response` (JSON+extras, partial extras, embedded-in-prose, no-JSON fallback,
  no-weight fallback, non-numeric weight, array fallback), structured-response
  end-to-end via mocked Ollama, cache hit/disabled/expiry, retry-on-URL-error-
  then-succeed, no-retry-on-4xx, retry-on-5xx-then-raise. Suite is now 43 tests
  (was 14). All pass.
- **Docs:** updated `README.md` (highlights, new endpoints in API reference,
  new response fields + `request_id`/CORS notes, `invalid_image` code, config
  table with `AIF_CACHE_TTL`, expanded Testing + Notes sections), `AGENTS.md`
  and `CLAUDE.md` (backend selection, architecture, tests sections), and
  `.env.example` + local `.env` with `AIF_CACHE_TTL=300`.
- `ruff check .` clean. `python -m unittest discover -s tests -v` → 43 passed.

## 2026-08-19 (session: docs for newcomers)
- Added CONTRIBUTING.md with conventions, repository layout, commands,
  and step-by-step guides for common changes (new backend, new endpoint,
  weight-parsing changes, GUI features) plus testing notes and a
  pre-commit checklist. Linked from README.md, AGENTS.md, and CLAUDE.md.
- Added module-level docstrings to `app.py` (with "how to add a backend /
  endpoint" sections) and `gui.py` (threading model + how to extend the
  UI), and docstrings for every class and method in both files.
- Added `CONTRIBUTING.md` to the README project-structure tree.
- All 43 tests pass; `ruff check .` is clean.

## 2026-08-19 (session: demo cow test button)
- Added a "Test demo cows" button to the GUI (`gui.py`) that runs the
  estimator over every image in the `cows/` folder (sorted, jpg/webp/etc.),
  showing per-image progress, the last result, and adding each to the
  session history. Runs in a background thread like the normal estimate.
- All 14 tests pass; `ruff check .` is clean.

## 2026-08-19 12:27 (session: custom Ollama URL in GUI)
- Added `ollama_url` constructor arg to `CowWeightEstimator` in `app.py`
  (overrides `AIF_OLLAMA_URL` env / `DEFAULT_OLLAMA_URL` when passed).
- Added an "Ollama URL" entry to the GUI selector frame (`gui.py`) so the
  user can point the Ollama-compatible backend at any endpoint URL at
  runtime (e.g. local Ollama, OpenAI-compatible gateway) without editing
  `.env`. The URL is passed through to `CowWeightEstimator` per request.
- All 14 tests pass; `ruff check .` is clean.

## 2026-08-19 12:25 (session: GUI polish + code quality)
- Fixed the default Ollama Cloud model name from `gemma4:31b` (20GB local
  download, fails against the cloud endpoint) to `gemma4:31b-cloud` (the
  direct-cloud tag) in `app.py`, `.env`, `.env.example`, README, AGENTS.md,
  and CLAUDE.md. Verified against the Ollama model page.
- Injected the estimator into the server via `create_server(host, port,
  estimator=None)` and read it off `self.server.estimator` in the handler,
  removing the shared `EstimateHandler.estimator` class attribute. Tests now
  pass the estimator to `create_server` instead of monkey-patching the class.
- Added structured error codes to every error response: `missing_body`,
  `invalid_json`, `missing_image`, `not_found`, `estimation_failed`. New 502
  test asserts `code == "estimation_failed"`; existing 400 test asserts
  `code == "missing_image"`.
- Replaced the silenced `log_message` with the `aif` logger; access and
  error logs now flow through `logging` instead of being dropped. Added
  `setup_logging()` called from both `app.py` and `gui.py` entry points.
- Added `model_response` field to the estimator result (raw model text for
  ollama, empty string for the fallback) so the GUI can show the model's
  full reply.
- Added `pyproject.toml` with project metadata, optional `dev`/`gui` extras,
  and a ruff config (line-length 100, py311 target, E/F/I/UP/W rules). Lint
  is clean. Added `coverage` config too.
- Added `.env.example` as a committed template and gitignored `.env` (a
  real Ollama API key was previously committed in `.env`; it returns 401
  now so it was already revoked, but the file should never have been
  tracked). Expanded `.gitignore` to also cover `build/`, `dist/`, `*.spec`,
  `.coverage`, `htmlcov/`.
- Rewrote `gui.py` with: backend/model selector (combobox + entry, no more
  `.env` editing to switch backends), indeterminate progress bar during
  estimates, copy-to-clipboard button, `Enter` estimates / `Ctrl+Enter`
  inserts a newline, session-only history panel (last 20 estimates with
  time/image/weight/source), image preview using Pillow when importable
  (graceful fallback to filename + byte size when not), and a read-only
  view of the model's full reply under the weight.
- Added `GuiSmokeTests` that builds the Tk root + `CowWeightApp` and
  destroys it, catching import/layout regressions in CI without an
  interactive display. Verified the full GUI flow end-to-end with a temp
  image: preview rendered, estimate ran on the background thread, history
  row added, copy button worked.
- Updated README.md, AGENTS.md, and CLAUDE.md to reflect the new model
  name, error codes, logging, ruff/pyproject, GUI features, and `.env`
  handling. All 14 tests pass; `ruff check .` is clean.

## 2026-08-19 (session: README rewrite)
- Rewrote `README.md` end-to-end: added a Highlights section, Requirements,
  Quick start (GUI + API), full API reference with request/response tables
  and status codes, a Configuration section with a settings table and Ollama
  Cloud setup steps, offline-mode notes, corrected the test command (dropped
  the stale Linux CI `cd /home/runner/...` path), Project structure, an
  Architecture summary, the Releases/PyInstaller workflow, and a Notes section
  covering import-time estimator construction, WebP handling, and weight
  extraction. No code changes.

## 2026-08-19 14:30
- Applied ponytail-audit cuts to `app.py`: removed the unused `custom` backend
  (`_estimate_via_custom_api`, `BACKEND_CUSTOM`, the `api_url`/`api_key`
  constructor params, and `X-API-Key`/three-key response guessing), inlined the
  single-caller `_is_ollama_cloud_url` helper, dropped the `BACKEND_OLLAMA` and
  `BACKEND_NONE` constants in favor of string literals, and removed the unused
  `Tuple` import. Net ~47 lines removed.
- Removed `test_custom_api_url_selects_custom_backend` and the now-irrelevant
  `api_url=None` argument from `test_default_backend_is_ollama` in
  `tests/test_app.py`. All 12 tests pass.
- Updated `AGENTS.md`, `CLAUDE.md`, `README.md`, and `.env` to drop `custom`
  backend references; also corrected AGENTS.md/CLAUDE.md's stale
  localhost/llava wording for the `ollama` backend to match the actual Ollama
  Cloud default.

## 2026-08-19 11:56
- Added `.github/workflows/build-windows.yml` GitHub Actions workflow that
  triggers on release publish, runs the test suite on `windows-latest`,
  bundles `gui.py` into a one-file Windows `.exe` with PyInstaller, and
  uploads the executable to the release and as a workflow artifact.

## 2026-07-23 21:10
- Fixed direct Ollama Cloud requests to use the API model name `gemma4:31b`;
  the `gemma4:31b-cloud` alias is only valid through a local Ollama runtime.
- Improved Ollama HTTP errors to include the response detail returned by the
  cloud API and added a test assertion for the outgoing direct-cloud model.
- Fixed WebP uploads on Windows by stripping base64 data-URI prefixes even when
  the operating system reports the image as `application/octet-stream`.

## 2026-07-23 21:06
- Switched the default Ollama endpoint from the local runtime to the direct
  Ollama Cloud API (`https://ollama.com/api/generate`) for `gemma4:31b-cloud`.
- Added `OLLAMA_API_KEY` bearer-token support, a clear missing-key error, cloud
  request tests, and updated configuration and documentation with API-key setup.

## 2026-07-23 21:30
- Changed the default Ollama model to `gemma4:31b-cloud` in both the built-in
  application default and `.env` configuration. Updated the model default in
  the README and Claude guidance.

## 2026-07-23 21:00
- Added `gui.py`, a dependency-free Tkinter Windows desktop interface for selecting
  a local cow image, editing the estimation prompt, and displaying the estimate.
  It runs the existing configured estimator directly and keeps the window
  responsive while an estimate is being requested.
- Added `start_gui.bat` for launching the desktop app by double-clicking, with
  no command window, and updated `README.md` with the launch instructions.

## 2026-07-21 13:17
- Made Ollama the default estimation backend in `app.py` (`CowWeightEstimator`).
  New `ollama` backend POSTs to the local Ollama runtime (`/api/generate` by
  default, model `llava`), sending the image as base64 and extracting the weight
  from the model's text reply (`<n> kg` preferred, else first number).
- Added explicit backend selection: `ollama` (default), `custom` (generic AI API
  via `AIF_AI_API_URL`, preserving the original behavior), and `none`
  (deterministic local fallback).
- Added a stdlib-only `.env` loader (`_load_env_file`) so config is read from a
  `.env` file at startup without adding any external dependency; existing
  environment variables take precedence.
- Created `.env` with default Ollama settings and documented backend switches.
- Updated `tests/test_app.py`: HTTP tests now force `backend="none"` so they
  keep using the deterministic fallback; added `OllamaEstimatorTests` covering
  backend selection, weight extraction, and data-URI stripping. All 10 tests pass.
- Updated `CLAUDE.md` and `README.md` to describe the new default backend,
  backend-selection options, and `.env`-based configuration.
