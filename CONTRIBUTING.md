# Contributing

The project is a Rust-served local WebUI/API with a small dependency-free
Python estimator library. Read [AGENTS.md](AGENTS.md) for repository-specific
agent rules and [README.md](README.md) for the user/API documentation.

## Ground rules

- Keep the frontend plain HTML, CSS, and modern browser JavaScript. Do not add
  React, Node/npm, a bundler, or a Python web framework for ordinary UI work.
- Keep HTTP behavior in Rust. The server has no async runtime or web
  framework; it uses `std::net` with one thread per connection.
- The Rust dependencies are `ureq` with rustls, `serde_json`, and `sha2`.
  Python runtime code remains standard-library-only.
- Preserve image magic-byte validation, request body limits, CORS support,
  request IDs, structured error codes, and secret redaction.
- Render user/model text safely. The WebUI must use `textContent` or DOM node
  APIs for dynamic values, never string-built HTML.
- Runtime request overrides must clone the server `Config`; never mutate
  process environment variables or shared configuration.
- Public Rust items get `///` documentation. Keep new logic small and add a
  focused test when behavior is not a trivial one-liner.

## Commands

| Task | Command |
| --- | --- |
| Build the server | `cargo build --release --manifest-path backend/Cargo.toml` |
| Run the application | `python app.py` |
| Run the browser-opening compatibility launcher | `python gui.py` |
| Rust tests | `cargo test --manifest-path backend/Cargo.toml` |
| Python tests | `python -m unittest discover -s tests -v` |
| Lint | `ruff check .` |

Visit `http://127.0.0.1:8080/` after starting the server. `AIF_BACKEND_BIN`
overrides the binary used by `app.py` and the HTTP tests.

## Repository layout

```text
backend/src/
├── config.rs       .env loader, defaults, and effective Config
├── validate.rs     base64/data-URI and image magic-byte validation
├── parse.rs        structured JSON and text weight extraction
├── fallback.rs     deterministic offline estimate
├── cache.rs        in-memory TTL result cache
├── ollama.rs       Ollama client, bearer auth, and retry policy
├── http.rs         threaded HTTP server, WebUI routes, API, demos
└── main.rs         --host/--port entry point
web/                index.html, styles.css, app.js
cows/               approved demo images compiled into the server
aif/                Python config and reusable estimator library
app.py              Rust launcher
gui.py              legacy WebUI launcher
tests/              Python HTTP, estimator, config, and launcher tests
```

## Adding a WebUI feature

1. Reuse native browser capabilities before adding code or dependencies.
2. Keep markup semantic and controls labelled. Preserve visible focus states,
   keyboard operation, live status text, responsive layout, and dark-mode
   readability.
3. Keep dynamic values in DOM properties such as `textContent`, `src`, and
   `value`. Never use `innerHTML` for model responses, filenames, errors, or
   request data.
4. If an API route is needed, add an explicit `(method, path)` match in
   `backend/src/http.rs`. Do not add arbitrary static file serving.
5. Add an HTTP-level test in `tests/test_server.py` for the status, content
   type, response shape, and security boundary.

## Adding an API field or option

Optional request fields must leave old clients working. Validate type, size,
allowed values, and URL shape before use. Derive a request-local `Config` from
the server default; do not call `set_var` or log the request body. Document
the field and its error code in README.md, and test both omission and use.

## Demo images and static assets

Demo images are approved at compile time in `backend/src/http.rs`. If a demo
is added, give it a fixed ID and MIME type and add it to the controlled list;
never construct a filesystem path from a URL segment. WebUI assets are also
embedded with `include_bytes!`, which keeps the release binary self-contained.

## Before finishing

Run the Rust tests, release build, Python tests, and ruff. Inspect `git diff`
for unrelated changes. Append a dated summary to `commits.md`; append only —
never rewrite or delete earlier entries.
