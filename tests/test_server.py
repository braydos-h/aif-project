"""HTTP test suite against the Rust backend binary.

The Python HTTP server was replaced by ``aif-backend`` (see ``backend/``).
These tests spawn the compiled binary on a free port, hit it over real
sockets, and assert the same API contract the old ``EstimateApiTests``
covered: status codes, JSON serialization, CORS headers, request IDs, error
codes, and the full request path.

The binary must be built first:
    cargo build --release --manifest-path backend/Cargo.toml
``AIF_BACKEND_BIN`` can override the binary path.
"""

import base64
import json
import os
import socket
import subprocess
import time
import unittest
import urllib.error
import urllib.request

from aif import DEFAULT_PROMPT

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def _png_bytes() -> bytes:
    """A minimal 1x1 transparent PNG — passes magic-byte validation."""
    return (
        b"\x89PNG\r\n\x1a\n"
        b"\x00\x00\x00\rIHDR\x00\x00\x00\x01\x00\x00\x00\x01\x08\x06\x00\x00\x00"
        b"\x1f\x15\xc4\x89"
        b"\x00\x00\x00\nIDATx\x9cc\x00\x01\x00\x00\x05\x00\x01\r\n-\xb4"
        b"\x00\x00\x00\x00IEND\xaeB`\x82"
    )


def _png_base64() -> str:
    return base64.b64encode(_png_bytes()).decode("ascii")


def _find_binary() -> str:
    """Locate the Rust backend binary, mirroring app.find_binary."""
    override = os.environ.get("AIF_BACKEND_BIN")
    if override:
        return override
    names = ("aif-backend.exe", "aif-backend")
    for name in names:
        candidate = os.path.join(REPO_ROOT, "backend", "target", "release", name)
        if os.path.isfile(candidate):
            return candidate
    raise FileNotFoundError(
        "Rust backend binary not found — run "
        "`cargo build --release --manifest-path backend/Cargo.toml` first."
    )


