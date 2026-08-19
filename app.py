"""Backward-compatible entry point for the HTTP API server.

The actual server lives in ``aif.server``; this wrapper keeps
``python app.py`` working unchanged. Prefer importing from ``aif``:
``from aif import CowWeightEstimator, create_server``.
"""

from aif import (
    DEFAULT_CACHE_TTL,
    DEFAULT_OLLAMA_MODEL,
    DEFAULT_OLLAMA_URL,
    DEFAULT_PROMPT,
    IMAGE_MAGIC_BYTES,
    KG_TO_LBS,
    OLLAMA_MAX_RETRIES,
    OLLAMA_RETRY_BACKOFF,
    VERSION,
    CowWeightEstimator,
    EstimateHandler,
    ImageValidationError,
    create_server,
    setup_logging,
)

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


def main() -> None:
    """Start the HTTP API server on 127.0.0.1:8080."""
    setup_logging()
    server = create_server()
    print("Cow weight estimation API listening on http://127.0.0.1:8080")
    server.serve_forever()


if __name__ == "__main__":
    main()
