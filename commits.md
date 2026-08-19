# Commits

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
