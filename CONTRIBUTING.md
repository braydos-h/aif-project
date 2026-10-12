# Contributing

The repository contains the Rust-served web app and an independent native
SwiftUI iOS app. Read [AGENTS.md](AGENTS.md) for repository-specific rules,
[README.md](README.md) for the product overview, and [ios/README.md](ios/README.md)
for the iOS build workflow.

## Ground rules

- Keep the frontend plain HTML, CSS, and modern browser JavaScript. Do not add
  React, Node/npm, a bundler, or a Python web framework for ordinary UI work.
- Keep HTTP behavior in Rust. The server has no async runtime or web
  framework; it uses `std::net` with one thread per connection.
- Keep iOS code in Swift and use the Apple frameworks and offline-first
  architecture described in `ios/README.md`. Do not wrap the web app in a
  WebView or add secrets to source control.
- The Rust dependencies are `ureq` with rustls, `serde_json`, and `sha2`.
  Integration tests use only `std` plus `serde_json` (already a dependency).
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
| Build the web server | `cd web; cargo build --release --manifest-path backend/Cargo.toml` |
| Run the web app | `web/backend/target/release/aif-backend --host 0.0.0.0 --port 8080` |
| Run the browser-opening launcher | `web/start_gui.bat` (or `web/start_gui.ps1`) |
| Rust tests (unit + HTTP integration) | `cd web; cargo test --manifest-path backend/Cargo.toml` |
| Lint (format check) | `cd web; cargo fmt --manifest-path backend/Cargo.toml -- --check` |
| Lint (Clippy, required in CI) | `cd web; cargo clippy --manifest-path backend/Cargo.toml --all-targets --all-features -- -D warnings` |
| WebUI syntax check (required in CI) | `node --check web/app.js` |
| Browser journey | `cd web; npm ci; npm run test:browser` |
| iOS build and tests | `xcodebuild -project ios/AIF.xcodeproj -scheme AIF -destination 'platform=iOS Simulator,name=iPhone 16' CODE_SIGNING_ALLOWED=NO test` (macOS + Xcode 15+) |

Visit `http://127.0.0.1:8080/` after starting the server. `AIF_BACKEND_BIN`
overrides the binary used by the integration tests.

## Repository layout

```text
web/backend/src/
├── args.rs         --host/--port CLI parsing
├── hash.rs         shared SHA-256 helpers
├── config/         defaults + Config (mod.rs), .env loading (env.rs), units (units.rs)
├── validate/       image refs (mod.rs), base64 codec (base64.rs), magic bytes (image.rs), URL fetch (fetch.rs)
├── parse/          weight extraction (mod.rs), JSON blocks (structured.rs), free text (text.rs)
├── fallback.rs     deterministic offline estimate
├── tape.rs         Schaeffer tape estimator, ranges, dosing disclaimer
├── cache.rs        in-memory TTL result cache
├── ollama/         orchestration (mod.rs), transport/retry (client.rs), reply parsing (response.rs), cache keys (cache_key.rs)
├── http/           dispatch (mod.rs), server loop (server.rs), responses (response.rs), request ids (request_id.rs),
│                   static/demo registry (assets.rs), option validation (validation.rs), info/demo/metrics (handlers.rs), estimator API (estimate.rs)
└── main.rs         --host/--port entry point
web/                index.html, styles.css, app.js, account UI, browser tests
web/cows/           approved demo images compiled into the server
web/backend/tests/  real-HTTP integration tests and static WebUI guards
ios/                native SwiftUI application, services, and tests
web/start_gui.*     launch the release binary and open the WebUI
```

For iOS changes, keep persistent models in `ios/AIF/Models`, local file and
record operations in `ios/AIF/Services`, and SwiftUI screens in
`ios/AIF/Features`. Keep credentials in Keychain, exclude them from backup
formats, and require explicit confirmation before a photo is sent to a cloud
provider. Local photo inference must remain unavailable unless an
independently validated cattle-weight model is installed; never return a
fabricated estimate.

## Adding a WebUI feature

1. Reuse native browser capabilities before adding code or dependencies.
2. Keep markup semantic and controls labelled. Preserve visible focus states,
   keyboard operation, live status text, responsive layout, and dark-mode
   readability.
3. Keep dynamic values in DOM properties such as `textContent`, `src`, and
   `value`. Never use `innerHTML` for model responses, filenames, errors, or
   request data.
4. If an API route is needed, add an explicit `(method, path)` match in
   `web/backend/src/http/mod.rs`. Do not add arbitrary static file serving.
5. Add an HTTP-level test in `web/backend/tests/server.rs` for the status, content
   type, response shape, and security boundary.

## Adding an API field or option

Optional request fields must leave old clients working. Validate type, size,
allowed values, and URL shape before use. Derive a request-local `Config` from
the server default; do not call `set_var` or log the request body. Document
the field and its error code in README.md, and test both omission and use.

## Demo images and static assets

Demo images are approved at compile time in `web/backend/src/http/assets.rs`. If a demo
is added, give it a fixed ID and MIME type and add it to the controlled list;
never construct a filesystem path from a URL segment. WebUI assets are also
embedded with `include_bytes!`, which keeps the release binary self-contained.

## Before finishing

Run the Rust tests, release build, and `cargo fmt --check`. Inspect `git diff`
for unrelated changes. Append a dated summary to `commits.md`; append only —
never rewrite or delete earlier entries.
