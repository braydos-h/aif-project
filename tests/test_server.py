import base64
import json
import threading
import unittest
import urllib.error
import urllib.request
from unittest import mock

from aif import DEFAULT_PROMPT, CowWeightEstimator, create_server


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


class EstimateApiTests(unittest.TestCase):
    """Boot a real server on a background thread and exercise it over HTTP."""

    @classmethod
    def setUpClass(cls) -> None:
        # Ollama is the default backend, but the HTTP-level tests exercise the
        # deterministic local fallback so they don't depend on a running LLM.
        cls.server = create_server(port=0, estimator=CowWeightEstimator(backend="none"))
        cls.thread = threading.Thread(target=cls.server.serve_forever, daemon=True)
        cls.thread.start()
        cls.base_url = f"http://127.0.0.1:{cls.server.server_port}"

    @classmethod
    def tearDownClass(cls) -> None:
        cls.server.shutdown()
        cls.server.server_close()
        cls.thread.join(timeout=5)

    def post(self, payload):
        data = json.dumps(payload).encode("utf-8")
        request = urllib.request.Request(
            f"{self.base_url}/estimate-weight",
            data=data,
            method="POST",
            headers={"Content-Type": "application/json"},
        )
        with urllib.request.urlopen(request, timeout=10) as response:
            body = response.read().decode("utf-8")
            return response.status, json.loads(body), response.headers

    def get(self, path):
        request = urllib.request.Request(f"{self.base_url}{path}", method="GET")
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
            f"{self.base_url}/estimate-weight",
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
            f"{self.base_url}/estimate-weight",
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

    def test_estimation_failure_returns_bad_gateway(self):
        # Force the ollama backend with no API key -> ValueError -> 502.
        # Silence the expected exception logging so the test run stays clean.
        with mock.patch("aif.server.logger") as silent_logger:
            bad_server = create_server(
                port=0, estimator=CowWeightEstimator(backend="ollama")
            )
            thread = threading.Thread(target=bad_server.serve_forever, daemon=True)
            thread.start()
            base_url = f"http://127.0.0.1:{bad_server.server_port}"
            try:
                data = json.dumps({"image_base64": _png_base64()}).encode("utf-8")
                request = urllib.request.Request(
                    f"{base_url}/estimate-weight",
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
                bad_server.shutdown()
                bad_server.server_close()
                thread.join(timeout=5)
        self.assertTrue(silent_logger.exception.called)

    def test_invalid_image_returns_bad_request(self):
        # Force the ollama backend WITH an API key so we reach _to_base64_image,
        # which will reject the non-image bytes with ImageValidationError -> 400.
        with mock.patch.dict(
            "os.environ",
            {"OLLAMA_API_KEY": "test-key"},
            clear=False,
        ):
            bad_image_server = create_server(
                port=0, estimator=CowWeightEstimator(backend="ollama")
            )
            thread = threading.Thread(
                target=bad_image_server.serve_forever, daemon=True
            )
            thread.start()
            base_url = f"http://127.0.0.1:{bad_image_server.server_port}"
            try:
                # "QUJD" decodes to "ABC" — not a valid image magic byte.
                data = json.dumps({"image_base64": "QUJD"}).encode("utf-8")
                request = urllib.request.Request(
                    f"{base_url}/estimate-weight",
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
                bad_image_server.shutdown()
                bad_image_server.server_close()
                thread.join(timeout=5)

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
        request = urllib.request.Request(f"{self.base_url}/nope", method="GET")
        with self.assertRaises(urllib.error.HTTPError) as context:
            urllib.request.urlopen(request, timeout=10)
        self.assertEqual(context.exception.code, 404)
        error = json.loads(context.exception.read().decode("utf-8"))
        self.assertEqual(error["code"], "not_found")

    def test_options_preflight_returns_204(self):
        request = urllib.request.Request(
            f"{self.base_url}/estimate-weight", method="OPTIONS"
        )
        with urllib.request.urlopen(request, timeout=10) as response:
            self.assertEqual(response.status, 204)
            self.assertEqual(response.headers["Access-Control-Allow-Origin"], "*")
            self.assertIn("POST", response.headers["Access-Control-Allow-Methods"])

    def test_cors_header_on_success(self):
        status, _, headers = self.post({"image_base64": _png_base64()})
        self.assertEqual(status, 200)
        self.assertEqual(headers["Access-Control-Allow-Origin"], "*")


if __name__ == "__main__":
    unittest.main()
