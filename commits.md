# Commits

## 2026-08-19 (session: installer)
- Added `install.ps1`, a bootstrap installer that: installs Python 3.12 via
  winget when missing (installing winget itself if absent), installs Rust via
  winget when missing, builds the Rust backend in release mode, and creates a
  "Cow Weight Estimator" desktop shortcut pointing at `start_gui.ps1`. Pauses
  with a readable error message on any failure.

## 2026-08-19 (session: PowerShell launcher)
- Added `start_gui.ps1`, a PowerShell equivalent of `start_gui.bat`: changes
  to the script directory, verifies `gui.py` exists next to the script,
  launches it with `pythonw.exe` if found on PATH (falling back to
  `python.exe`), and prints a paused error message if Python is missing.

## 2026-08-19 13:10 (session: harden start_gui.bat)
- **`start_gui.bat` now robust:** `cd`s to the script directory so `.env`
  resolution and relative paths work regardless of where it's double-clicked
  from; verifies `gui.py` exists next to the script; locates `pythonw.exe`
  via PATH (falling back to `python.exe` so the GUI still opens with a
  console window if `pythonw` is missing); prints a clear, paused error
  message if Python is not installed. Verified by launching the GUI twice
  (window title "Cow Weight Estimator" appeared both times).

## 2026-08-19 15:10 (session: repo cleanup + package restructure)
- **Restructured the codebase into a proper Python package (`aif/`):**
  - Split `app.py` into `aif/config.py` (constants, defaults, `.env` loader,
    `setup_logging`), `aif/estimator.py` (`CowWeightEstimator`,
    `ImageValidationError`, image validation helpers), and `aif/server.py`
    (`EstimateHandler`, `create_server`). Moved the Tkinter app into
    `aif/gui.py`. Added `aif/__init__.py` re-exporting the public API.
  - `app.py` and `gui.py` at the repo root are now thin backward-compatible
    wrappers, so `python app.py`, `python gui.py`, `start_gui.bat`, and any
    `from app import ...` / `from gui import ...` imports keep working
    unchanged.
  - `_load_env_file` now resolves `.env` relative to the repo root (parent of
    the `aif/` package) instead of the single-file directory.
- **Tests split by concern** (`tests/`): `test_server.py` (HTTP suite),
  `test_estimator.py` (Ollama/structured-response/cache/retry units), and
  `test_gui.py` (GUI smoke + entry-point import check), all importing from
  `aif`. Deleted the old `tests/test_app.py`. Suite is now 44 tests, all
  passing.
- **Tooling/config:** `pyproject.toml` coverage source and setuptools config
  now target the `aif` package; the release workflow bundles `aif;aif`
  instead of `app.py`; `.gitignore` covers `.ruff_cache/`.
- **Housekeeping:** deleted the gitignored `497789W-AIF10-AT1-Brayden-
  Habets [Autosaved].pptx`, all `__pycache__` dirs, and `.ruff_cache`.
- **Docs:** updated README (structure/architecture/tests), CONTRIBUTING.md
  (layout, module paths), AGENTS.md and CLAUDE.md (module layout, tests
  section) to reflect the package layout.
- `ruff check .` clean. `python -m unittest discover -s tests -v` → 44 passed.

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

## 2026-08-19 12:50 (session: full git history audit)
- Went through the entire `git log` and documented every commit that was
  previously missing from this file, oldest first. Full commit-by-commit
  history below.

### 2026-07-21 12:45 — `df66280` Initial commit
- Created the repository with a one-line `README.md`.

### 2026-07-21 03:15 — `c40a379` Initial plan (copilot-swe-agent)
- Copilot's initial plan commit (no file changes).

### 2026-07-21 03:17 — `32a1757` Add minimal cow weight estimation API and tests (copilot-swe-agent)
- Added `app.py` (116 lines): `CowWeightEstimator` with `api_url`/`api_key`
  constructor args, `estimate()` dispatching to `_estimate_via_ai_api`
  (generic AI API POSTing `{"image", "prompt"}`, `X-API-Key` header, weight
  read from `weight_kg`/`estimate_kg`/`estimated_weight_kg`) or
  `_estimate_fallback` (SHA-256-derived deterministic weight), plus
  `EstimateHandler` serving `POST /estimate-weight` with `image_url` or
  `image_base64` and optional `prompt`.
