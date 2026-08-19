"""Windows desktop interface for estimating a cow's weight from an image.

A Tkinter app (``CowWeightApp``) that wraps ``app.CowWeightEstimator``
directly — it does **not** start the HTTP server. The user picks an image
file, edits the prompt, switches backend/model/URL at runtime, and views
the weight, the model's full reply, and a session history.

Threading model
---------------
All work that can block (network calls, image encoding) runs on daemon
background threads. UI updates are marshalled back to the Tk event loop
via ``root.after(0, ...)`` — never touch widgets from a background thread.

Extending the UI
----------------
- Widgets are built in ``_build_layout`` using a grid on a single padded
  frame; new rows extend the row numbers (the history treeview is the
  row with weight 1). Button commands are methods of ``CowWeightApp``.
- Follow the "…_in_background" + "…_result" pattern for anything that can
  block: disable buttons, start the progress bar, spawn a daemon thread,
  and update widgets from the thread via ``root.after``.
- Keep Tk and backend logic separate: reuse ``CowWeightEstimator`` from
  ``app.py`` instead of inlining estimation code here.
"""

import base64
import logging
import mimetypes
import os
import threading
import time
import tkinter as tk
import webbrowser
from tkinter import filedialog, messagebox, ttk

from app import (
    DEFAULT_OLLAMA_MODEL,
    DEFAULT_OLLAMA_URL,
    DEFAULT_PROMPT,
    CowWeightEstimator,
    setup_logging,
)

try:
    from PIL import Image, ImageTk  # type: ignore[import-not-found]

    PIL_AVAILABLE = True
except ImportError:
    PIL_AVAILABLE = False


WINDOW_TITLE = "Cow Weight Estimator"
PROJECT_URL = "https://github.com/braydos-h/aif-project"
PREVIEW_SIZE = (160, 160)
HISTORY_COLUMNS = ("time", "image", "weight", "source")
HISTORY_MAX_ROWS = 20
BACKEND_CHOICES = ("ollama", "none")
DEMO_COW_DIR = os.path.join(os.path.dirname(os.path.abspath(__file__)), "cows")
DEMO_IMAGE_EXTS = (".jpg", ".jpeg", ".png", ".webp", ".bmp", ".gif")


logger = logging.getLogger("aif.gui")


def image_file_to_data_uri(filename: str) -> str:
    """Read an image file and return a data URI accepted by the estimator."""
    with open(filename, "rb") as image_file:
        encoded = base64.b64encode(image_file.read()).decode("ascii")
    mime_type = mimetypes.guess_type(filename)[0] or "application/octet-stream"
    return f"data:{mime_type};base64,{encoded}"


def _short_name(path: str, width: int = 32) -> str:
    name = os.path.basename(path)
    return name if len(name) <= width else name[: width - 1] + "…"


