"""Reusable Python configuration and estimation helpers.

The browser application and HTTP API run in the Rust backend. The Python
package remains available for scripts and tests that need the estimator
directly, while ``app.py`` launches the Rust-served WebUI.
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
    repo_root,
    setup_logging,
)
from .estimator import CowWeightEstimator, ImageValidationError

__all__ = [
    "CowWeightEstimator",
    "DEFAULT_CACHE_TTL",
    "DEFAULT_OLLAMA_MODEL",
    "DEFAULT_OLLAMA_URL",
    "DEFAULT_PROMPT",
    "IMAGE_MAGIC_BYTES",
    "ImageValidationError",
    "KG_TO_LBS",
    "OLLAMA_MAX_RETRIES",
    "OLLAMA_RETRY_BACKOFF",
    "VERSION",
    "repo_root",
    "setup_logging",
]
