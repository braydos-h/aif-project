# Commits / session log

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