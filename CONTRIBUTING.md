# Contributing

Thanks for wanting to add code to the cow weight estimator. This guide walks
through the project's shape, conventions, and the exact steps for the most
common changes. The full API/user docs live in [README.md](README.md);
agent-specific notes live in [AGENTS.md](AGENTS.md) and
[CLAUDE.md](CLAUDE.md).

## Ground rules

- **Standard library only.** No new runtime dependencies. If you want a
  library, first reach for `urllib`, `re`, `hashlib`, `json`, etc. Optional
  extras (Pillow for GUI previews) go in `pyproject.toml`'s
  `[project.optional-dependencies]` and must degrade gracefully when missing.
- **No `pip install` needed to run.** Keep it that way.
- **Docstrings on everything.** Every public class and method carries a
  docstring explaining what it does, its arguments/returns, and what it
  raises. Follow the existing style (see `app.py`).
- **Follow the layering.** Estimation logic lives in `app.py`'s
  `CowWeightEstimator`; HTTP lives in `EstimateHandler`; UI lives in
  `gui.py`. `gui.py` never talks HTTP, and neither layer re-implements the
  other.
- **Error messages are structured.** HTTP errors carry a machine-readable
  `code` plus a human `error` message. New failure modes get a new code
  string, documented in the handler docstring and README.

## Environment & commands

| Task                          | Command                                        |
| ----------------------------- | ---------------------------------------------- |
| Run the API server            | `python app.py` (listens on `127.0.0.1:8080`)  |
| Run the desktop GUI           | `python gui.py`                                |
| Run all tests                 | `python -m unittest discover -s tests -v`      |
| Run one test                  | `python -m unittest tests.test_app.<TestClass>.<test_name> -v` |
| Lint                          | `ruff check .`                                 |
| Auto-fix lint                 | `ruff check --fix .`                           |

Before finishing any change: all tests pass and `ruff check .` is clean.

## Repository layout

```
app.py           HTTP API + CowWeightEstimator (the core logic)
gui.py           Tkinter desktop app; reuses app.CowWeightEstimator
tests/test_app.py  HTTP + unit + GUI smoke tests
pyproject.toml   Metadata + ruff config + optional extras
.env.example     Committed config template; copy to .env locally
cows/            Demo images used by the GUI's "Test demo cows" button
.github/workflows/build-windows.yml  Release: tests + PyInstaller .exe
commits.md       Changelog of sessions; append a dated entry after every session
```

## How the pieces fit

- `CowWeightEstimator` is created with `backend`, `model`, `ollama_url`,
  `cache_ttl` (constructor args fall back to env vars / `.env`). Its
  `estimate(image_reference, prompt)` returns a dict with at least
  `estimated_weight_kg`, `estimated_weight_lbs`, `source`, `prompt_used`
  (plus `model`, `model_response` and optional `confidence`/`breed`/
  `body_condition_score` for the ollama backend).
- The HTTP server is built by `create_server(host, port, estimator)` and
  injects the estimator onto `server.estimator`. `EstimateHandler` reads it
  from there — never import or instantiate estimators inside the handler.
- The GUI builds a **fresh** `CowWeightEstimator` per request from the
  UI's backend/model/URL fields, so you don't need to restart it to switch.
- Both `app.py` and `gui.py` read `.env` at import time. If you add a new
  env var, add it to `.env.example` (never `.env` — it's gitignored and
  may contain a real key) and the config table in `README.md`.

## Common changes

### Add a new estimation backend

1. Write a private method `_estimate_via_<name>(image_reference, prompt)`
   on `CowWeightEstimator` returning the same dict shape (see above) and
   raising `ValueError` on failure.
2. Accept the backend name in `__init__` (constructor arg and/or a new
   `AIF_*` env var) and dispatch to it in `estimate()`.
3. Add the option to the GUI combobox (`BACKEND_CHOICES` in `gui.py`) and
   handle its config fields.
4. Document in README (config table), `.env.example`, `AGENTS.md`/`CLAUDE.md`
   "Backend selection" section, and add tests in `tests/test_app.py`.

### Add a new HTTP endpoint

1. Add a `do_<METHOD>` method on `EstimateHandler`.
2. Set `self.request_id = self._new_request_id()` first; respond with
   `self._send_json(...)` for success or `self._error(code, message, status)`
   for failure (new `code` string, documented in the handler docstring).
3. Add `GET /info`'s `endpoints` list entry, README API reference, and an
   HTTP-level test (boot `create_server(port=0, ...)`, like `EstimateApiTests`).

### Change how weights are parsed from model output

- Structured-JSON-first parsing lives in `CowWeightEstimator._parse_structured_response`
  (returns `(weight_kg, extras_dict)`).
- Free-text fallback lives in `_extract_weight_from_text` (prefers `<n> kg`,
  then the first bare number).
- Add cases to `StructuredResponseTests` / `OllamaEstimatorTests`.

### Add a GUI feature

- Build widgets in `_build_layout` (grid rows; the history treeview is the
  stretch row). Button commands are `CowWeightApp` methods.
- Anything that can block (network, encoding) goes on a daemon thread using
  the existing pattern: disable buttons → `progress.start()` → spawn
  `threading.Thread(..., daemon=True)` → update widgets from the thread via
  `root.after(0, ...)`. Never touch Tk widgets from a background thread.
- Extend `GuiSmokeTests` for layout regressions.

## Testing notes

- HTTP tests use `create_server(port=0, estimator=...)` on a background
  thread, then hit it over real sockets — keep new endpoint tests in that
  style so status codes and serialization are exercised.
- Unit tests that touch Ollama mock `urllib.request.urlopen`; the live
  network path is never exercised in CI.
- The `none` backend is deterministic (SHA-256 of the image reference), so
  HTTP tests use it to get stable weights.

## Before you commit

1. `python -m unittest discover -s tests -v` — all green.
2. `ruff check .` — clean.
3. Docs updated: README (API/config changes), `.env.example` (new env
   vars), AGENTS.md/CLAUDE.md if behavior/architecture changed.
4. Append an entry to `commits.md` (date/time + summary of what you did).
