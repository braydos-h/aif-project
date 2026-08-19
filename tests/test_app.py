import base64
import io
import json
import threading
import time
import unittest
import urllib.error
import urllib.request
from unittest import mock

from app import (
    DEFAULT_PROMPT,
    CowWeightEstimator,
    ImageValidationError,
    create_server,
)


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


def _png_data_uri() -> str:
    return f"data:image/png;base64,{_png_base64()}"


class EstimateApiTests(unittest.TestCase):
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
        with mock.patch("app.logger") as silent_logger:
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


class OllamaEstimatorTests(unittest.TestCase):
    def test_default_backend_is_ollama(self):
        estimator = CowWeightEstimator(backend=None)
        self.assertEqual(estimator.backend, "ollama")

    def test_none_backend_uses_local_fallback(self):
        estimator = CowWeightEstimator(backend="none")
        result = estimator.estimate("https://example.com/cow.jpg")
        self.assertEqual(result["source"], "local_fallback")

    def test_fallback_includes_lbs(self):
        estimator = CowWeightEstimator(backend="none")
        result = estimator.estimate("https://example.com/cow.jpg")
        self.assertIn("estimated_weight_lbs", result)
        self.assertAlmostEqual(
            result["estimated_weight_lbs"],
            result["estimated_weight_kg"] * 2.20462,
            places=1,
        )

    def test_cloud_endpoint_uses_bearer_token(self):
        class FakeResponse:
            def read(self):
                return b'{"response": "612 kg"}'

            def __enter__(self):
                return self

            def __exit__(self, exc_type, exc_value, traceback):
                return False

        with mock.patch.dict(
            "os.environ",
            {
                "AIF_OLLAMA_URL": "https://ollama.com/api/generate",
                "AIF_AI_MODEL": "gemma4:31b",
                "OLLAMA_API_KEY": "test-key",
            },
            clear=False,
        ), mock.patch("urllib.request.urlopen", return_value=FakeResponse()) as urlopen:
            result = CowWeightEstimator(backend="ollama").estimate(_png_base64())

        self.assertEqual(result["estimated_weight_kg"], 612.0)
        self.assertEqual(urlopen.call_args.args[0].get_header("Authorization"), "Bearer test-key")
        sent_payload = json.loads(urlopen.call_args.args[0].data)
        self.assertEqual(sent_payload["model"], "gemma4:31b")

    def test_cloud_endpoint_requires_api_key(self):
        with mock.patch.dict(
            "os.environ",
            {
                "AIF_OLLAMA_URL": "https://ollama.com/api/generate",
                "OLLAMA_API_KEY": "",
            },
            clear=False,
        ):
            with self.assertRaisesRegex(ValueError, "OLLAMA_API_KEY"):
                CowWeightEstimator(backend="ollama").estimate(_png_base64())

    def test_extract_weight_prefers_explicit_kg(self):
        self.assertEqual(
            CowWeightEstimator._extract_weight_from_text("The cow weighs about 612 kg."),
            612.0,
        )

    def test_extract_weight_falls_back_to_bare_number(self):
        self.assertEqual(
            CowWeightEstimator._extract_weight_from_text("Estimated weight: 540"),
            540.0,
        )

    def test_extract_weight_returns_none_when_no_number(self):
        self.assertIsNone(CowWeightEstimator._extract_weight_from_text("no number here"))

    def test_to_base64_strips_data_uri_prefix(self):
        encoded = CowWeightEstimator._to_base64_image(_png_data_uri())
        self.assertEqual(encoded, _png_base64())

    def test_to_base64_strips_generic_mime_data_uri_prefix(self):
        encoded = CowWeightEstimator._to_base64_image(
            f"data:application/octet-stream;base64,{_png_base64()}"
        )
        self.assertEqual(encoded, _png_base64())

    def test_to_base64_rejects_non_image_bytes(self):
        with self.assertRaises(ImageValidationError):
            CowWeightEstimator._to_base64_image("QUJD")  # decodes to "ABC"

    def test_to_base64_rejects_empty_payload(self):
        with self.assertRaises(ImageValidationError):
            CowWeightEstimator._to_base64_image("")

    def test_to_base64_rejects_invalid_base64(self):
        with self.assertRaises(ImageValidationError):
            CowWeightEstimator._to_base64_image("!!!not-base64!!!")

    def test_to_base64_accepts_jpeg_magic_bytes(self):
        # JPEG SOI + a byte. Build a tiny fake JPEG payload.
        jpeg_b64 = base64.b64encode(b"\xff\xd8\xff\xe0rest-of-jpeg").decode("ascii")
        result = CowWeightEstimator._to_base64_image(jpeg_b64)
        self.assertEqual(result, jpeg_b64)

    def test_to_base64_accepts_webp_magic_bytes(self):
        webp = b"RIFF\x00\x00\x00\x00WEBPVP8 "
        webp_b64 = base64.b64encode(webp).decode("ascii")
        result = CowWeightEstimator._to_base64_image(webp_b64)
        self.assertEqual(result, webp_b64)


