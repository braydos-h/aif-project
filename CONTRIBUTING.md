# Contributing

Thanks for wanting to add code to the cow weight estimator. This guide walks
through the project's shape, conventions, and the exact steps for the most
common changes. The full API/user docs live in [README.md](README.md);
agent-specific notes live in [AGENTS.md](AGENTS.md) and
[CLAUDE.md](CLAUDE.md).

## Ground rules

- **Python is stdlib-only; Rust has three dependencies.** The GUI
  (`aif/`) is pure standard library (`urllib`, `re`, `hashlib`, `json`,
  `tkinter`, …). The Rust backend (`backend/`) depends only on `ureq` (with
  rustls TLS), `serde_json`, and `sha2` — no async runtime, no web framework.
  If you want a library, first reach for the standard library of either
  language. Optional extras (Pillow for GUI previews) go in
  `pyproject.toml`'s `[project.optional-dependencies]` and must degrade
  gracefully when missing.
- **No `pip install` needed to run.** Keep it that way.
- **Docstrings on everything.** Every public class/method/function carries a
  docstring explaining what it does, its arguments/returns, and what it
  raises. Follow the existing style (see `aif/estimator.py`); Rust items get
  `///` docs in the same spirit.
- **Follow the layering.** GUI + Python estimator live in `aif/`; the HTTP
  API lives in Rust (`backend/`). `gui.py` never talks HTTP, and the Rust
  server never imports Python. When the same logic exists in both (e.g. the
  `none` backend's math, weight extraction), it must stay **behaviorally
  identical** — Rust and Python implementations of the same contract are
  parity-checked by tests.
- **Error messages are structured.** HTTP errors carry a machine-readable
  `code` plus a human `error` message. New failure modes get a new code
  string, documented in `backend/src/http.rs` and README.

## Environment & commands

| Task                          | Command                                        |
| ----------------------------- | ---------------------------------------------- |
| Build the Rust server         | `cargo build --release --manifest-path backend/Cargo.toml` |
| Run the API server            | `python app.py` (spawns the Rust binary; listens on `127.0.0.1:8080`) |
| Run the desktop GUI           | `python gui.py`                                |
| Run Rust tests                | `cargo test --manifest-path backend/Cargo.toml` |
| Run all Python tests          | `python -m unittest discover -s tests -v` (needs the release binary) |
| Run one Python test           | `python -m unittest tests.test_estimator.OllamaEstimatorTests.test_cloud_endpoint_uses_bearer_token -v` |
| Lint                          | `ruff check .`                                 |
| Auto-fix lint                 | `ruff check --fix .`                           |

`app.py` at the repo root is a thin wrapper that launches the Rust backend
(override the binary path with `AIF_BACKEND_BIN`); `gui.py` is a wrapper over
`aif.gui`. Import the package (`from aif import ...`) in new Python code
rather than the wrappers.

Before finishing any change: `cargo test`, `python -m unittest discover -s
tests -v`, and `ruff check .` are all clean.

## Repository layout

```
backend/
├── Cargo.toml           Crate metadata + the three dependencies
└── src/
    ├── config.rs        .env loader, defaults, env-var precedence
    ├── validate.rs      base64 decoding + image magic-byte validation
    ├── parse.rs         structured-JSON-first / free-text weight extraction
    ├── fallback.rs      deterministic SHA-256-derived estimate
    ├── cache.rs         in-memory TTL result cache
    ├── ollama.rs        Ollama Cloud client (bearer auth, retry policy)
    ├── http.rs          hand-rolled threaded HTTP/1.1 server
    ├── lib.rs           module wiring
    └── main.rs          entry point (--host / --port flags)
aif/
├── __init__.py     Package exports (estimator, constants)
├── config.py       Constants, defaults, .env loader, setup_logging
├── estimator.py    CowWeightEstimator + image validation (GUI only)
└── gui.py          Tkinter desktop app; reuses aif.estimator
app.py              Thin wrapper: python app.py runs the Rust API server
gui.py              Thin wrapper: python gui.py runs the desktop app
tests/
├── test_server.py    HTTP request/response suite (real sockets → Rust binary)
├── test_estimator.py Estimator unit tests (parsing, cache, retry, auth)
└── test_gui.py       GUI smoke tests
pyproject.toml      Python metadata + ruff config + optional extras
.env.example        Committed config template; copy to .env locally
cows/               Demo images used by the GUI's "Test demo cows" button
.github/workflows/build-windows.yml  Release: Rust build + tests + PyInstaller .exe
commits.md          Changelog of sessions; append a dated entry after every session
```

## How the pieces fit

- The **Rust server** reads `.env`/env vars itself (`backend/src/config.rs`,
  same precedence rules as `aif/config.py`), builds a `Config` +
  `Cache::new(ttl)`, and serves the API on `std::net::TcpListener` with one
  thread per connection — slow Ollama requests never block each other. Its
  `estimate` path mirrors `CowWeightEstimator.estimate()` for both backends.
- **`CowWeightEstimator`** (`aif/estimator.py`) is now GUI-only: the desktop
  app builds a **fresh** estimator per request from the UI's
  backend/model/URL fields. The Python server (`aif/server.py`) is gone —
  do not recreate it; new HTTP work belongs in `backend/`.
- The `aif` package reads `.env` at import time. The Rust binary reads it at
  startup. If you add a new env var, add it to `.env.example` (never `.env`
  — it's gitignored and may contain a real key) and the config table in
  `README.md`.

## Common changes

### Add a new estimation backend

The backend must work in **both** languages (parity contract):

1. Rust: write a branch in `estimate_via_*` dispatch in
   `backend/src/http.rs` (or a new module) that returns the same dict shape
   (`estimated_weight_kg`, `estimated_weight_lbs`, `source`, `prompt_used`,
   plus extras) and surfaces failures as errors mapped to `502
   estimation_failed`.
2. Python: write a private method `_estimate_via_<name>` on
   `CowWeightEstimator` in `aif/estimator.py`, returning the same dict shape
   and raising `ValueError` on failure.
3. Accept the backend name in both configs (env var `AIF_AI_BACKEND` +
   constructor arg) and dispatch to it.
4. Add the option to the GUI combobox (`BACKEND_CHOICES` in `aif/gui.py`).
5. Document in README (config table), `.env.example`, AGENTS.md/CLAUDE.md
   "Backend selection" section, and add tests on both sides.

### Add a new HTTP endpoint

All HTTP work is Rust now:

1. Add a route to `dispatch()` in `backend/src/http.rs` (match on
   method + path).
2. Build the response with `Response::json(status, payload)` for success or
   `error_json(code, message, request_id)` for failure (new `code` string,
   documented in the module docstring).
3. Update `GET /`'s `endpoints` list, the README API reference, and add an
   HTTP-level test (spawn the binary, like `EstimateApiTests`).

### Change how weights are parsed from model output

- Rust: `backend/src/parse.rs` — `parse_structured_response` (JSON first,
  returns `(weight, extras)`), `extract_weight_from_text` (prefers `<n> kg`,
  then the first bare number). Add cases to its `#[cfg(test)]` module.
- Python: `CowWeightEstimator._parse_structured_response` /
  `_extract_weight_from_text` in `aif/estimator.py` must stay behaviorally
  identical. Add cases to `StructuredResponseTests` /
  `OllamaEstimatorTests`.
- The GUI and the server must extract the same weight from the same reply.

### Add a GUI feature

- Build widgets in `_build_layout` (grid rows; the history treeview is the
  stretch row). Button commands are `CowWeightApp` methods.
- Anything that can block (network, encoding) goes on a daemon thread using
  the existing pattern: disable buttons → `progress.start()` → spawn
  `threading.Thread(..., daemon=True)` → update widgets from the thread via
  `root.after(0, ...)`. Never touch Tk widgets from a background thread.
- Extend `GuiSmokeTests` for layout regressions.

## Testing notes

- HTTP tests spawn the compiled Rust binary on a free port (env
  `AIF_BACKEND_BIN` overrides the path) and hit it over real sockets — keep
  new endpoint tests in that style so status codes and serialization are
  exercised. Build the release binary before running them.
- Rust unit tests live next to each module (`#[cfg(test)]`) in `backend/src/`.
- Unit tests that touch Ollama mock `urllib.request.urlopen`; the live
  network path is never exercised in CI.
- The `none` backend is deterministic (SHA-256 of the image reference), so
  HTTP tests use it to get stable weights.
- When mocking module-level names in Python, patch `aif.estimator.logger`
  (not the root `aif` logger).

## Before you commit

1. `cargo test --manifest-path backend/Cargo.toml` — all green.
2. `python -m unittest discover -s tests -v` — all green (release binary built).
3. `ruff check .` — clean.
4. Docs updated: README (API/config changes), `.env.example` (new env
   vars), AGENTS.md/CLAUDE.md if behavior/architecture changed.
5. Append an entry to `commits.md` (date/time + summary of what you did).
