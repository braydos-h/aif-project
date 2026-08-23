"""Compatibility launcher for the browser-based WebUI.

Use ``python app.py`` for a console-friendly server start. This legacy entry
point starts the same Rust server and opens the WebUI in the default browser.
"""

from app import main

__all__ = ["main"]

if __name__ == "__main__":
    main(open_browser=True)