class StructuredResponseTests(unittest.TestCase):
    def test_parses_json_weight_with_extras(self):
        text = '{"weight_kg": 540, "confidence": 0.82, "breed": "Angus", "body_condition_score": 6}'
        weight, extras = CowWeightEstimator._parse_structured_response(text)
        self.assertEqual(weight, 540.0)
        self.assertEqual(extras["confidence"], 0.82)
        self.assertEqual(extras["breed"], "Angus")
        self.assertEqual(extras["body_condition_score"], 6.0)

    def test_parses_json_weight_with_partial_extras(self):
        text = '{"weight_kg": 500, "confidence": 0.7}'
        weight, extras = CowWeightEstimator._parse_structured_response(text)
        self.assertEqual(weight, 500.0)
        self.assertEqual(extras, {"confidence": 0.7})

    def test_falls_back_to_text_when_no_json(self):
        weight, extras = CowWeightEstimator._parse_structured_response("About 612 kg.")
        self.assertEqual(weight, 612.0)
        self.assertEqual(extras, {})

    def test_falls_back_to_text_when_json_has_no_weight(self):
        weight, extras = CowWeightEstimator._parse_structured_response('{"breed": "Angus"}')
        self.assertIsNone(weight)
        self.assertEqual(extras, {})

    def test_falls_back_when_json_weight_not_numeric(self):
        weight, extras = CowWeightEstimator._parse_structured_response(
            '{"weight_kg": "heavy"}'
        )
        self.assertIsNone(weight)
        self.assertEqual(extras, {})

    def test_array_json_falls_back_to_text_extraction(self):
        # A JSON array is not matched by the {...} regex, so the text-extraction
        # fallback runs and picks up the first bare number.
        weight, extras = CowWeightEstimator._parse_structured_response("[1, 2, 3]")
        self.assertEqual(weight, 1.0)
        self.assertEqual(extras, {})

    def test_extracts_json_embedded_in_prose(self):
        text = 'Here is my estimate: {"weight_kg": 480, "confidence": 0.65} thanks!'
        weight, extras = CowWeightEstimator._parse_structured_response(text)
        self.assertEqual(weight, 480.0)
        self.assertEqual(extras["confidence"], 0.65)

    def test_structured_response_end_to_end_via_ollama(self):
        inner = json.dumps(
            {"weight_kg": 700, "confidence": 0.9, "breed": "Hereford", "body_condition_score": 7}
        )
        outer = json.dumps({"response": inner})

        class FakeResponse:
            def read(self):
                return outer.encode("utf-8")

            def __enter__(self):
                return self

            def __exit__(self, exc_type, exc_value, traceback):
                return False

        with mock.patch.dict(
            "os.environ",
            {
                "AIF_OLLAMA_URL": "https://ollama.com/api/generate",
                "OLLAMA_API_KEY": "test-key",
            },
            clear=False,
        ), mock.patch("urllib.request.urlopen", return_value=FakeResponse()):
            result = CowWeightEstimator(backend="ollama").estimate(_png_base64())

        self.assertEqual(result["estimated_weight_kg"], 700.0)
        self.assertEqual(result["confidence"], 0.9)
        self.assertEqual(result["breed"], "Hereford")
        self.assertEqual(result["body_condition_score"], 7.0)
        self.assertEqual(result["source"], "ollama")


