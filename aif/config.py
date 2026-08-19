"""Configuration and constants for the cow weight estimator.

All tunables live here and are loaded once at import time from environment
variables, falling back to a ``.env`` file in the repository root
(``_load_env_file``). Values already in the environment take precedence
over ``.env``.
"""

import logging
import os

DEFAULT_PROMPT = (
    "Estimate this cow's weight in kilograms from the provided image. "
    'Reply with ONLY a JSON object of the form '
    '{"weight_kg": <number>, "confidence": <0..1>, '
    '"breed": <string>, "body_condition_score": <1..9>} '
    "where confidence is your confidence in the estimate (0..1), breed is your "
    'best guess of the breed (or "unknown"), and body_condition_score is a '
    "1-9 score. Do not include any text outside the JSON object."
)

# Default backend is Ollama Cloud. Override via .env / env vars.
DEFAULT_OLLAMA_URL = "https://ollama.com/api/generate"
DEFAULT_OLLAMA_MODEL = "gemma4:31b-cloud"

# In-memory result cache. TTL=0 disables caching entirely.
DEFAULT_CACHE_TTL = 300

# Retry policy for transient Ollama failures (5xx / network errors).
OLLAMA_MAX_RETRIES = 1
OLLAMA_RETRY_BACKOFF = 1.0  # seconds

KG_TO_LBS = 2.20462

# Magic bytes for the image formats we accept.
IMAGE_MAGIC_BYTES = (
    b"\xff\xd8\xff",  # JPEG
    b"\x89PNG\r\n\x1a\n",  # PNG
    b"GIF87a",  # GIF87a
    b"GIF89a",  # GIF89a
    b"BM",  # BMP
    b"RIFF",  # WebP (RIFF....WEBP)
)

logger = logging.getLogger("aif")

VERSION = "0.1.0"


def setup_logging(level: str = "INFO") -> None:
    """Configure the root logger for both the server and the GUI."""
    logging.basicConfig(
        level=getattr(logging, level.upper(), logging.INFO),
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
    )


def _load_env_file(filename: str = ".env") -> None:
    """Load a .env file into os.environ without overriding existing values.

    Standard library only — mirrors python-dotenv's basic behavior so the
    project stays dependency-free. Looks for the file in the repository root
    (the parent of the ``aif`` package). Lines like ``KEY=value`` are parsed;
    blank lines and ``#`` comments are ignored. Quoted values have the quotes
    stripped.
    """
    repo_root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    env_path = os.path.join(repo_root, filename)
    if not os.path.isfile(env_path):
        return

    with open(env_path, encoding="utf-8") as handle:
        for raw_line in handle:
            line = raw_line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            key, _, value = line.partition("=")
            key = key.strip()
            value = value.strip()
            if len(value) >= 2 and value[0] == value[-1] and value[0] in ("'", '"'):
                value = value[1:-1]
            if key and key not in os.environ:
                os.environ[key] = value


_load_env_file()