- Added `tests/test_app.py` (64 lines) and a 2-line `.gitignore`
  (`__pycache__/`, `*.pyc`). Expanded `README.md`.

### 2026-07-21 12:50 — `c6519ec` Merge pull request #1 (braydos-h/copilot/estimate-cow-weight-using-api)
- Merged the Copilot-authored minimal API into `main`.

### 2026-07-21 13:13 — `14bbf05` Add CLAUDE.md guidance for Claude Code
- Added `CLAUDE.md` describing how Claude Code should interact with the repo:
  run/test commands, architecture overview (`CowWeightEstimator` +
  `EstimateHandler`), AI vs local fallback behavior, testing approach, and
  the rule to update `commits.md` after each session.

### 2026-07-21 13:18 — `98bd550` Add Ollama/custom backends and .env loader
- Added stdlib-only `_load_env_file` `.env` loader (existing env vars take
  precedence) and created `.env` with Ollama/default settings.
- Extended `CowWeightEstimator` to three backends: `ollama` (default, POSTs
  base64 image + prompt to local Ollama `/api/generate`, model `llava`,
  weight extracted from text reply — `<n> kg` preferred, else first number),
  `custom` (generic AI API via `AIF_AI_API_URL`, preserving original
  behavior), and `none` (deterministic local fallback).
- Added `_to_base64_image` (URL fetch or base64/data-URI string) and
  `_extract_weight_from_text` helpers.
- Updated tests: HTTP tests force `backend="none"`; new `OllamaEstimatorTests`
  cover backend selection, weight extraction, and data-URI stripping. All 10
  tests pass. Updated `CLAUDE.md` and `README.md`.

### 2026-07-23 21:00 — `d09112f` Add GUI and migrate default to Ollama Cloud
- Added `gui.py` (112 lines): dependency-free Tkinter desktop app
  (`CowWeightApp`) to pick a local cow image, edit the prompt, and display
  the estimate; runs the estimator directly on a background thread so the
  window stays responsive. Added `start_gui.bat` for double-click launch
  with no command window.
