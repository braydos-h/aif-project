# Cow Weight Estimator

Estimate a cow's weight from a photo. Ships as a dependency-free Python HTTP
API (`app.py`) and a Tkinter desktop app (`gui.py`). Standard library only —
no `pip install` required to run.

The default backend is **Ollama Cloud**, which sends the image to a vision
model and extracts the weight from its reply. A deterministic offline fallback
is included for testing and air-gapped use.

---

## Highlights

- **Two interfaces, one estimator.** A JSON HTTP API for automation and a
  double-clickable Windows GUI for interactive use. Both share the same
  `CowWeightEstimator` and `.env` configuration.
- **Zero runtime dependencies.** Pure standard library (`http.server`,
  `urllib`, `hashlib`, `base64`, `re`). Loads its own `.env` at startup via a
  stdlib-only loader, so no `python-dotenv` either.
- **Pluggable backend.** `ollama` (vision model via Ollama Cloud, default) or
  `none` (stable SHA-256-derived local estimate, no network).
- **Robust input handling.** Accepts `image_url` or raw/base64 `image_base64`,
  including `data:` URIs. Strips WebP data-URI prefixes even when Windows labels
  them `application/octet-stream`.
- **Tested.** Full HTTP request/response suite (real server on a background
  thread, not in-process calls) plus unit tests for weight extraction, backend
  selection, and bearer-token auth.
- **One-file Windows `.exe`.** A GitHub Actions workflow builds a standalone
  `CowWeightEstimator.exe` with PyInstaller on every published release.

---

## Requirements

- **Python 3.8+** (tested on 3.12 in CI).
- No third-party packages. `tkinter` ships with standard Python installers on
  Windows; on some Linux distros you may need `python3-tk`.
