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

If `AIF_AI_API_URL` is set, the service forwards image + prompt to that AI API.
If not set, a deterministic local fallback estimate is returned.

## Test

```bash
cd /home/runner/work/aif-project/aif-project
python -m unittest discover -s tests -v
```