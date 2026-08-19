import os
import sys
import tempfile
import unittest
from unittest import mock

from aif.config import _env_candidates, repo_root


class RepoRootTests(unittest.TestCase):
    def test_returns_repo_root_in_normal_python(self):
        expected = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        self.assertEqual(repo_root(), expected)

    def test_returns_meipass_when_frozen(self):
        with mock.patch.object(sys, "frozen", True, create=True), mock.patch.object(
            sys, "_MEIPASS", "C:\\frozen\\meipass", create=True
        ):
            self.assertEqual(repo_root(), "C:\\frozen\\meipass")

    def test_ignores_meipass_when_not_frozen(self):
        with mock.patch.object(sys, "_MEIPASS", "C:\\frozen\\meipass", create=True):
            self.assertNotEqual(repo_root(), "C:\\frozen\\meipass")


class EnvCandidatesTests(unittest.TestCase):
    def test_looks_next_to_executable_first(self):
        with tempfile.TemporaryDirectory() as tmp:
            fake_exe = os.path.join(tmp, "CowWeightEstimator.exe")
            with mock.patch.object(sys, "executable", fake_exe):
                candidates = _env_candidates(".env")
        self.assertEqual(
            candidates,
            (os.path.join(tmp, ".env"), os.path.join(repo_root(), ".env")),
        )

    def test_reads_env_from_first_existing_candidate(self):
        with tempfile.TemporaryDirectory() as tmp:
            env_path = os.path.join(tmp, ".env")
            with open(env_path, "w", encoding="utf-8") as handle:
                handle.write("AIF_CONFIG_TEST_KEY=from-exe-dir\n")
            with mock.patch.object(sys, "executable", os.path.join(tmp, "app.exe")):
                with mock.patch("aif.config.repo_root", return_value=tmp):
                    from aif.config import _load_env_file

                    _load_env_file(".env")
        self.assertEqual(os.environ.get("AIF_CONFIG_TEST_KEY"), "from-exe-dir")
        os.environ.pop("AIF_CONFIG_TEST_KEY", None)


if __name__ == "__main__":
    unittest.main()
