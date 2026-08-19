"""HTTP API server for the cow weight estimator.

``EstimateHandler`` serves ``POST /estimate-weight``, ``GET /health``,
``GET /`` and ``OPTIONS``; ``create_server`` is the dependency-injection
entry point used by the tests and the CLI.

How to add a new endpoint:
    1. Add a ``do_<METHOD>`` handler to ``EstimateHandler`` that validates
       input, calls ``self.server.estimator`` (never a shared class
       attribute), sets ``self.request_id``, and sends errors through
       ``self._error(code, message, status)`` with a new machine-readable
       ``code`` string.
    2. Update the API reference in README.md and add an HTTP test in
       tests/.
"""

import json
import logging
import uuid
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler, HTTPServer
from typing import Any

from .config import VERSION
from .estimator import CowWeightEstimator, ImageValidationError

logger = logging.getLogger("aif")


class EstimateHandler(BaseHTTPRequestHandler):
    """Serve the HTTP API on top of an injected estimator.

    The estimator instance is read off ``self.server.estimator`` (injected
    by ``create_server``) rather than a class attribute, so each server can
    have its own backend/model. Endpoints:

    - ``POST /estimate-weight`` — estimate a cow's weight from an image.
    - ``GET /health`` — liveness probe with the active backend/model.
    - ``GET /`` and ``GET /info`` — service metadata.
    - ``OPTIONS`` — CORS preflight.

    Anything else returns ``404``. Every response carries a ``request_id``
    (also echoed as an ``x-request-id`` header) to correlate logs. Errors
    are JSON with a machine-readable ``code`` field plus a human ``error``
    message.

    Error codes: ``missing_body``, ``invalid_json``, ``missing_image``,
    ``invalid_image``, ``not_found``, ``estimation_failed``.
    """

    server_version = f"aif-cow-weight/{VERSION}"

    def _new_request_id(self) -> str:
        """Generate a short unique id for the current request."""
        return uuid.uuid4().hex[:8]

    def _cors_headers(self) -> None:
        """Attach permissive CORS headers so browsers can call the API."""
        self.send_header("Access-Control-Allow-Origin", "*")
        self.send_header("Access-Control-Allow-Methods", "POST, GET, OPTIONS")
        self.send_header("Access-Control-Allow-Headers", "Content-Type")

    def do_OPTIONS(self) -> None:
        """Answer CORS preflight requests with 204 and no body."""
        self.request_id = self._new_request_id()
        self.send_response(int(HTTPStatus.NO_CONTENT))
        self._cors_headers()
        self.send_header("Content-Length", "0")
        self.end_headers()

    def do_GET(self) -> None:
        """Serve ``GET /health`` (liveness + config) and ``GET /`` /
        ``GET /info`` (metadata). Anything else returns 404."""
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
        """Handle ``POST /estimate-weight``.

        Request body (JSON): ``image_url`` **or** ``image_base64`` (raw
        base64 or a ``data:`` URI), plus an optional ``prompt``. Responds
        200 with the estimate, 400 for bad input (including invalid image
        bytes), 502 if the estimator fails, 404 for unknown paths.
        """
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
        """Send a JSON error body with a machine-readable ``code`` and the
        request id. 5xx errors are logged at error level, everything else at
        warning level."""
        if status.value >= 500:
            logger.error("%s [%s]: %s", code, self.request_id, message)
        else:
            logger.warning("%s [%s]: %s", code, self.request_id, message)
        self._send_json(
            {"error": message, "code": code, "request_id": self.request_id},
            status=status,
        )

    def _send_json(self, payload: dict[str, Any], status: HTTPStatus) -> None:
        """Serialize ``payload`` as JSON and write the response with CORS
        headers and an ``x-request-id`` header. Injects the request id into
        the body when the payload doesn't already carry one."""
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
    """Create an HTTP server serving the estimation API.

    Args:
        host: Interface to bind. Defaults to loopback only.
        port: Port to listen on. Use ``0`` for an ephemeral port (the test
            suite relies on this).
        estimator: Estimator instance to use. When ``None``, a default
            ``CowWeightEstimator`` is built from env vars / ``.env`` at
            call time; changes to the environment after that have no effect.

    Returns:
        An ``HTTPServer`` with the estimator attached as
        ``server.estimator`` (read by ``EstimateHandler``). Call
        ``serve_forever()`` to start it.
    """
    server = HTTPServer((host, port), EstimateHandler)
    server.estimator = estimator or CowWeightEstimator()  # type: ignore[attr-defined]
    return server
