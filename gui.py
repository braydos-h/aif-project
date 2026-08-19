"""Backward-compatible entry point for the desktop GUI.

The Tkinter app lives in ``aif.gui``; this wrapper keeps
``python gui.py`` (and ``start_gui.bat``) working unchanged. The app
imports estimators from ``aif.estimator`` directly — no HTTP server runs.
"""

from aif.gui import CowWeightApp, main

__all__ = ["CowWeightApp", "main"]

if __name__ == "__main__":
    main()
