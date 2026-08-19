import base64
import hashlib
import json
import logging
import os
import re
import urllib.error
import urllib.request
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, HTTPServer
from typing import Any, Dict, Optional
from urllib.parse import urlparse


DEFAULT_PROMPT = "Estimate this cow's weight in kilograms from the provided image."

# Default backend is Ollama Cloud. Override via .env / env vars.
DEFAULT_OLLAMA_URL = "https://ollama.com/api/generate"
DEFAULT_OLLAMA_MODEL = "gemma4:31b-cloud"

logger = logging.getLogger("aif")


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

    with open(env_path, "r", encoding="utf-8") as handle:
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


class CowWeightEstimator:
    def __init__(
        self,
        model: Optional[str] = None,
        backend: Optional[str] = None,
    ) -> None:
        self.ollama_url = os.environ.get("AIF_OLLAMA_URL", DEFAULT_OLLAMA_URL)
        self.ollama_api_key = os.environ.get("OLLAMA_API_KEY")
        self.model = model or os.environ.get("AIF_AI_MODEL", DEFAULT_OLLAMA_MODEL)
        self.backend = backend or os.environ.get("AIF_AI_BACKEND", "ollama")

    def estimate(self, image_reference: str, prompt: Optional[str] = None) -> Dict[str, Any]:
        prompt_to_use = prompt or DEFAULT_PROMPT
        if self.backend == "none":
            return self._estimate_fallback(image_reference, prompt_to_use)
        return self._estimate_via_ollama(image_reference, prompt_to_use)

    def _estimate_via_ollama(self, image_reference: str, prompt: str) -> Dict[str, Any]:
        if urlparse(self.ollama_url).hostname == "ollama.com" and not self.ollama_api_key:
            raise ValueError(
                "Ollama Cloud requires an API key. Set OLLAMA_API_KEY in .env "
                "to an API key created at https://ollama.com/settings/keys."
            )

        image_b64 = self._to_base64_image(image_reference)
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

        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                body = response.read().decode("utf-8")
                parsed = json.loads(body) if body else {}
        except urllib.error.HTTPError as exc:
            error_body = exc.read().decode("utf-8", errors="replace")
            try:
                error_detail = json.loads(error_body).get("error", error_body)
            except json.JSONDecodeError:
                error_detail = error_body
            detail = error_detail.strip() if isinstance(error_detail, str) else str(error_detail)
            message = detail or exc.reason
            raise ValueError(f"Ollama request failed (HTTP {exc.code}): {message}") from exc
        except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as exc:
            raise ValueError(f"Unable to reach Ollama at {self.ollama_url}: {exc}") from exc

        text = parsed.get("response")
        if not text and isinstance(parsed.get("message"), dict):
            text = parsed["message"].get("content")
        if not text:
            raise ValueError("Ollama response did not contain any text")

        weight_kg = self._extract_weight_from_text(text)
        if weight_kg is None:
            raise ValueError(f"Could not extract a weight from Ollama response: {text!r}")

        return {
            "estimated_weight_kg": weight_kg,
            "source": "ollama",
            "model": self.model,
            "prompt_used": prompt,
            "model_response": text,
        }

    def _estimate_fallback(self, image_reference: str, prompt: str) -> Dict[str, Any]:
        digest = hashlib.sha256(image_reference.encode("utf-8")).hexdigest()
        normalized = int(digest[:8], 16) / 0xFFFFFFFF
        estimated_weight_kg = round(250 + (normalized * 650), 1)
        return {
            "estimated_weight_kg": estimated_weight_kg,
            "source": "local_fallback",
            "prompt_used": prompt,
            "model_response": "",
        }

    @staticmethod
    def _to_base64_image(image_reference: str) -> str:
        """Return raw base64 image bytes from a URL or a base64/data-URI string."""
        if image_reference.startswith("http://") or image_reference.startswith("https://"):
            request = urllib.request.Request(image_reference, headers={"User-Agent": "aif-project/1.0"})
            with urllib.request.urlopen(request, timeout=30) as response:
                image_bytes = response.read()
            return base64.b64encode(image_bytes).decode("ascii")

        stripped = image_reference
        # Accept any base64 data URI. Some Windows MIME databases do not know
        # image/webp and label WebP files as application/octet-stream.
        data_prefix_match = re.match(r"data:[^,]*;base64,", stripped, re.IGNORECASE)
        if data_prefix_match:
            stripped = stripped[data_prefix_match.end():]
        return stripped

    @staticmethod
    def _extract_weight_from_text(text: str) -> Optional[float]:
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
    def do_POST(self) -> None:
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
        except ValueError as exc:
            logger.exception("estimation failed")
            self._error("estimation_failed", str(exc), HTTPStatus.BAD_GATEWAY)
            return

        self._send_json(result, status=HTTPStatus.OK)

    def log_message(self, format: str, *args: Any) -> None:
        logger.info("%s - %s", self.address_string(), format % args)

    def _error(self, code: str, message: str, status: HTTPStatus) -> None:
        if status.value >= 500:
            logger.error("%s: %s", code, message)
        else:
            logger.warning("%s: %s", code, message)
        self._send_json({"error": message, "code": code}, status=status)

    def _send_json(self, payload: Dict[str, Any], status: HTTPStatus) -> None:
        encoded = json.dumps(payload).encode("utf-8")
        self.send_response(int(status))
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)


def create_server(
    host: str = "127.0.0.1",
    port: int = 8080,
    estimator: Optional[CowWeightEstimator] = None,
) -> HTTPServer:
    server = HTTPServer((host, port), EstimateHandler)
    server.estimator = estimator or CowWeightEstimator()  # type: ignore[attr-defined]
    return server


if __name__ == "__main__":
    setup_logging()
    server = create_server()
    print("Cow weight estimation API listening on http://127.0.0.1:8080")
    server.serve_forever()