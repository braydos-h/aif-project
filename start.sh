#!/bin/sh
# Start the release backend and open the WebUI (Linux/macOS).
# Mirrors start_gui.bat / start_gui.ps1.
set -eu
cd "$(dirname "$0")"

BACKEND="backend/target/release/aif-backend"
if [ ! -x "$BACKEND" ]; then
    echo "Error: backend binary not found at \"$BACKEND\"" >&2
    echo "Build it with: cargo build --release --manifest-path backend/Cargo.toml" >&2
    exit 1
fi

"$BACKEND" --host 127.0.0.1 --port 8080 &
SERVER_PID=$!

URL="http://127.0.0.1:8080/"
if command -v xdg-open >/dev/null 2>&1; then
    xdg-open "$URL" >/dev/null 2>&1 &
elif command -v open >/dev/null 2>&1; then
    open "$URL" &
else
    echo "Open $URL"
fi

wait "$SERVER_PID"
