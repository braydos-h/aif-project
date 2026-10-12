# AIF Web Application

This folder contains the original Rust-powered HTTP server and browser
application. It remains independently buildable and deployable; it does not
depend on the native iOS project.

## Requirements and quick start

Install the pinned Rust toolchain from `rust-toolchain.toml`. Build and start
the server from this folder:

```sh
cargo build --release --manifest-path backend/Cargo.toml
./backend/target/release/aif-backend --host 127.0.0.1 --port 8080
```

Then open <http://127.0.0.1:8080/>. On Windows, run
`install.ps1` for first-time setup or `start_gui.bat` after building. On
Linux/macOS, `start.sh` starts the release binary and opens the browser.
See the [repository README](../README.md) for web behavior and the
[`docs/`](docs/) directory for deployment, operations, security, privacy, and
recovery documentation.

## Tests

```sh
cargo fmt --manifest-path backend/Cargo.toml -- --check
cargo test --manifest-path backend/Cargo.toml
cargo build --release --manifest-path backend/Cargo.toml
npm ci
npm run test:browser
```

Browser journeys need the release binary and Playwright's Chromium browser.
