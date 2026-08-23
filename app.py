"""Launcher for the Rust backend and its same-origin WebUI.

``python app.py`` starts the compiled ``aif-backend`` binary. The Rust server
serves both the browser application and the JSON API; this module never runs
a second web server.

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


def main(open_browser: bool = False) -> None:
    """Start the Rust WebUI/API server on 127.0.0.1:8080.

    Args:
        open_browser: Open the local WebUI after spawning the backend.
    """
    try:
        binary = find_binary()
    except FileNotFoundError as exc:
        print(str(exc), file=sys.stderr)
        sys.exit(1)
    print(f"Cow Weight Estimator (aif-backend {VERSION})", flush=True)
    print("WebUI: http://127.0.0.1:8080/", flush=True)
    print("API:   http://127.0.0.1:8080/estimate-weight", flush=True)
    if open_browser:
        import webbrowser

        webbrowser.open("http://127.0.0.1:8080/")
    subprocess.run([binary, "--host", "127.0.0.1", "--port", "8080"], check=False)


if __name__ == "__main__":
    main()
