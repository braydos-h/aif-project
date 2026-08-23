import unittest
from unittest import mock

import gui


class WebUiLauncherTests(unittest.TestCase):
    def test_legacy_gui_entry_point_uses_the_web_server_launcher(self):
        self.assertTrue(callable(gui.main))
        with mock.patch("app.find_binary", return_value="backend.exe"), mock.patch(
            "app.subprocess.run"
        ) as run:
            gui.main()
        run.assert_called_once_with(
            ["backend.exe", "--host", "127.0.0.1", "--port", "8080"],
            check=False,
        )


if __name__ == "__main__":
    unittest.main()