- Migrated the default backend to Ollama Cloud
  (`https://ollama.com/api/generate`, model `gemma4:31b`), added
  `OLLAMA_API_KEY` bearer-token auth, improved Ollama HTTP error messages
  (include the cloud API's response detail), and made base64 data-URI
  stripping generic (fixes Windows WebP uploads where the OS reports
  `application/octet-stream`).
- Added a sample cow image asset (Hawthorne Valley Farm webp), updated
  `.env`, README, AGENTS.md, CLAUDE.md, tests, and the session log.

### 2026-07-23 21:06 — (covered above: Ollama Cloud endpoint + API key)
- Switched the default endpoint from the local runtime to the direct Ollama
  Cloud API for `gemma4:31b-cloud`; added `OLLAMA_API_KEY` bearer-token
  support, a clear missing-key error, cloud request tests, and updated
  configuration/docs with API-key setup.

### 2026-07-23 21:10 — (covered above: model name + WebP fixes)
- Fixed direct Ollama Cloud requests to use the API model name `gemma4:31b`
  (the `-cloud` alias is only valid through a local Ollama runtime);
  improved HTTP error detail handling; fixed WebP uploads on Windows.

### 2026-07-23 21:30 — (covered above: default model)
- Changed the default Ollama model to `gemma4:31b-cloud` in the built-in
  default and `.env`; updated README and Claude guidance.

### 2026-07-24 14:23 — `975661a` Update commits.md
- Renamed the file header from "Commits / session log" to "Commits".

### 2026-07-28 13:03 — `aee1e20` Add sample cow image assets
- Added three binary sample images as repository assets:
  `highland-cow-calf-1024x768.jpg` (219,959 B), `images.jpg` (46,356 B), and
  `{C2B7C5D3-E916-42CA-AE8E-C1EC0DF1C631}.png` (528,806 B).

### 2026-07-28 13:04 — `cd523d2` Delete {C2B7C5D3-E916-42CA-AE8E-C1EC0DF1C631}.png
- Removed the GUID-named PNG asset (528,806 B deleted).

### 2026-08-19 11:51 — `ef8ee26` Update .gitignore
- Ignored `497789W-AIF10-AT1-Brayden-Habets [Autosaved].pptx`.

### 2026-08-19 11:56 — `34fa232` Drop custom backend, add Windows build
- Removed the generic AI API path (`_estimate_via_custom_api`, `BACKEND_CUSTOM`,
  `api_url`/`api_key` params, `X-API-Key`/three-key response guessing) from
  `app.py`; aligned docs/tests with the remaining `ollama` + `none` backends.
- Added `.github/workflows/build-windows.yml`: triggers on release publish,
  runs the test suite on `windows-latest`, bundles `gui.py` into a one-file
  Windows `.exe` with PyInstaller, uploads it to the release and as a
  workflow artifact.

### 2026-08-19 11:57 — `b4d0127` Rewrite README documentation
- Expanded `README.md` with a full overview of the API, GUI, configuration,
  testing, architecture, and the Windows release workflow.

### 2026-08-19 11:59 — `56c1454` Organize cow images into cows folder
- Moved the three sample images into `cows/` with a consistent numbered
  scheme: `cow 1.webp`, `cow 2.jpg` (was highland-cow-calf-1024x768.jpg),
  `cow 3.jpg` (was images.jpg).

### 2026-08-19 12:11 — `43c6922` Add structured errors and estimator injection
- Refactored server creation so each `HTTPServer` owns its estimator via
  `create_server(host, port, estimator=None)` (read off
  `self.server.estimator`), removing the class-level handler dependency;
  tests now inject the fallback backend directly.
- Added centralized `setup_logging()`, request/error logging through the
  `aif` logger, and consistent error responses with machine-readable codes
  (`missing_body`, `invalid_json`, `missing_image`, `not_found`,
  `estimation_failed`).
- Added `model_response` to estimator outputs and updated the default model
  to `gemma4:31b-cloud`.

### 2026-08-19 12:13 — `8730de6` Add project metadata and Python 3.11 cleanup
- Added `pyproject.toml` (project metadata, optional `dev`/`gui` extras,
  ruff config with line-length 100 / py311 target / E-F-I-UP-W rules,
  coverage config).
- Modernized type hints to Python 3.11 union syntax (`str | None`) and
  tidied import ordering in `app.py`, `gui.py`, and tests. No behavior
  change.

### 2026-08-19 12:13 — `2a8cc21` Update gui.py (GUI rewrite)
- Rewrote `gui.py` (+206 lines): backend/model selector (combobox + entry,
  no more `.env` editing), indeterminate progress bar, copy-to-clipboard
  button, `Enter` estimates / `Ctrl+Enter` newline, session history panel
  (last 20 estimates: time/image/weight/source), Pillow image preview with
  graceful fallback to filename + byte size, and a read-only view of the
  model's full reply. Window min size 720×560.

### 2026-08-19 12:13 — `2d7d688` Update test_app.py
- Added `GuiSmokeTests` (builds Tk root + `CowWeightApp`, then destroys it)
  and silenced expected exception logging in the 502 test via
  `mock.patch("app.logger")`, asserting the logger was called.

### 2026-08-19 12:13 — `61deeed` Update .env
- Changed `AIF_AI_MODEL` to `gemma4:31b-cloud` in `.env`.

### 2026-08-19 12:16 — `b523c65` Add env example and ignore build artifacts
- Added checked-in `.env.example` with Ollama Cloud defaults; expanded
  `.gitignore` (`.env`, `build/`, `dist/`, `*.spec`, `.coverage`, `htmlcov/`);
  updated AGENTS.md, CLAUDE.md, and README.md to match current backend
  defaults and response shape.

### 2026-08-19 12:25 — `de8b6a7` Update commits.md
- Session log entry for the GUI polish + code quality session.

### 2026-08-19 12:29 — `fde45df` Add configurable Ollama URL to GUI and estimator
- Added `ollama_url` constructor arg to `CowWeightEstimator` (overrides
  `AIF_OLLAMA_URL` env / `DEFAULT_OLLAMA_URL`) and an "Ollama URL" entry in
  the GUI selector frame so the endpoint can be changed at runtime.

### 2026-08-19 12:35 — `d98576c` Add demo cow batch run to GUI
- Added a "Test demo cows" button that runs the estimator over every image
  in `cows/` (sorted), showing per-image progress, the last result, and
  adding each to the session history, on a background thread.

### 2026-08-19 12:36 — `393d364` Wire up retry button state and action
- Retry button is now enabled whenever a previous request exists (after both
  successful and failed estimates). Added `CowWeightApp.retry()` which
  safely reruns `estimate()` (or `estimate_demo_cows()` for demo runs) only
  when `last_request` is present; disabled otherwise.
- Also in this commit: `DEFAULT_PROMPT` rewritten to request a JSON object
  `{weight_kg, confidence, breed, body_condition_score}`; added
  `ImageValidationError`, `_validate_image_bytes` (JPEG/PNG/GIF/BMP/WebP
  magic bytes incl. RIFF….WEBP), `_kg_to_lbs`/`KG_TO_LBS`, per-estimator
  cache keyed by `sha256(base64)` with `AIF_CACHE_TTL` (default 300 s, 0
  disables), `_call_ollama_with_retry` (one retry on 5xx/URLError/Timeout,
  none on 4xx/JSONDecodeError), `_parse_structured_response` (JSON first,
  text fallback), `VERSION = "0.1.0"`, `server_version` header, `do_GET`
  (`/health`, `/`, `/info`, 404 otherwise), `do_OPTIONS` CORS preflight →
  204, per-request `request_id` (8-char uuid hex) in body + `x-request-id`
  header, CORS headers on every response, and `invalid_image` 400 handling.

### 2026-08-19 12:36–12:37 — `6c8e886` / `940e13b` / `a997215` / `b1b0640` Update app.py (docstrings)
- Added the module-level docstring to `app.py` with "How to add a new
  backend" and "How to add a new endpoint" sections, plus docstrings for
  `_kg_to_lbs`, `_validate_image_bytes`, `CowWeightEstimator` (class +
  `estimate`, `_cache_get`, `_cache_put`, `_estimate_via_ollama`,
  `_estimate_fallback`), and `EstimateHandler` (class + `_new_request_id`,
  `_cors_headers`, `do_OPTIONS`, `do_GET`, `do_POST`, `_error`).

### 2026-08-19 12:37 — `2468d3f` / `d1db722` Update gui.py
- `_show_result` and `_show_demo_result` now take the full result dict and
  display `kg / lbs`, plus breed and confidence (as a percentage) in the
  status line; added `_format_result_text` helper.

### 2026-08-19 12:37 — `8b0df09` Document _send_json request ID behavior
- Added a docstring to `EstimateHandler._send_json` clarifying that it sends
  JSON with CORS and `x-request-id` headers and injects `request_id` into
  the body when absent; added GUI method docstrings.

### 2026-08-19 12:38 — `4644a40` Broaden app tests and document GUI methods
- Added comprehensive coverage: valid image fixtures (`_png_bytes()` etc.),
  lbs output, request ID header/body, health and root endpoints,
  CORS/OPTIONS, invalid image errors, structured Ollama response parsing,
  cache TTL behavior, and retry rules (transient vs 4xx). Added method-level
  docstrings in `gui.py` (layout, shortcuts, preview, background flows).

### 2026-08-19 12:42 — `c68bcb0` Expand API docs and cache TTL configuration
- Updated README with API capabilities/endpoint behavior (request IDs, CORS
  preflight, health/root routes, structured output, retry/cache/image-
  validation notes); added `AIF_CACHE_TTL` to `.env` and `.env.example`;
  small cleanups in app.py error-detail handling, GUI callback docstrings,
  and test formatting.

### 2026-08-19 12:43 — `62f8163` Document API and improve estimator behavior
- Added `CONTRIBUTING.md` (127 lines: conventions, repository layout,
  commands, step-by-step guides for new backend/endpoint/weight-parsing/GUI
  changes, testing notes, pre-commit checklist); expanded README; session
  log entry.

### 2026-08-19 12:43 — `b1a5db0` Refactor Ollama error detail handling
- Replaced the inline conditional computing `detail` from Ollama error
  responses with an explicit `if/else` for clearer string vs non-string
  handling (no behavior change); added the missing trailing newline at the
  end of `app.py`.

### 2026-08-19 12:44 — `b738a07` / `d9b94d6` Update test_app.py
- Final test-suite churn: reworked the 502/400 tests to silence expected
  exception logging with `mock.patch("app.logger")` and assert the logger
  was called, keeping the test run output clean. Suite at 43 tests, all
  passing; `ruff check .` clean.

### 2026-08-19 13:09 - Add Ollama API key field to GUI
- Added a masked 'API key' entry to the GUI selector area (aif/gui.py), pre-filled from the OLLAMA_API_KEY env var and passed to the estimator on every request.
