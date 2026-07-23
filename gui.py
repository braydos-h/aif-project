"""Windows desktop interface for estimating a cow's weight from an image."""

import base64
import mimetypes
import os
import threading
import tkinter as tk
from tkinter import filedialog, messagebox, ttk
from typing import Optional

from app import CowWeightEstimator, DEFAULT_PROMPT


WINDOW_TITLE = "Cow Weight Estimator"


def image_file_to_data_uri(filename: str) -> str:
    """Read an image file and return a data URI accepted by the estimator."""
    with open(filename, "rb") as image_file:
        encoded = base64.b64encode(image_file.read()).decode("ascii")
    mime_type = mimetypes.guess_type(filename)[0] or "application/octet-stream"
    return f"data:{mime_type};base64,{encoded}"


class CowWeightApp:
    def __init__(self, root: tk.Tk) -> None:
        self.root = root
        self.root.title(WINDOW_TITLE)
        self.root.minsize(560, 310)
        self.image_path = tk.StringVar()
        self.status_text = tk.StringVar(value="Choose a cow image to begin.")
        self.result_text = tk.StringVar(value="")

        frame = ttk.Frame(root, padding=20)
        frame.grid(sticky="nsew")
        root.columnconfigure(0, weight=1)
        root.rowconfigure(0, weight=1)
        frame.columnconfigure(0, weight=1)

        ttk.Label(frame, text="Cow Weight Estimator", font=("Segoe UI", 16, "bold")).grid(
            row=0, column=0, columnspan=2, sticky="w"
        )
        ttk.Label(frame, text="Image file").grid(row=1, column=0, sticky="w", pady=(18, 4))
        ttk.Entry(frame, textvariable=self.image_path, state="readonly").grid(
            row=2, column=0, sticky="ew", padx=(0, 8)
        )
        ttk.Button(frame, text="Browse…", command=self.choose_image).grid(row=2, column=1)

        ttk.Label(frame, text="Prompt (optional)").grid(row=3, column=0, sticky="w", pady=(14, 4))
        self.prompt = tk.Text(frame, height=4, wrap="word")
        self.prompt.insert("1.0", DEFAULT_PROMPT)
        self.prompt.grid(row=4, column=0, columnspan=2, sticky="ew")

        self.estimate_button = ttk.Button(frame, text="Estimate weight", command=self.estimate)
        self.estimate_button.grid(row=5, column=0, sticky="w", pady=(16, 8))
        ttk.Label(frame, textvariable=self.status_text).grid(row=6, column=0, columnspan=2, sticky="w")
        ttk.Label(frame, textvariable=self.result_text, font=("Segoe UI", 14, "bold")).grid(
            row=7, column=0, columnspan=2, sticky="w", pady=(8, 0)
        )

    def choose_image(self) -> None:
        filename = filedialog.askopenfilename(
            title="Choose a cow image",
            filetypes=[
                ("Image files", "*.jpg *.jpeg *.png *.webp *.bmp *.gif"),
                ("All files", "*.*"),
            ],
        )
        if filename:
            self.image_path.set(filename)
            self.status_text.set("Ready to estimate.")
            self.result_text.set("")

    def estimate(self) -> None:
        filename = self.image_path.get()
        if not filename or not os.path.isfile(filename):
            messagebox.showerror(WINDOW_TITLE, "Please choose an image file first.")
            return

        prompt = self.prompt.get("1.0", "end").strip() or DEFAULT_PROMPT
        self.estimate_button.configure(state="disabled")
        self.status_text.set("Estimating weight…")
        self.result_text.set("")
        threading.Thread(target=self._estimate_in_background, args=(filename, prompt), daemon=True).start()

    def _estimate_in_background(self, filename: str, prompt: str) -> None:
        try:
            result = CowWeightEstimator().estimate(image_file_to_data_uri(filename), prompt)
        except (OSError, ValueError) as exc:
            self.root.after(0, self._show_error, str(exc))
            return
        self.root.after(0, self._show_result, result["estimated_weight_kg"], result["source"])

    def _show_error(self, error: str) -> None:
        self.status_text.set("Could not estimate weight.")
        self.estimate_button.configure(state="normal")
        messagebox.showerror(WINDOW_TITLE, error)

    def _show_result(self, weight_kg: float, source: str) -> None:
        self.status_text.set(f"Estimate completed using {source}.")
        self.result_text.set(f"Estimated weight: {weight_kg:g} kg")
        self.estimate_button.configure(state="normal")


def main() -> None:
    root = tk.Tk()
    CowWeightApp(root)
    root.mainloop()


if __name__ == "__main__":
    main()
