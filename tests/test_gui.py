import unittest


class GuiSmokeTests(unittest.TestCase):
    """Build the Tk root and the main app, then destroy it. Catches import /
    layout regressions in aif/gui.py without requiring an interactive
    display."""

    def test_app_builds_without_errors(self):
        import tkinter as tk

        from aif.gui import CowWeightApp

        root = tk.Tk()
        try:
            app = CowWeightApp(root)  # noqa: F841
            root.update_idletasks()
        finally:
            root.destroy()

    def test_gui_entry_point_is_importable(self):
        import gui

        self.assertTrue(callable(gui.main))
        self.assertTrue(issubclass(gui.CowWeightApp, object))


if __name__ == "__main__":
    unittest.main()
