# aif-project

Minimal API service for estimating cow weight from a camera image payload.

## Windows desktop app

Double-click `start_gui.bat` in File Explorer to open the app without a command
window. You can also start it from PowerShell:

```powershell
python gui.py
```

Choose a cow image, optionally adjust the prompt, and select **Estimate
weight**. The desktop app uses the same backend and `.env` configuration as the
API, so no separate server or command window needs to stay running.

## API server

```powershell
python app.py
```

POST JSON to `http://127.0.0.1:8080/estimate-weight`:

```json
{
  "image_url": "https://example.com/cow.jpg",
  "prompt": "Estimate this cow's weight in kg."
}
```

The service uses **Ollama Cloud** by default (configurable via `.env`). It sends
the image and prompt to Ollama and extracts the weight from the model's reply.

Backends (set `AIF_AI_BACKEND` in `.env`):

- `ollama` (default) — Ollama Cloud (`AIF_OLLAMA_URL`, `OLLAMA_API_KEY`, model `AIF_AI_MODEL`)
- `custom` — generic AI API, used when `AIF_AI_API_URL` is set
- `none` — deterministic local fallback estimate (no network calls)

Create an API key in [Ollama settings](https://ollama.com/settings/keys), then
set `OLLAMA_API_KEY` in `.env`. The default cloud endpoint is
`https://ollama.com/api/generate` and the direct-cloud model name is
`gemma4:31b`. The `gemma4:31b-cloud` alias is only used through a local Ollama
runtime.

## Test

```bash
cd /home/runner/work/aif-project/aif-project
python -m unittest discover -s tests -v
```
