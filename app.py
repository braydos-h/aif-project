import base64
import hashlib
import json
import logging
import os
import re
import time
import urllib.error
import urllib.request
import uuid
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, HTTPServer
from typing import Any
from urllib.parse import urlparse

DEFAULT_PROMPT = (
    "Estimate this cow's weight in kilograms from the provided image. "
    'Reply with ONLY a JSON object of the form '
    '{"weight_kg": <number>, "confidence": <0..1>, '
    '"breed": <string>, "body_condition_score": <1..9>} '
    "where confidence is your confidence in the estimate (0..1), breed is your "
    "best guess of the breed (or \"unknown\"), and body_condition_score is a "
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


class ImageValidationError(ValueError):
    """Raised when the supplied image bytes are not a recognised image format."""


def setup_logging(level: str = "INFO") -> None:
    """Configure the root logger for both the server and the GUI."""
    logging.basicConfig(
        level=getattr(logging, level.upper(), logging.INFO),
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
    )


def _load_env_file(filename: str = ".env") -> None:
    """Load a .env file into os.environ without overriding existing values.

    Standard library only — mirrors python-dotenv's basic behavior so the
    project stays dependency-free. Lines like `KEY=value` are parsed; blank
    lines and `#` comments are ignored. Quoted values have the quotes stripped.
    """
    env_path = os.path.join(os.path.dirname(os.path.abspath(__file__)), filename)
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


def _kg_to_lbs(kg: float) -> float:
    return round(kg * KG_TO_LBS, 1)


def _validate_image_bytes(image_bytes: bytes) -> None:
    """Raise ImageValidationError if the bytes don't look like a supported image."""
    if not image_bytes:
        raise ImageValidationError("Image payload is empty")
    # WebP: RIFF....WEBP — check the WEBP tag at offset 8.
    if image_bytes.startswith(b"RIFF") and len(image_bytes) >= 12:
        if image_bytes[8:12] == b"WEBP":
            return
    for magic in IMAGE_MAGIC_BYTES:
        if image_bytes.startswith(magic):
            return
    raise ImageValidationError(
        "Image bytes do not match a supported format (JPEG, PNG, GIF, BMP, WebP)"
    )


class CowWeightEstimator:
    def __init__(
        self,
        model: str | None = None,
        backend: str | None = None,
        ollama_url: str | None = None,
        cache_ttl: int | None = None,
    ) -> None:
        self.ollama_url = ollama_url or os.environ.get("AIF_OLLAMA_URL", DEFAULT_OLLAMA_URL)
        self.ollama_api_key = os.environ.get("OLLAMA_API_KEY")
        self.model = model or os.environ.get("AIF_AI_MODEL", DEFAULT_OLLAMA_MODEL)
        self.backend = backend or os.environ.get("AIF_AI_BACKEND", "ollama")
        if cache_ttl is None:
            try:
                cache_ttl = int(os.environ.get("AIF_CACHE_TTL", str(DEFAULT_CACHE_TTL)))
            except ValueError:
                cache_ttl = DEFAULT_CACHE_TTL
        self.cache_ttl = max(0, cache_ttl)
        # key -> (expires_at, result_dict)
        self._cache: dict[str, tuple[float, dict[str, Any]]] = {}

    def estimate(self, image_reference: str, prompt: str | None = None) -> dict[str, Any]:
        prompt_to_use = prompt or DEFAULT_PROMPT
        if self.backend == "none":
            return self._estimate_fallback(image_reference, prompt_to_use)
        return self._estimate_via_ollama(image_reference, prompt_to_use)

    def _cache_get(self, key: str) -> dict[str, Any] | None:
        if self.cache_ttl <= 0:
            return None
        entry = self._cache.get(key)
        if not entry:
            return None
        expires_at, result = entry
        if time.monotonic() > expires_at:
            self._cache.pop(key, None)
            return None
        return dict(result)  # shallow copy so callers can't mutate the cache

    def _cache_put(self, key: str, result: dict[str, Any]) -> None:
        if self.cache_ttl <= 0:
            return
        self._cache[key] = (time.monotonic() + self.cache_ttl, result)

    def _estimate_via_ollama(self, image_reference: str, prompt: str) -> dict[str, Any]:
        if urlparse(self.ollama_url).hostname == "ollama.com" and not self.ollama_api_key:
            raise ValueError(
                "Ollama Cloud requires an API key. Set OLLAMA_API_KEY in .env "
                "to an API key created at https://ollama.com/settings/keys."
            )

        image_b64 = self._to_base64_image(image_reference)
        cache_key = hashlib.sha256(image_b64.encode("utf-8")).hexdigest()
        cached = self._cache_get(cache_key)
        if cached is not None:
            logger.info("cache hit for image %s", cache_key[:12])
            return cached

        payload = json.dumps(
            {
                "model": self.model,
                "prompt": prompt,
                "images": [image_b64],
                "stream": False,
            }
        ).encode("utf-8")
        request = urllib.request.Request(
            self.ollama_url,
            data=payload,
            method="POST",
            headers={"Content-Type": "application/json"},
        )
        if self.ollama_api_key:
            request.add_header("Authorization", f"Bearer {self.ollama_api_key}")

        text = self._call_ollama_with_retry(request)
        weight_kg, extras = self._parse_structured_response(text)
        if weight_kg is None:
            raise ValueError(f"Could not extract a weight from Ollama response: {text!r}")

        result: dict[str, Any] = {
            "estimated_weight_kg": weight_kg,
            "estimated_weight_lbs": _kg_to_lbs(weight_kg),
            "source": "ollama",
            "model": self.model,
            "prompt_used": prompt,
            "model_response": text,
        }
        result.update(extras)
        self._cache_put(cache_key, result)
        return result

    def _call_ollama_with_retry(self, request: urllib.request.Request) -> str:
        """Call the Ollama endpoint, retrying once on transient failures."""
        last_exc: Exception | None = None
        for attempt in range(OLLAMA_MAX_RETRIES + 1):
            try:
                with urllib.request.urlopen(request, timeout=60) as response:
                    body = response.read().decode("utf-8")
                    parsed = json.loads(body) if body else {}
                text = parsed.get("response")
                if not text and isinstance(parsed.get("message"), dict):
                    text = parsed["message"].get("content")
                if not text:
                    raise ValueError("Ollama response did not contain any text")
                return text
            except urllib.error.HTTPError as exc:
                error_body = exc.read().decode("utf-8", errors="replace")
                try:
                    error_detail = json.loads(error_body).get("error", error_body)
                except json.JSONDecodeError:
                    error_detail = error_body
                detail = error_detail.strip() if isinstance(error_detail, str) else str(error_detail)
                message = detail or exc.reason
                last_exc = ValueError(
                    f"Ollama request failed (HTTP {exc.code}): {message}"
                )
                # Only retry on 5xx (server-side / transient). 4xx is a real error.
                if exc.code < 500 or attempt == OLLAMA_MAX_RETRIES:
                    raise last_exc from exc
                logger.warning(
                    "Ollama returned HTTP %s, retrying in %.1fs (attempt %d/%d)",
                    exc.code,
                    OLLAMA_RETRY_BACKOFF,
                    attempt + 1,
                    OLLAMA_MAX_RETRIES,
                )
            except (urllib.error.URLError, TimeoutError) as exc:
                last_exc = ValueError(
                    f"Unable to reach Ollama at {self.ollama_url}: {exc}"
                )
                if attempt == OLLAMA_MAX_RETRIES:
                    raise last_exc from exc
                logger.warning(
                    "Ollama unreachable (%s), retrying in %.1fs (attempt %d/%d)",
                    exc,
                    OLLAMA_RETRY_BACKOFF,
                    attempt + 1,
                    OLLAMA_MAX_RETRIES,
                )
            except json.JSONDecodeError as exc:
                # Not transient — don't retry.
                raise ValueError(f"Ollama returned non-JSON body: {exc}") from exc

            time.sleep(OLLAMA_RETRY_BACKOFF)

        # Unreachable: every branch above either returns or raises.
        raise last_exc if last_exc else ValueError("Ollama call failed")

    @staticmethod
    def _parse_structured_response(text: str) -> tuple[float | None, dict[str, Any]]:
        """Pull a weight + extras out of the model's reply.

        Tries to find a JSON object with a ``weight_kg`` field first; if found,
        also extracts ``confidence``, ``breed`` and ``body_condition_score``
        when present. Falls back to ``_extract_weight_from_text`` (which prefers
        ``<n> kg`` then the first bare number) when no usable JSON is present.
        """
        # Find the first {...} block in the text.
        json_match = re.search(r"\{[^{}]*\}", text, re.DOTALL)
        if json_match:
            try:
                obj = json.loads(json_match.group(0))
            except json.JSONDecodeError:
                obj = None
            if isinstance(obj, dict):
                weight = obj.get("weight_kg")
                if weight is not None:
                    try:
                        weight_kg = float(weight)
                    except (TypeError, ValueError):
                        weight_kg = None
                    if weight_kg is not None:
                        extras: dict[str, Any] = {}
                        if "confidence" in obj:
                            try:
                                extras["confidence"] = float(obj["confidence"])
                            except (TypeError, ValueError):
                                pass
                        if "breed" in obj and isinstance(obj["breed"], str):
                            extras["breed"] = obj["breed"]
                        if "body_condition_score" in obj:
                            try:
                                extras["body_condition_score"] = float(
                                    obj["body_condition_score"]
                                )
                            except (TypeError, ValueError):
                                pass
                        return weight_kg, extras

        # No usable JSON — fall back to text extraction.
        return CowWeightEstimator._extract_weight_from_text(text), {}

    def _estimate_fallback(self, image_reference: str, prompt: str) -> dict[str, Any]:
        digest = hashlib.sha256(image_reference.encode("utf-8")).hexdigest()
        normalized = int(digest[:8], 16) / 0xFFFFFFFF
        estimated_weight_kg = round(250 + (normalized * 650), 1)
        return {
            "estimated_weight_kg": estimated_weight_kg,
            "estimated_weight_lbs": _kg_to_lbs(estimated_weight_kg),
            "source": "local_fallback",
            "prompt_used": prompt,
            "model_response": "",
        }

    @staticmethod
    def _to_base64_image(image_reference: str) -> str:
        """Return raw base64 image bytes from a URL or a base64/data-URI string.

        Validates that the decoded bytes look like a supported image format.
        Raises ImageValidationError if not.
        """
        if image_reference.startswith("http://") or image_reference.startswith("https://"):
            request = urllib.request.Request(
                image_reference, headers={"User-Agent": "aif-project/1.0"}
            )
            with urllib.request.urlopen(request, timeout=30) as response:
                image_bytes = response.read()
            _validate_image_bytes(image_bytes)
            return base64.b64encode(image_bytes).decode("ascii")

        stripped = image_reference
        # Accept any base64 data URI. Some Windows MIME databases do not know
        # image/webp and label WebP files as application/octet-stream.
        data_prefix_match = re.match(r"data:[^,]*;base64,", stripped, re.IGNORECASE)
        if data_prefix_match:
            stripped = stripped[data_prefix_match.end():]
        # Validate the decoded bytes.
        try:
            decoded = base64.b64decode(stripped, validate=True)
        except (ValueError, base64.binascii.Error) as exc:
            raise ImageValidationError("Image base64 payload is not valid base64") from exc
        _validate_image_bytes(decoded)
        return stripped

    @staticmethod
    def _extract_weight_from_text(text: str) -> float | None:
        """Pull a weight in kilograms out of free-form model output."""
        # Prefer an explicit "<number> kg" (case-insensitive).
        match = re.search(r"(\d+(?:\.\d+)?)\s*kg", text, re.IGNORECASE)
        if match:
            return float(match.group(1))
        # Fall back to the first bare number in the text.
        match = re.search(r"\d+(?:\.\d+)?", text)
        if match:
            return float(match.group(0))
        return None


class EstimateHandler(BaseHTTPRequestHandler):
    server_version = f"aif-cow-weight/{VERSION}"

    def _new_request_id(self) -> str:
        return uuid.uuid4().hex[:8]

    def _cors_headers(self) -> None:
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "POST, GET, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")

    def do_OPTIONS(self) -> None:
        self.request_id = self._new_request_id()
        self.send_response(int(HTTPStatus.NO_CONTENT))
        self._cors_headers()
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_GET(self) -> None:
        self.request_id = self._new_request_id()
        if self.path == "/health":
            estimator = self.server.estimator  # type: ignore[attr-defined]
            self._send_json(
                {
                    "status": "ok",
                    "backend": estimator.backend,
                    "model": estimator.model,
                    "request_id": self.request_id,
                },
                status=HTTPStatus.OK,
            )
            return
        if self.path == "/" or self.path == "/info":
            self._send_json(
                {
                    "name": "Cow Weight Estimator",
                    "version": VERSION,
                    "endpoints": [
                        "POST /estimate-weight",
                        "GET /health",
                        "GET /",
                    ],
                    "request_id": self.request_id,
                },
                status=HTTPStatus.OK,
            )
            return
        self._error("not_found", "Not found", HTTPStatus.NOT_FOUND)

    def do_POST(self) -> None:
        self.request_id = self._new_request_id()
        if self.path != "/estimate-weight":
            self._error("not_found", "Not found", HTTPStatus.NOT_FOUND)
            return

        length_header = self.headers.get("Content-Length")
        if not length_header:
            self._error("missing_body", "Missing request body", HTTPStatus.BAD_REQUEST)
            return

        try:
            body = self.rfile.read(int(length_header))
            payload = json.loads(body.decode("utf-8"))
        except (ValueError, json.JSONDecodeError):
            self._error("invalid_json", "Invalid JSON payload", HTTPStatus.BAD_REQUEST)
            return

        image_reference = payload.get("image_url") or payload.get("image_base64")
        prompt = payload.get("prompt")
        if not image_reference:
            self._error(
                "missing_image",
                "Provide image_url or image_base64 in request payload",
                HTTPStatus.BAD_REQUEST,
            )
            return

        estimator = self.server.estimator  # type: ignore[attr-defined]
        try:
            result = estimator.estimate(image_reference=image_reference, prompt=prompt)
        except ImageValidationError as exc:
            logger.warning("invalid_image [%s]: %s", self.request_id, exc)
            self._error("invalid_image", str(exc), HTTPStatus.BAD_REQUEST)
            return
        except ValueError as exc:
            logger.exception("estimation failed [%s]", self.request_id)
            self._error("estimation_failed", str(exc), HTTPStatus.BAD_GATEWAY)
            return

        self._send_json(result, status=HTTPStatus.OK)

    def log_message(self, format: str, *args: Any) -> None:
        rid = getattr(self, "request_id", "-")
        logger.info("%s [%s] - %s", self.address_string(), rid, format % args)

    def _error(self, code: str, message: str, status: HTTPStatus) -> None:
        """Send a JSON error body with a machine-readable ``code``.
        ``code`` is one of the handler's documented error codes; ``status``
        is the HTTP status to return. 5xx errors are logged at error level,
        everything else at warning."""
        if status.value >= 500:
            logger.error("%s [%s]: %s", code, self.request_id, message)
        else:
            logger.warning("%s [%s]: %s", code, self.request_id, message)
        self._send_json(
            {"error": message, "code": code, "request_id": self.request_id},
            status=status,
        )

    def _send_json(self, payload: dict[str, Any], status: HTTPStatus) -> None:
        if "request_id" not in payload:
            payload = {**payload, "request_id": getattr(self, "request_id", "-")}
        encoded = json.dumps(payload).encode("utf-8")
        self.send_response(int(status))
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.send_header("x-request-id", getattr(self, "request_id", "-"))
        self._cors_headers()
        self.end_headers()
        self.wfile.write(encoded)


def create_server(
    host: str = "127.0.0.1",
    port: int = 8080,
    estimator: CowWeightEstimator | None = None,
) -> HTTPServer:
    server = HTTPServer((host, port), EstimateHandler)
    server.estimator = estimator or CowWeightEstimator()  # type: ignore[attr-defined]
    return server


if __name__ == "__main__":
    setup_logging()
    server = create_server()
    print("Cow weight estimation API listening on http://127.0.0.1:8080")
    server.serve_forever()