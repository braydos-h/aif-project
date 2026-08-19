"""Entry point for the HTTP API server.

``python app.py`` launches the Rust backend (``backend/``, crate
``aif-backend``) which serves the same API the Python server used to: a
threaded HTTP/1.1 server with per-request parallelism. If the release binary
is missing, prints build instructions and exits.

Env var ``AIF_BACKEND_BIN`` overrides the binary path (useful for tests and
CI). Default is ``backend/target/release/aif-backend(.exe)`` relative to
this repository.
"""

import os
import subprocess
import sys

from aif import VERSION

BINARY_NAMES = ("aif-backend.exe", "aif-backend")


def _repo_root() -> str:
    return os.path.dirname(os.path.abspath(__file__))


def find_binary() -> str:
    """Locate the Rust backend binary, honoring ``AIF_BACKEND_BIN``."""
    override = os.environ.get("AIF_BACKEND_BIN")
    if override:
        return override
    candidates = []
    for name in BINARY_NAMES:
        candidates.append(os.path.join(_repo_root(), "backend", "target", "release", name))
    for candidate in candidates:
        if os.path.isfile(candidate):
            return candidate
    raise FileNotFoundError(
        "Rust backend binary not found. Build it with:\n"
        "    cargo build --release --manifest-path backend/Cargo.toml"
    )


def main() -> None:
    """Start the Rust HTTP API server on 127.0.0.1:8080."""
    try:
        binary = find_binary()
    except FileNotFoundError as exc:
        print(str(exc), file=sys.stderr)
        sys.exit(1)
    print(
        f"Cow weight estimation API (aif-backend {VERSION}) "
        f"listening on http://127.0.0.1:8080"
    )
    subprocess.run([binary, "--host", "127.0.0.1", "--port", "8080"], check=False)


if __name__ == "__main__":
    main()