class CacheTests(unittest.TestCase):
    def test_cache_returns_same_result_for_repeat_image(self):
        call_count = 0

        class FakeResponse:
            def read(self):
                return b'{"response": "500 kg"}'

            def __enter__(self):
                return self

            def __exit__(self, exc_type, exc_value, traceback):
                return False

        def counting_urlopen(*args, **kwargs):
            nonlocal call_count
            call_count += 1
            return FakeResponse()

        with mock.patch.dict(
            "os.environ",
            {
                "AIF_OLLAMA_URL": "https://ollama.com/api/generate",
                "OLLAMA_API_KEY": "test-key",
            },
            clear=False,
        ), mock.patch("urllib.request.urlopen", side_effect=counting_urlopen):
            estimator = CowWeightEstimator(backend="ollama", cache_ttl=300)
            first = estimator.estimate(_png_base64())
            second = estimator.estimate(_png_base64())

        self.assertEqual(call_count, 1)  # second call served from cache
        self.assertEqual(first["estimated_weight_kg"], second["estimated_weight_kg"])
        self.assertEqual(second["estimated_weight_kg"], 500.0)

    def test_cache_disabled_when_ttl_zero(self):
        call_count = 0

        class FakeResponse:
            def read(self):
                return b'{"response": "500 kg"}'

            def __enter__(self):
                return self

            def __exit__(self, exc_type, exc_value, traceback):
                return False

        def counting_urlopen(*args, **kwargs):
            nonlocal call_count
            call_count += 1
            return FakeResponse()

        with mock.patch.dict(
            "os.environ",
            {
                "AIF_OLLAMA_URL": "https://ollama.com/api/generate",
                "OLLAMA_API_KEY": "test-key",
            },
            clear=False,
        ), mock.patch("urllib.request.urlopen", side_effect=counting_urlopen):
            estimator = CowWeightEstimator(backend="ollama", cache_ttl=0)
            estimator.estimate(_png_base64())
            estimator.estimate(_png_base64())

        self.assertEqual(call_count, 2)  # no caching, two network calls

    def test_cache_expires_after_ttl(self):
        class FakeResponse:
            def __init__(self, weight):
                self.weight = weight

            def read(self):
                return f'{{"response": "{self.weight} kg"}}'.encode()

            def __enter__(self):
                return self

            def __exit__(self, exc_type, exc_value, traceback):
                return False

        responses = [FakeResponse(500), FakeResponse(600)]

        def urlopen_side_effect(*args, **kwargs):
            return responses.pop(0)

        with mock.patch.dict(
            "os.environ",
            {
                "AIF_OLLAMA_URL": "https://ollama.com/api/generate",
                "OLLAMA_API_KEY": "test-key",
            },
            clear=False,
        ), mock.patch("urllib.request.urlopen", side_effect=urlopen_side_effect):
            estimator = CowWeightEstimator(backend="ollama", cache_ttl=100)
            first = estimator.estimate(_png_base64())
            # Expire the cache entry by fast-forwarding the monotonic clock.
            # The cache stores (expires_at, result); bumping time.monotonic past
            # it makes _cache_get treat the entry as stale.
            original_monotonic = time.monotonic
            try:
                time.monotonic = lambda: original_monotonic() + 200  # type: ignore[assignment]
                second = estimator.estimate(_png_base64())
            finally:
                time.monotonic = original_monotonic  # type: ignore[assignment]

        self.assertEqual(first["estimated_weight_kg"], 500.0)
        self.assertEqual(second["estimated_weight_kg"], 600.0)


