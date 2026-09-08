"""Static checks for the super-simple WebUI.

The page must stay dependency-free and simple: one file picker (batch),
one estimate button, one status line, one result area, one session history.
Dynamic values must use textContent/DOM APIs (never innerHTML) and no
secrets or images may be persisted in browser storage.
"""

import os
import unittest

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
WEB_DIR = os.path.join(REPO_ROOT, "web")


def _read(name: str) -> str:
    with open(os.path.join(WEB_DIR, name), encoding="utf-8") as handle:
        return handle.read()


class SimpleWebUiTests(unittest.TestCase):
    def test_index_has_batch_upload_result_and_history(self):
        html = _read("index.html")
        self.assertIn("Cow Weight Estimator", html)
        self.assertIn('id="image-input"', html)
        self.assertIn("multiple", html)
        self.assertIn('id="estimate-button"', html)
        self.assertIn('id="status"', html)
        self.assertIn('id="result-area"', html)
        self.assertIn('id="history-list"', html)

    def test_index_stays_simple_without_settings_clutter(self):
        html = _read("index.html")
        for gone in (
            "advanced-settings",
            "api-key-input",
            "prompt-input",
            "backend-select",
            "model-input",
            "ollama-url-input",
        ):
            self.assertNotIn(gone, html)

    def test_index_has_demo_picker_capture_and_disclaimer(self):
        html = _read("index.html")
        self.assertIn('id="demo-select"', html)
        self.assertIn('id="demo-button"', html)
        self.assertIn('capture="environment"', html)
        self.assertIn("not a scale or vet advice", html)
        self.assertIn("local_fallback", html)

    def test_js_demo_picker_uses_image_url_and_matches_20mb_limit(self):
        js = _read("app.js")
        self.assertIn("demo-select", js)
        self.assertIn("/demo-cows", js)
        self.assertIn("image_url", js)
        self.assertIn("20 * 1024 * 1024", js)
        self.assertNotIn("15 * 1024 * 1024", js)

    def test_css_has_touch_targets_and_print_rules(self):
        css = _read("styles.css")
        self.assertIn("min-height: 44px", css)
        self.assertIn("@media print", css)

    def test_js_posts_estimates_and_renders_safely(self):
        js = _read("app.js")
        self.assertIn("estimate-weight", js)
        self.assertIn("history-list", js)
        self.assertIn("textContent", js)
        self.assertNotIn("innerHTML", js)
        self.assertNotIn("localStorage", js)
        self.assertNotIn("sessionStorage", js)

    def test_css_stays_small_and_supports_dark_mode(self):
        css = _read("styles.css")
        self.assertIn("prefers-color-scheme", css)
        # Super-simple stylesheet: hard cap keeps ornament from creeping back.
        self.assertLessEqual(len([line for line in css.splitlines() if line.strip()]), 200)


if __name__ == "__main__":
    unittest.main()
