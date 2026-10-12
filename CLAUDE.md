# CLAUDE.md

This repository contains two independent products: a Rust-served web app in
`web/` and a native SwiftUI iPhone app in `ios/`. The iOS app must not depend
on the web server or wrap the browser UI. Web launchers and tests are in
`web/` and `web/backend/tests/`.

Follow [AGENTS.md](AGENTS.md) and [CONTRIBUTING.md](CONTRIBUTING.md). In
particular:

- keep the frontend dependency-free HTML/CSS/vanilla JavaScript;
- keep HTTP routes and configuration behavior in Rust;
- preserve existing API clients, image validation, body limits, request IDs,
  CORS, error codes, and deterministic fallback parity;
- derive per-request overrides from a cloned `Config`, never process env;
- never log/return API keys or persist them in browser storage/history;
- use explicit static/demo routes only, never arbitrary filesystem serving;
- render dynamic browser values with DOM APIs/textContent, not innerHTML;
- append a dated entry to `commits.md` at the end of every session.

Build and test with:

```bash
cd web
cargo build --release --manifest-path backend/Cargo.toml
cargo test --manifest-path backend/Cargo.toml
```

On macOS with Xcode 15 or later, build and test the iOS project separately:

```bash
xcodebuild -project ios/AIF.xcodeproj -scheme AIF \
  -destination 'platform=iOS Simulator,name=iPhone 16' \
  CODE_SIGNING_ALLOWED=NO test
```
