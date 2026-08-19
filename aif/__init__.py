"""Cow Weight Estimator — estimate a cow's weight from an image.

A dependency-free package split into focused modules:

- ``aif.config`` — constants, defaults, and the stdlib-only ``.env`` loader.
- ``aif.estimator`` — ``CowWeightEstimator`` (backend-agnostic estimation).
- ``aif.server`` — HTTP API (``EstimateHandler`` + ``create_server``).
- ``aif.gui`` — Tkinter desktop app (``CowWeightApp``).

Entry points ``app.py`` and ``gui.py`` at the repository root are thin
wrappers around ``aif.server`` and ``aif.gui`` so ``python app.py`` and
``python gui.py`` keep working unchanged.
"""

from .config import (
    DEFAULT_CACHE_TTL,
    DEFAULT_OLLAMA_MODEL,
    DEFAULT_OLLAMA_URL,
    DEFAULT_PROMPT,
    IMAGE_MAGIC_BYTES,
    KG_TO_LBS,
    OLLAMA_MAX_RETRIES,
    OLLAMA_RETRY_BACKOFF,
    VERSION,
    setup_logging,
)
from .estimator import CowWeightEstimator, ImageValidationError
from .server import EstimateHandler, create_server

__all__ = [
    "CowWeightEstimator",
    "DEFAULT_CACHE_TTL",
    "DEFAULT_OLLAMA_MODEL",
    "DEFAULT_OLLAMA_URL",
    "DEFAULT_PROMPT",
    "EstimateHandler",
    "IMAGE_MAGIC_BYTES",
    "ImageValidationError",
    "KG_TO_LBS",
    "OLLAMA_MAX_RETRIES",
    "OLLAMA_RETRY_BACKOFF",
    "VERSION",
    "create_server",
    "setup_logging",
]