class RetryTests(unittest.TestCase):
    def test_retries_once_on_url_error_then_succeeds(self):
        class FakeResponse:
            def read(self):
                return b'{"response": "550 kg"}'

            def __enter__(self):
                return self

            def __exit__(self, exc_type, exc_value, traceback):
                return False

        call_count = 0

        def flaky_urlopen(*args, **kwargs):
            nonlocal call_count
            call_count += 1
            if call_count == 1:
                raise urllib.error.URLError("transient")
            return FakeResponse()

        with mock.patch.dict(
            "os.environ",
            {
                "AIF_OLLAMA_URL": "https://ollama.com/api/generate",
                "OLLAMA_API_KEY": "test-key",
            },
            clear=False,
        ), mock.patch("urllib.request.urlopen", side_effect=flaky_urlopen), mock.patch(
            "time.sleep"
        ):
            result = CowWeightEstimator(backend="ollama").estimate(_png_base64())

        self.assertEqual(call_count, 2)
        self.assertEqual(result["estimated_weight_kg"], 550.0)

    def test_does_not_retry_on_4xx_http_error(self):
        call_count = 0

        def four_oh_four_urlopen(*args, **kwargs):
            nonlocal call_count
            call_count += 1
            raise urllib.error.HTTPError(
                args[0] if args else "url",
                404,
                "Not Found",
                {"Content-Type": "application/json"},
                io.BytesIO(b'{"error": "model not found"}'),
            )

        with mock.patch.dict(
            "os.environ",
            {
                "AIF_OLLAMA_URL": "https://ollama.com/api/generate",
                "OLLAMA_API_KEY": "test-key",
            },
            clear=False,
        ), mock.patch("urllib.request.urlopen", side_effect=four_oh_four_urlopen), mock.patch(
            "time.sleep"
        ) as sleep_mock:
            with self.assertRaises(ValueError) as ctx:
                CowWeightEstimator(backend="ollama").estimate(_png_base64())

        self.assertEqual(call_count, 1)  # no retry on 4xx
        self.assertIn("HTTP 404", str(ctx.exception))
        sleep_mock.assert_not_called()

    def test_retries_on_5xx_then_raises_after_exhausting(self):
        call_count = 0

        def five_hundred_urlopen(*args, **kwargs):
            nonlocal call_count
            call_count += 1
            raise urllib.error.HTTPError(
                args[0] if args else "url",
                503,
                "Service Unavailable",
                {"Content-Type": "application/json"},
                io.BytesIO(b'{"error": "overloaded"}'),
            )

        with mock.patch.dict(
            "os.environ",
            {
                "AIF_OLLAMA_URL": "https://ollama.com/api/generate",
                "OLLAMA_API_KEY": "test-key",
            },
            clear=False,
        ), mock.patch("urllib.request.urlopen", side_effect=five_hundred_urlopen), mock.patch(
            "time.sleep"
        ):
            with self.assertRaises(ValueError) as ctx:
                CowWeightEstimator(backend="ollama").estimate(_png_base64())

        # 1 initial + 1 retry = 2 calls.
        self.assertEqual(call_count, 2)
        self.assertIn("HTTP 503", str(ctx.exception))


class GuiSmokeTests(unittest.TestCase):
    """Build the Tk root and the main app, then destroy it. Catches import /
    layout regressions in gui.py without requiring an interactive display."""

    def test_app_builds_without_errors(self):
        import tkinter as tk

        from gui import CowWeightApp

        root = tk.Tk()
        try:
            app = CowWeightApp(root)  # noqa: F841
            root.update_idletasks()
        finally:
            root.destroy()


if __name__ == "__main__":
    unittest.main()
