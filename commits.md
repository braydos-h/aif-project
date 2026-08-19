# Commits

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
