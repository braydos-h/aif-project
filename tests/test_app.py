import json
import threading
import unittest
import urllib.error
import urllib.request

from app import CowWeightEstimator, DEFAULT_PROMPT, EstimateHandler, create_server


class EstimateApiTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls) -> None:
        # Ollama is the default backend, but the HTTP-level tests exercise the
        # deterministic local fallback so they don't depend on a running LLM.
        EstimateHandler.estimator = CowWeightEstimator(backend="none")
        cls.server = create_server(port=0)
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
            return response.status, json.loads(body)

    def test_estimate_weight_with_image_url(self):
        status, body = self.post({"image_url": "https://example.com/cow.jpg", "prompt": "Estimate in kg"})
        self.assertEqual(status, 200)
        self.assertIn("estimated_weight_kg", body)
        self.assertEqual(body["source"], "local_fallback")
        self.assertEqual(body["prompt_used"], "Estimate in kg")

    def test_estimate_weight_uses_default_prompt(self):
        status, body = self.post({"image_base64": "ZmFrZS1pbWFnZS1ieXRlcw=="})
        self.assertEqual(status, 200)
        self.assertEqual(body["prompt_used"], DEFAULT_PROMPT)

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


class OllamaEstimatorTests(unittest.TestCase):
    def test_default_backend_is_ollama(self):
        estimator = CowWeightEstimator(backend=None, api_url=None)
        self.assertEqual(estimator.backend, "ollama")

    def test_custom_api_url_selects_custom_backend(self):
        estimator = CowWeightEstimator(api_url="https://example.com/ai")
        self.assertEqual(estimator.backend, "custom")

    def test_none_backend_uses_local_fallback(self):
        estimator = CowWeightEstimator(backend="none")
        result = estimator.estimate("https://example.com/cow.jpg")
        self.assertEqual(result["source"], "local_fallback")

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
        encoded = CowWeightEstimator._to_base64_image("data:image/jpeg;base64,QUJD")
        self.assertEqual(encoded, "QUJD")


if __name__ == "__main__":
    unittest.main()