class CowWeightApp:
    """Tkinter desktop app for estimating cow weight from a local image.

    Args:
        root: The Tk root window this app builds its widgets in.

    Note:
        Everything on the class is a widget-building or UI-update concern;
        estimation itself is delegated to ``CowWeightEstimator`` (imported
        from ``app.py``). A fresh estimator is built per request from the
        UI-selected backend/model/URL.
    """

    def __init__(self, root: tk.Tk) -> None:
        self.root = root
        self.root.title(WINDOW_TITLE)
        self.root.minsize(720, 560)

        self.image_path = tk.StringVar()
        self.status_text = tk.StringVar(value="Choose a cow image to begin.")
        self.result_text = tk.StringVar(value="")
        self.backend_var = tk.StringVar(value=CowWeightEstimator().backend)
        self.model_var = tk.StringVar(value=DEFAULT_OLLAMA_MODEL)
        self.url_var = tk.StringVar(value=DEFAULT_OLLAMA_URL)
        self.preview_image: ImageTk.PhotoImage | None = None  # keep ref
        self.last_request: tuple | None = None

        self._build_layout()
        self._bind_shortcuts()

    def _build_layout(self) -> None:
        """Create every widget and lay it out on a grid.

        Rows 0–14: title, image picker, preview, backend/model/URL
        selectors, prompt, buttons, status, result, model reply, history,
        footer. The history treeview is the only row that stretches.
        """
        frame = ttk.Frame(self.root, padding=20)
        frame.grid(sticky="nsew")
        self.root.columnconfigure(0, weight=1)
        self.root.rowconfigure(0, weight=1)
        frame.columnconfigure(0, weight=1)
        frame.columnconfigure(1, weight=1)

        ttk.Label(frame, text="Cow Weight Estimator", font=("Segoe UI", 16, "bold")).grid(
            row=0, column=0, columnspan=3, sticky="w"
        )

        # --- Image picker + preview ---
        ttk.Label(frame, text="Image file").grid(row=1, column=0, sticky="w", pady=(18, 4))
        ttk.Entry(frame, textvariable=self.image_path, state="readonly").grid(
            row=2, column=0, columnspan=2, sticky="ew", padx=(0, 8)
        )
        ttk.Button(frame, text="Browse…", command=self.choose_image).grid(row=2, column=2)

        self.preview_label = ttk.Label(frame, text="(no image)", anchor="center", relief="sunken")
        self.preview_label.grid(
            row=3, column=0, columnspan=3, sticky="w", pady=(8, 0), ipadx=4, ipady=4
        )
        self.preview_label.configure(width=24)

        # --- Backend / model / URL selectors ---
        selector_frame = ttk.Frame(frame)
        selector_frame.grid(row=4, column=0, columnspan=3, sticky="ew", pady=(14, 0))
        selector_frame.columnconfigure(1, weight=1)
        selector_frame.columnconfigure(4, weight=1)

        ttk.Label(selector_frame, text="Backend").grid(row=0, column=0, sticky="w", padx=(0, 8))
        self.backend_combo = ttk.Combobox(
            selector_frame,
            textvariable=self.backend_var,
            values=BACKEND_CHOICES,
            state="readonly",
            width=10,
        )
        self.backend_combo.grid(row=0, column=1, sticky="w")

        ttk.Label(selector_frame, text="Model").grid(row=0, column=2, sticky="w", padx=(16, 8))
        self.model_entry = ttk.Entry(selector_frame, textvariable=self.model_var, width=24)
        self.model_entry.grid(row=0, column=3, sticky="w")

        url_label = ttk.Label(selector_frame, text="Ollama URL")
        url_label.grid(row=1, column=0, sticky="w", padx=(0, 8), pady=(8, 0))
        self.url_entry = ttk.Entry(selector_frame, textvariable=self.url_var)
        self.url_entry.grid(row=1, column=1, columnspan=3, sticky="ew", pady=(8, 0))

        # --- Prompt ---
        ttk.Label(frame, text="Prompt (optional)").grid(row=5, column=0, sticky="w", pady=(14, 4))
        self.prompt = tk.Text(frame, height=4, wrap="word")
        self.prompt.insert("1.0", DEFAULT_PROMPT)
        self.prompt.grid(row=6, column=0, columnspan=3, sticky="ew")

        # --- Buttons + progress ---
        button_frame = ttk.Frame(frame)
        button_frame.grid(row=7, column=0, columnspan=3, sticky="w", pady=(16, 8))
        self.estimate_button = ttk.Button(
            button_frame, text="Estimate weight", command=self.estimate
        )
        self.estimate_button.pack(side="left")
        self.demo_button = ttk.Button(
            button_frame, text="Test demo cows", command=self.estimate_demo_cows
        )
        self.demo_button.pack(side="left", padx=(8, 0))
        self.copy_button = ttk.Button(
            button_frame, text="Copy result", command=self.copy_result, state="disabled"
        )
        self.copy_button.pack(side="left", padx=(8, 0))
        self.retry_button = ttk.Button(
            button_frame, text="Retry", command=self.retry, state="disabled"
        )
        self.retry_button.pack(side="left", padx=(8, 0))

        self.progress = ttk.Progressbar(button_frame, mode="indeterminate", length=160)
        self.progress.pack(side="left", padx=(16, 0))

        ttk.Label(frame, textvariable=self.status_text).grid(
            row=8, column=0, columnspan=3, sticky="w"
        )
        ttk.Label(frame, textvariable=self.result_text, font=("Segoe UI", 14, "bold")).grid(
            row=9, column=0, columnspan=3, sticky="w", pady=(8, 0)
        )

        # --- Model reply (read-only) ---
        ttk.Label(frame, text="Model reply").grid(row=10, column=0, sticky="w", pady=(10, 4))
        self.reply_text = tk.Text(frame, height=4, wrap="word", state="disabled")
        self.reply_text.grid(row=11, column=0, columnspan=3, sticky="ew")

        # --- History ---
        ttk.Label(frame, text="History (this session)").grid(
            row=12, column=0, sticky="w", pady=(14, 4)
        )
        self.history = ttk.Treeview(
            frame, columns=HISTORY_COLUMNS, show="headings", height=6
        )
        for col, label, width in [
            ("time", "Time", 90),
            ("image", "Image", 260),
            ("weight", "Weight (kg)", 100),
            ("source", "Source", 120),
        ]:
            self.history.heading(col, text=label)
            self.history.column(col, width=width, stretch=(col == "image"))
        self.history.grid(row=13, column=0, columnspan=3, sticky="nsew")
        frame.rowconfigure(13, weight=1)

        footer = ttk.Label(
            frame,
            text="Made by Brayden",
            foreground="#0000EE",
            cursor="hand2",
        )
        footer.grid(row=14, column=0, columnspan=3, sticky="w", pady=(12, 0))
        footer.bind("<Button-1>", lambda _event: webbrowser.open(PROJECT_URL))

    def _bind_shortcuts(self) -> None:
        """Wire keyboard shortcuts: Enter estimates from anywhere; in the
        prompt box Enter keeps a newline and Ctrl+Enter estimates."""
        self.root.bind("<Return>", lambda _event: self.estimate())
        self.root.bind("<Control-Return>", lambda _event: self.estimate())
        # Let the prompt Text widget keep newlines on Enter; Ctrl+Enter estimates.
        self.prompt.bind("<Return>", lambda event: ("break",))
        self.prompt.bind("<Control-Return>", lambda _event: self.estimate())

    def choose_image(self) -> None:
        """Open a file picker and, on selection, update the path field,
        clear the result, and load the preview."""
        filename = filedialog.askopenfilename(
            title="Choose a cow image",
            filetypes=[
                ("Image files", "*.jpg *.jpeg *.png *.webp *.bmp *.gif"),
                ("All files", "*.*"),
            ],
        )
        if not filename:
            return
        self.image_path.set(filename)
        self.status_text.set("Ready to estimate.")
        self.result_text.set("")
        self._set_reply("")
        self._load_preview(filename)

    def _load_preview(self, filename: str) -> None:
        """Show a thumbnail of ``filename`` via Pillow.

        When Pillow is unavailable (or the file can't be decoded), degrades
        gracefully to showing the filename and byte size (or
        "(preview unavailable)").
        """
        if not PIL_AVAILABLE:
            size = os.path.getsize(filename)
            self.preview_label.configure(
                text=f"{_short_name(filename)}\n{size} bytes (preview needs Pillow)",
                image="",
            )
            return
        try:
            with Image.open(filename) as img:
                img = img.convert("RGB")
                img.thumbnail(PREVIEW_SIZE)
                self.preview_image = ImageTk.PhotoImage(img)
            self.preview_label.configure(image=self.preview_image, text="")
        except OSError as exc:
            logger.warning("preview failed for %s: %s", filename, exc)
            self.preview_label.configure(image="", text="(preview unavailable)")

    def estimate(self) -> None:
        """Validate the selected file, snapshot the current settings, and
        kick off an estimate on a background thread.

        The last request is remembered in ``self.last_request`` so Retry can
        re-run it (a ``"demo"`` marker means the demo-cow run).
        """
        filename = self.image_path.get()
        if not filename or not os.path.isfile(filename):
            messagebox.showerror(WINDOW_TITLE, "Please choose an image file first.")
            return

        prompt = self.prompt.get("1.0", "end").strip() or DEFAULT_PROMPT
        backend = self.backend_var.get()
        model = self.model_var.get().strip() or DEFAULT_OLLAMA_MODEL
        ollama_url = self.url_var.get().strip() or DEFAULT_OLLAMA_URL
        self.estimate_button.configure(state="disabled")
        self.copy_button.configure(state="disabled")
        self.retry_button.configure(state="disabled")
        self.status_text.set("Estimating weight…")
        self.result_text.set("")
        self._set_reply("")
        self.progress.start(10)
        start_time = time.monotonic()
        self.last_request = (filename, prompt, backend, model, ollama_url)
        threading.Thread(
            target=self._estimate_in_background,
            args=(filename, prompt, backend, model, ollama_url, start_time),
            daemon=True,
        ).start()

    def estimate_demo_cows(self) -> None:
        """Run the estimator over every image in the ``cows/`` folder.

        Files are picked up by extension (``DEMO_IMAGE_EXTS``), sorted by
        name, and estimated one by one on a background thread with per-file
        status updates and history rows.
        """
        if not os.path.isdir(DEMO_COW_DIR):
            messagebox.showerror(WINDOW_TITLE, f"Demo cow folder not found: {DEMO_COW_DIR}")
            return
        demo_files = sorted(
            os.path.join(DEMO_COW_DIR, name)
            for name in os.listdir(DEMO_COW_DIR)
            if name.lower().endswith(DEMO_IMAGE_EXTS)
        )
        if not demo_files:
            messagebox.showerror(WINDOW_TITLE, f"No cow images found in {DEMO_COW_DIR}")
            return

        prompt = self.prompt.get("1.0", "end").strip() or DEFAULT_PROMPT
        backend = self.backend_var.get()
        model = self.model_var.get().strip() or DEFAULT_OLLAMA_MODEL
        ollama_url = self.url_var.get().strip() or DEFAULT_OLLAMA_URL
        self.last_request = ("demo", prompt, backend, model, ollama_url)
        self.estimate_button.configure(state="disabled")
        self.demo_button.configure(state="disabled")
        self.copy_button.configure(state="disabled")
        self.retry_button.configure(state="disabled")
        self.result_text.set("")
        self._set_reply("")
        self.progress.start(10)
        threading.Thread(
            target=self._estimate_demo_cows_in_background,
            args=(demo_files, prompt, backend, model, ollama_url),
            daemon=True,
        ).start()

    def _estimate_demo_cows_in_background(
        self, demo_files: list[str], prompt: str, backend: str, model: str, ollama_url: str
    ) -> None:
        """Background thread body for the demo-cow run: estimate each file
        in order and report results back to the UI thread via ``after``.
        Stops at the first error."""
        estimator = CowWeightEstimator(backend=backend, model=model, ollama_url=ollama_url)
        for index, filename in enumerate(demo_files, start=1):
            message = f"Estimating {_short_name(filename)} ({index}/{len(demo_files)})…"
            self.root.after(0, self.status_text.set, message)
            try:
                result = estimator.estimate(image_file_to_data_uri(filename), prompt)
            except (OSError, ValueError) as exc:
                self.root.after(0, self._show_error, str(exc))
                return
            self.root.after(
                0,
                self._show_demo_result,
                result,
                filename,
            )
        self.root.after(0, self._finish_demo_run)

    def _show_demo_result(self, result: dict, filename: str) -> None:
        weight_kg = result["estimated_weight_kg"]
        weight_lbs = result.get("estimated_weight_lbs")
        if weight_lbs is not None:
            weight_str = f"{weight_kg:g} kg / {weight_lbs:g} lbs"
        else:
            weight_str = f"{weight_kg:g} kg"
        self.result_text.set(f"Last demo cow: {_short_name(filename)} → {weight_str}")
        self._set_reply(result.get("model_response", ""))
        self._add_history(weight_kg, result["source"], filename)

    def _finish_demo_run(self) -> None:
        self.progress.stop()
        self.status_text.set("Demo run completed.")
        self.estimate_button.configure(state="normal")
        self.demo_button.configure(state="normal")
        self.copy_button.configure(state="normal")
        self.retry_button.configure(state="normal")

    def _estimate_in_background(
        self,
        filename: str,
        prompt: str,
        backend: str,
        model: str,
        ollama_url: str,
        start_time: float,
    ) -> None:
        try:
            estimator = CowWeightEstimator(
                backend=backend, model=model, ollama_url=ollama_url
            )
            result = estimator.estimate(image_file_to_data_uri(filename), prompt)
        except (OSError, ValueError) as exc:
            self.root.after(0, self._show_error, str(exc))
            return
        elapsed = time.monotonic() - start_time
        self.root.after(0, self._show_result, result, filename, elapsed)

    def _show_error(self, error: str) -> None:
        self.progress.stop()
        self.status_text.set("Could not estimate weight.")
        self.estimate_button.configure(state="normal")
        self.retry_button.configure(state="normal" if self.last_request else "disabled")
        messagebox.showerror(WINDOW_TITLE, error)

    def _format_result_text(self, result: dict, elapsed: float) -> str:
        weight_kg = result["estimated_weight_kg"]
        weight_lbs = result.get("estimated_weight_lbs")
        if weight_lbs is not None:
            base = f"Estimated weight: {weight_kg:g} kg / {weight_lbs:g} lbs"
        else:
            base = f"Estimated weight: {weight_kg:g} kg"
        return f"{base} ({elapsed:.2f}s)"

    def _show_result(self, result: dict, filename: str, elapsed: float) -> None:
        self.progress.stop()
        source = result["source"]
        status = f"Estimate completed using {source} in {elapsed:.2f}s."
        breed = result.get("breed")
        confidence = result.get("confidence")
        if breed:
            status += f" Breed: {breed}."
        if confidence is not None:
            status += f" Confidence: {confidence:.0%}."
        self.status_text.set(status)
        self.result_text.set(self._format_result_text(result, elapsed))
        self.estimate_button.configure(state="normal")
        self.copy_button.configure(state="normal")
        self.retry_button.configure(state="normal")
        self._set_reply(result.get("model_response", ""))
        self._add_history(result["estimated_weight_kg"], source, filename, elapsed)

    def retry(self) -> None:
        if not self.last_request:
            return
        if self.last_request[0] == "demo":
            self.estimate_demo_cows()
        else:
            self.estimate()

    def _set_reply(self, text: str) -> None:
        self.reply_text.configure(state="normal")
        self.reply_text.delete("1.0", "end")
        if text:
            self.reply_text.insert("1.0", text)
        self.reply_text.configure(state="disabled")

    def _add_history(
        self, weight_kg: float, source: str, filename: str, elapsed: float | None = None
    ) -> None:
        timestamp = time.strftime("%H:%M:%S")
        weight_text = f"{weight_kg:g}" + (f" ({elapsed:.2f}s)" if elapsed is not None else "")
        self.history.insert(
            "",
            "end",
            values=(timestamp, _short_name(filename), weight_text, source),
        )
        children = self.history.get_children("")
        if len(children) > HISTORY_MAX_ROWS:
            self.history.delete(children[0])

    def copy_result(self) -> None:
        text = self.result_text.get()
        if not text:
            return
        weight = text.replace("Estimated weight: ", "")
        self.root.clipboard_clear()
        self.root.clipboard_append(weight)
        self.status_text.set(f"Copied: {weight}")


def main() -> None:
    setup_logging()
    root = tk.Tk()
    CowWeightApp(root)
    root.mainloop()


if __name__ == "__main__":
    main()
