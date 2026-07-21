import hashlib
import json
import os
import urllib.error
import urllib.request
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, HTTPServer
from typing import Any, Dict, Optional


DEFAULT_PROMPT = "Estimate this cow's weight in kilograms from the provided image."


class CowWeightEstimator:
    def __init__(self, api_url: Optional[str] = None, api_key: Optional[str] = None) -> None:
        self.api_url = api_url or os.environ.get("AIF_AI_API_URL")
        self.api_key = api_key or os.environ.get("AIF_AI_API_KEY")

    def estimate(self, image_reference: str, prompt: Optional[str] = None) -> Dict[str, Any]:
        prompt_to_use = prompt or DEFAULT_PROMPT
        if self.api_url:
            return self._estimate_via_ai_api(image_reference, prompt_to_use)
        return self._estimate_fallback(image_reference, prompt_to_use)

    def _estimate_via_ai_api(self, image_reference: str, prompt: str) -> Dict[str, Any]:
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