def _free_port() -> int:
    """Pick an ephemeral port by binding and releasing a socket."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


class RustBackendServer:
    """Spawn the backend binary on a free port, wait for health, kill on exit."""

    def __init__(self, env_extra: dict | None = None) -> None:
        self.port = _free_port()
        env = dict(os.environ)
        env.pop("AIF_AI_BACKEND", None)
        env.pop("OLLAMA_API_KEY", None)
        if env_extra:
            env.update(env_extra)
        self.process = subprocess.Popen(
            [_find_binary(), "--host", "127.0.0.1", "--port", str(self.port)],
            env=env,
            stderr=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
        )
        self.base_url = f"http://127.0.0.1:{self.port}"
        self._wait_until_ready()

    def _wait_until_ready(self, timeout: float = 15.0) -> None:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                raise RuntimeError(f"backend exited early with code {self.process.returncode}")
            try:
                with urllib.request.urlopen(f"{self.base_url}/health", timeout=1):
                    return
            except (urllib.error.URLError, OSError):
                time.sleep(0.05)
        raise RuntimeError("backend did not become ready in time")

    def close(self) -> None:
        if self.process.poll() is None:
            self.process.terminate()
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.kill()


class EstimateApiTests(unittest.TestCase):
    """Boot the real Rust backend on a free port and exercise it over HTTP."""

    @classmethod
    def setUpClass(cls) -> None:
        # Force the deterministic local fallback so the tests don't depend on
        # a running LLM.
        cls.server = RustBackendServer({"AIF_AI_BACKEND": "none"})

    @classmethod
    def tearDownClass(cls) -> None:
        cls.server.close()

    def post(self, payload):
        data = json.dumps(payload).encode("utf-8")
        request = urllib.request.Request(
            f"{self.server.base_url}/estimate-weight",
            data=data,
            method="POST",
            headers={"Content-Type": "application/json"},
        )
        with urllib.request.urlopen(request, timeout=10) as response:
            body = response.read().decode("utf-8")
            return response.status, json.loads(body), response.headers

    def get(self, path):
        request = urllib.request.Request(f"{self.server.base_url}{path}", method="GET")
        with urllib.request.urlopen(request, timeout=10) as response:
            body = response.read().decode("utf-8")
            return response.status, json.loads(body), response.headers

    def test_estimate_weight_with_image_url(self):
        status, body, _ = self.post(
            {"image_url": "https://example.com/cow.jpg", "prompt": "Estimate in kg"}
        )
        self.assertEqual(status, 200)
        self.assertIn("estimated_weight_kg", body)
        self.assertEqual(body["source"], "local_fallback")
        self.assertEqual(body["prompt_used"], "Estimate in kg")

    def test_estimate_weight_uses_default_prompt(self):
        status, body, _ = self.post({"image_base64": _png_base64()})
        self.assertEqual(status, 200)
        self.assertEqual(body["prompt_used"], DEFAULT_PROMPT)

    def test_response_includes_lbs(self):
        status, body, _ = self.post({"image_base64": _png_base64()})
        self.assertEqual(status, 200)
        self.assertIn("estimated_weight_lbs", body)
        self.assertAlmostEqual(
            body["estimated_weight_lbs"], body["estimated_weight_kg"] * 2.20462, places=1
        )

    def test_response_has_request_id_header_and_body(self):
        status, body, headers = self.post({"image_base64": _png_base64()})
        self.assertEqual(status, 200)
        self.assertIn("x-request-id", headers)
        rid = headers["x-request-id"]
        self.assertEqual(len(rid), 8)
        self.assertEqual(body["request_id"], rid)

    def test_error_response_has_request_id(self):
        data = json.dumps({"prompt": "Estimate in kg"}).encode("utf-8")
        request = urllib.request.Request(
            f"{self.server.base_url}/estimate-weight",
            data=data,
            method="POST",
            headers={"Content-Type": "application/json"},
        )
        with self.assertRaises(urllib.error.HTTPError) as context:
            urllib.request.urlopen(request, timeout=10)
        self.assertEqual(context.exception.code, 400)
        body = json.loads(context.exception.read().decode("utf-8"))
        self.assertEqual(context.exception.headers["x-request-id"], body["request_id"])

    def test_missing_image_returns_bad_request(self):
        data = json.dumps({"prompt": "Estimate in kg"}).encode("utf-8")
        request = urllib.request.Request(
            f"{self.server.base_url}/estimate-weight",
            data=data,
            method="POST",
            headers={"Content-Type": "application/json"},
        )
        with self.assertRaises(urllib.error.HTTPError) as context:
            urllib.request.urlopen(request, timeout=10)
        self.assertEqual(context.exception.code, 400)
        error = json.loads(context.exception.read().decode("utf-8"))
        self.assertIn("error", error)
        self.assertEqual(error["code"], "missing_image")

    def test_missing_body_returns_bad_request(self):
        request = urllib.request.Request(
            f"{self.server.base_url}/estimate-weight",
            data=b"",
            method="POST",
            headers={"Content-Type": "application/json"},
        )
        with self.assertRaises(urllib.error.HTTPError) as context:
            urllib.request.urlopen(request, timeout=10)
        self.assertEqual(context.exception.code, 400)
        error = json.loads(context.exception.read().decode("utf-8"))
        self.assertEqual(error["code"], "missing_body")

    def test_invalid_json_returns_bad_request(self):
        request = urllib.request.Request(
            f"{self.server.base_url}/estimate-weight",
            data=b"not json",
            method="POST",
            headers={"Content-Type": "application/json"},
        )
        with self.assertRaises(urllib.error.HTTPError) as context:
            urllib.request.urlopen(request, timeout=10)
        self.assertEqual(context.exception.code, 400)
        error = json.loads(context.exception.read().decode("utf-8"))
        self.assertEqual(error["code"], "invalid_json")

    def test_estimation_failure_returns_bad_gateway(self):
        # Force the ollama backend with no API key -> estimation error -> 502.
        server = RustBackendServer({"AIF_AI_BACKEND": "ollama"})
        try:
            data = json.dumps({"image_base64": _png_base64()}).encode("utf-8")
            request = urllib.request.Request(
                f"{server.base_url}/estimate-weight",
                data=data,
                method="POST",
                headers={"Content-Type": "application/json"},
            )
            with self.assertRaises(urllib.error.HTTPError) as context:
                urllib.request.urlopen(request, timeout=10)
            self.assertEqual(context.exception.code, 502)
            error = json.loads(context.exception.read().decode("utf-8"))
            self.assertEqual(error["code"], "estimation_failed")
        finally:
            server.close()

    def test_invalid_image_returns_bad_request(self):
        # Ollama backend WITH a key so we reach image validation, which
        # rejects the non-image bytes -> 400 invalid_image.
        server = RustBackendServer(
            {"AIF_AI_BACKEND": "ollama", "OLLAMA_API_KEY": "test-key"}
        )
        try:
            # "QUJD" decodes to "ABC" — not a valid image magic byte.
            data = json.dumps({"image_base64": "QUJD"}).encode("utf-8")
            request = urllib.request.Request(
                f"{server.base_url}/estimate-weight",
                data=data,
                method="POST",
                headers={"Content-Type": "application/json"},
            )
            with self.assertRaises(urllib.error.HTTPError) as context:
                urllib.request.urlopen(request, timeout=10)
            self.assertEqual(context.exception.code, 400)
            error = json.loads(context.exception.read().decode("utf-8"))
            self.assertEqual(error["code"], "invalid_image")
        finally:
            server.close()

    def test_health_endpoint(self):
        status, body, _ = self.get("/health")
        self.assertEqual(status, 200)
        self.assertEqual(body["status"], "ok")
        self.assertEqual(body["backend"], "none")
        self.assertIn("model", body)
        self.assertIn("request_id", body)

    def test_root_info_endpoint(self):
        status, body, _ = self.get("/")
        self.assertEqual(status, 200)
        self.assertEqual(body["name"], "Cow Weight Estimator")
        self.assertIn("version", body)
        self.assertIn("endpoints", body)
        self.assertIn("POST /estimate-weight", body["endpoints"])

    def test_unknown_get_returns_404(self):
        request = urllib.request.Request(f"{self.server.base_url}/nope", method="GET")
        with self.assertRaises(urllib.error.HTTPError) as context:
            urllib.request.urlopen(request, timeout=10)
        self.assertEqual(context.exception.code, 404)
        error = json.loads(context.exception.read().decode("utf-8"))
        self.assertEqual(error["code"], "not_found")

    def test_options_preflight_returns_204(self):
        request = urllib.request.Request(
            f"{self.server.base_url}/estimate-weight", method="OPTIONS"
        )
        with urllib.request.urlopen(request, timeout=10) as response:
            self.assertEqual(response.status, 204)
            self.assertEqual(response.headers["Access-Control-Allow-Origin"], "*")
            self.assertIn("POST", response.headers["Access-Control-Allow-Methods"])

    def test_cors_header_on_success(self):
        status, _, headers = self.post({"image_base64": _png_base64()})
        self.assertEqual(status, 200)
        self.assertEqual(headers["Access-Control-Allow-Origin"], "*")

    def test_concurrent_requests_all_succeed(self):
        # The Rust server handles connections on separate threads — fire
        # several requests at once and make sure none blocks the others.
        urls = [f"https://example.com/cow-{i}.jpg" for i in range(8)]
        import threading as threading_mod

        results = [None] * len(urls)
        errors = [None] * len(urls)

        def worker(index, url):
            try:
                status, body, _ = self.post({"image_url": url})
                results[index] = (status, body)
            except Exception as exc:  # noqa: BLE001
                errors[index] = exc

        threads = [
            threading_mod.Thread(target=worker, args=(i, url)) for i, url in enumerate(urls)
        ]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join(timeout=10)
        self.assertFalse(any(errors), errors)
        for status, body in results:
            self.assertEqual(status, 200)
            self.assertEqual(body["source"], "local_fallback")


if __name__ == "__main__":
    unittest.main()
