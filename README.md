# aif-project

Minimal API service for estimating cow weight from a camera image payload.

## Run

```bash
python /home/runner/work/aif-project/aif-project/app.py
```

POST JSON to `http://127.0.0.1:8080/estimate-weight`:

```json
{
  "image_url": "https://example.com/cow.jpg",
  "prompt": "Estimate this cow's weight in kg."
}
```

The service uses the **Ollama** runtime by default (configurable via the `.env`
file). It POSTs the image and a prompt to Ollama and extracts the weight from
the model's reply.

Backends (set `AIF_AI_BACKEND` in `.env`):

- `ollama` (default) — local Ollama runtime (`AIF_OLLAMA_URL`, model `AIF_AI_MODEL`)
- `custom` — generic AI API, used when `AIF_AI_API_URL` is set
- `none` — deterministic local fallback estimate (no network calls)

Copy/edit `.env` to point at your Ollama instance and model (defaults:
`http://localhost:11434/api/generate` and `llava`).

## Test

```bash
cd /home/runner/work/aif-project/aif-project
python -m unittest discover -s tests -v
```