- For the Ollama Cloud backend: an Ollama API key (see [Configuration](#configuration)).

---

## Quick start

### Desktop app (Windows)

Double-click `start_gui.bat`, or run:

```powershell
python gui.py
```

Pick a cow image, tweak the prompt if you like, and select **Estimate weight**.
The estimate runs on a background thread so the window stays responsive. No
command window, no separate server to keep running — the GUI calls the
estimator directly.

### HTTP API server

```powershell
python app.py
```

Listens on `http://127.0.0.1:8080`. See [API reference](#api-reference).

---

## API reference

### `POST /estimate-weight`

Estimate a cow's weight from an image. Send **either** `image_url` **or**
`image_base64`; `prompt` is optional and defaults to a sensible built-in prompt.

#### Request body

| Field          | Type   | Required                  | Description                                                                            |
| -------------- | ------ | ------------------------- | -------------------------------------------------------------------------------------- |
| `image_url`    | string | one of `image_*` required | URL the server will fetch (HTTPS or HTTP) and encode.                                  |
| `image_base64` | string | one of `image_*` required | Raw base64 image bytes, or a `data:<mime>;base64,...` URI.                              |
| `prompt`       | string | no                        | Override the estimation prompt. Defaults to `DEFAULT_PROMPT` from `app.py`.           |

#### Example: URL

```bash
curl -s http://127.0.0.1:8080/estimate-weight \
  -H "Content-Type: application/json" \
  -d "{\"image_url\": \"https://example.com/cow.jpg\", \"prompt\": \"Estimate this cow's weight in kg.\"}"
```

```json
{
  "estimated_weight_kg": 612.0,
  "source": "ollama",
  "model": "gemma4:31b",
  "prompt_used": "Estimate this cow's weight in kg."
}
```

#### Example: base64

```json
{
  "image_base64": "iVBORw0KGgoAAAANSUhEUgAA..."
}
```

#### Response

| Field                  | Type    | Always present | Description                                                              |
| ---------------------- | ------- | -------------- | ------------------------------------------------------------------------ |
| `estimated_weight_kg`  | number  | yes            | The estimated weight in kilograms.                                       |
| `source`               | string  | yes            | `ollama` or `local_fallback`.                                            |
| `model`                | string  | ollama only    | The model name used.                                                     |
| `prompt_used`          | string  | yes            | The prompt actually sent (useful when the default was applied).         |

#### Status codes

| Status | Meaning                                                              |
| ------ | -------------------------------------------------------------------- |
| `200`  | Success.                                                             |
| `400`  | Bad request — missing image, invalid JSON, or no body.               |
| `404`  | Unknown path (only `POST /estimate-weight` is served).               |
| `502`  | Estimator failure — backend unreachable, no key, or unparseable reply. |

Any other method or path returns `404`.

---

## Configuration

All settings are read from environment variables, with `.env` used as a
fallback at startup. Values already in the environment take precedence over
`.env`. The estimator is constructed once at import time, so changes after the
server has started have no effect — restart to pick up new config.

| Variable           | Default                          | Used by          | Description                                                                 |
| ------------------ | -------------------------------- | ---------------- | --------------------------------------------------------------------------- |
| `AIF_AI_BACKEND`   | `ollama`                         | both             | `ollama` (Ollama Cloud) or `none` (deterministic local fallback).          |
| `AIF_OLLAMA_URL`   | `https://ollama.com/api/generate` | ollama           | Ollama Cloud generate endpoint.                                            |
| `AIF_AI_MODEL`     | `gemma4:31b`                      | ollama           | Model name sent in the request.                                            |
| `OLLAMA_API_KEY`   | _(none)_                         | ollama (cloud)   | Bearer token. Required when `AIF_OLLAMA_URL` points at `ollama.com`.       |

### Setting up Ollama Cloud

1. Create an API key at <https://ollama.com/settings/keys>.
2. Put it in `.env` (or export it in your shell):

   ```ini
   OLLAMA_API_KEY=your-key-here
   ```

3. Ensure `AIF_AI_BACKEND=ollama` (it's the default).

The default model is `gemma4:31b`, the direct-cloud API name. The
`gemma4:31b-cloud` alias only works through a local Ollama runtime, not the
direct cloud endpoint — use `gemma4:31b` for this service.

> Do not commit a real API key. `.env` is checked in here with a placeholder
> only; replace it locally and keep your key out of version control.

### Offline / no-network mode

Set `AIF_AI_BACKEND=none` for a deterministic, SHA-256-derived estimate in the
range 250–900 kg. Stable for a given input — useful for tests, demos, and
air-gapped environments. Reports `source == "local_fallback"`.

---

## Testing

```powershell
python -m unittest discover -s tests -v
```

The HTTP-level tests boot a real `create_server(port=0)` on a background
thread and exercise it over real sockets (not in-process handler calls), so
they validate status codes, JSON serialization, and the full request path.
They force `backend="none"` to stay deterministic and avoid a running LLM.
A separate `OllamaEstimatorTests` class covers weight extraction, data-URI
stripping, backend selection, and bearer-token auth via `unittest.mock` — the
live Ollama network path is not exercised.

Run a single test:

```powershell
python -m unittest tests.test_app.EstimateApiTests.test_estimate_weight_with_image_url -v
```

There is no linter or formatter configured.

---

## Project structure

```
.
├── app.py                       # HTTP API + CowWeightEstimator (the core)
├── gui.py                       # Tkinter desktop app, reuses app.CowWeightEstimator
├── start_gui.bat                # Double-click launcher (pythonw, no console window)
├── .env                         # Local config (committed with placeholders only)
├── tests/
│   └── test_app.py               # HTTP + estimator unit tests
└── .github/workflows/
    └── build-windows.yml         # Release build: tests + PyInstaller .exe upload
```

---

## Architecture

Two layers, both in `app.py`:

- **`CowWeightEstimator`** — the estimation logic, decoupled from HTTP.
  `estimate()` dispatches on the configured backend.
- **`EstimateHandler`** — a `BaseHTTPRequestHandler` subclass. Only
  `POST /estimate-weight` is valid; anything else returns `404`. The estimator
  is a class attribute, so it's built once at import time using env/`.env`.

The GUI (`gui.py`) does not start the HTTP server. It instantiates
`CowWeightEstimator` directly and runs the call on a background thread so the
Tk event loop never blocks.

---

## Releases (Windows `.exe`)

Publishing a GitHub release triggers `.github/workflows/build-windows.yml`,
which runs the test suite on `windows-latest`, builds a one-file windowed
`CowWeightEstimator.exe` with PyInstaller, and attaches it to the release and
as a workflow artifact. Download it from the release page and run it directly —
no Python installation needed on the target machine.

---

## Notes

- The estimator is created at import time. To change backends or keys, edit
  `.env` (or env vars) and restart the server/GUI.
- WebP uploads on Windows are handled even when the OS reports the image as
  `application/octet-stream`: any `data:<mime>;base64,` prefix is stripped.
- Weight is extracted from model output by preferring an explicit
  `<number> kg` (case-insensitive) and falling back to the first bare number.