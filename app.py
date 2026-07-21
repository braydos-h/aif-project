import base64
import hashlib
import json
import os
import re
import urllib.error
import urllib.request
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, HTTPServer
from typing import Any, Dict, Optional, Tuple


DEFAULT_PROMPT = "Estimate this cow's weight in kilograms from the provided image."

# Default backend is Ollama (local LLM runtime). Override via .env / env vars.
DEFAULT_OLLAMA_URL = "http://localhost:11434/api/generate"
DEFAULT_OLLAMA_MODEL = "llava"

# Backend choices: "ollama" (default), "custom" (generic AI API), "none" (local fallback).
BACKEND_OLLAMA = "ollama"
BACKEND_CUSTOM = "custom"
BACKEND_NONE = "none"


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
        api_url: Optional[str] = None,
        api_key: Optional[str] = None,
        model: Optional[str] = None,
        backend: Optional[str] = None,
    ) -> None:
        # A custom AI API URL takes precedence and selects the "custom" backend.
        self.api_url = api_url or os.environ.get("AIF_AI_API_URL")
        self.api_key = api_key or os.environ.get("AIF_AI_API_KEY")
        self.ollama_url = os.environ.get("AIF_OLLAMA_URL", DEFAULT_OLLAMA_URL)
        self.model = model or os.environ.get("AIF_AI_MODEL", DEFAULT_OLLAMA_MODEL)

        if backend:
            self.backend = backend
        elif self.api_url:
            self.backend = BACKEND_CUSTOM
        else:
            self.backend = os.environ.get("AIF_AI_BACKEND", BACKEND_OLLAMA)

    def estimate(self, image_reference: str, prompt: Optional[str] = None) -> Dict[str, Any]:
        prompt_to_use = prompt or DEFAULT_PROMPT
        if self.backend == BACKEND_NONE:
            return self._estimate_fallback(image_reference, prompt_to_use)
        if self.backend == BACKEND_CUSTOM:
            return self._estimate_via_custom_api(image_reference, prompt_to_use)
        return self._estimate_via_ollama(image_reference, prompt_to_use)

    def _estimate_via_ollama(self, image_reference: str, prompt: str) -> Dict[str, Any]:
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

        try:
            with urllib.request.urlopen(request, timeout=60) as response:
                body = response.read().decode("utf-8")
                parsed = json.loads(body) if body else {}
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
        }

    def _estimate_via_custom_api(self, image_reference: str, prompt: str) -> Dict[str, Any]:
        payload = json.dumps({"image": image_reference, "prompt": prompt}).encode("utf-8")
        request = urllib.request.Request(
            self.api_url,
            data=payload,
            method="POST",
            headers={"Content-Type": "application/json"},
        )
        if self.api_key:
            request.add_header("X-API-Key", self.api_key)

        try:
            with urllib.request.urlopen(request, timeout=20) as response:
                body = response.read().decode("utf-8")
                parsed = json.loads(body) if body else {}
        except (urllib.error.URLError, TimeoutError, json.JSONDecodeError) as exc:
            raise ValueError(f"Unable to get AI estimate: {exc}") from exc

        weight_kg = parsed.get("weight_kg") or parsed.get("estimate_kg") or parsed.get("estimated_weight_kg")
        if weight_kg is None:
            raise ValueError("AI API response missing weight estimate")

        return {"estimated_weight_kg": float(weight_kg), "source": "ai_api", "prompt_used": prompt}

    def _estimate_fallback(self, image_reference: str, prompt: str) -> Dict[str, Any]:
        digest = hashlib.sha256(image_reference.encode("utf-8")).hexdigest()
        normalized = int(digest[:8], 16) / 0xFFFFFFFF
        estimated_weight_kg = round(250 + (normalized * 650), 1)
        return {
            "estimated_weight_kg": estimated_weight_kg,
            "source": "local_fallback",
            "prompt_used": prompt,
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
        data_prefix_match = re.match(r"data:image/[^;]+;base64,", stripped)
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
    estimator = CowWeightEstimator()

    def do_POST(self) -> None:
        if self.path != "/estimate-weight":
            self._send_json({"error": "Not found"}, status=HTTPStatus.NOT_FOUND)
            return

        length_header = self.headers.get("Content-Length")
        if not length_header:
            self._send_json({"error": "Missing request body"}, status=HTTPStatus.BAD_REQUEST)
            return

        try:
            body = self.rfile.read(int(length_header))
            payload = json.loads(body.decode("utf-8"))
        except (ValueError, json.JSONDecodeError):
            self._send_json({"error": "Invalid JSON payload"}, status=HTTPStatus.BAD_REQUEST)
            return

        image_reference = payload.get("image_url") or payload.get("image_base64")
        prompt = payload.get("prompt")
        if not image_reference:
            self._send_json(
                {"error": "Provide image_url or image_base64 in request payload"},
                status=HTTPStatus.BAD_REQUEST,
            )
            return

        try:
            result = self.estimator.estimate(image_reference=image_reference, prompt=prompt)
        except ValueError as exc:
            self._send_json({"error": str(exc)}, status=HTTPStatus.BAD_GATEWAY)
            return

        self._send_json(result, status=HTTPStatus.OK)

    def log_message(self, format: str, *args: Any) -> None:
        return

    def _send_json(self, payload: Dict[str, Any], status: HTTPStatus) -> None:
        encoded = json.dumps(payload).encode("utf-8")
        self.send_response(int(status))
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        self.wfile.write(encoded)


def create_server(host: str = "127.0.0.1", port: int = 8080) -> HTTPServer:
    return HTTPServer((host, port), EstimateHandler)


if __name__ == "__main__":
    server = create_server()
    print("Cow weight estimation API listening on http://127.0.0.1:8080")
    server.serve_forever()