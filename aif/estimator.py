"""The estimation core: backend-agnostic cow weight estimation.

``CowWeightEstimator`` dispatches ``estimate()`` to the configured backend
(see "How to add a new backend" below). It is decoupled from HTTP and the
GUI — both the server (``aif.server``) and the desktop app (``aif.gui``)
share this module.
"""

import base64
import hashlib
import json
import logging
import os
import re
import time
import urllib.error
import urllib.request
from typing import Any
from urllib.parse import urlparse

from .config import (
    DEFAULT_CACHE_TTL,
    DEFAULT_OLLAMA_MODEL,
    DEFAULT_OLLAMA_URL,
    DEFAULT_PROMPT,
    IMAGE_MAGIC_BYTES,
    KG_TO_LBS,
    OLLAMA_MAX_RETRIES,
    OLLAMA_RETRY_BACKOFF,
)

logger = logging.getLogger("aif")


class ImageValidationError(ValueError):
    """Raised when the supplied image bytes are not a recognised image format."""


def _kg_to_lbs(kg: float) -> float:
    """Convert kilograms to pounds, rounded to one decimal place."""
    return round(kg * KG_TO_LBS, 1)


def _validate_image_bytes(image_bytes: bytes) -> None:
    """Raise ImageValidationError if the bytes don't look like a supported image."""
    if not image_bytes:
        raise ImageValidationError("Image payload is empty")
    # WebP: RIFF....WEBP — check the WEBP tag at offset 8.
    if image_bytes.startswith(b"RIFF") and len(image_bytes) >= 12:
        if image_bytes[8:12] == b"WEBP":
            return
    for magic in IMAGE_MAGIC_BYTES:
        if image_bytes.startswith(magic):
            return
    raise ImageValidationError(
        "Image bytes do not match a supported format (JPEG, PNG, GIF, BMP, WebP)"
    )


class CowWeightEstimator:
    """Estimate a cow's weight from an image reference.

    Decoupled from HTTP: the same class is used by the ``EstimateHandler``
    and by the desktop GUI. Configuration comes from constructor arguments
    that fall back to environment variables / the ``.env`` file, which are
    read once at import time — changing env vars after construction has no
    effect on a running server.

    Args:
        model: Model name sent to the backend. Defaults to ``AIF_AI_MODEL``
            or ``DEFAULT_OLLAMA_MODEL``.
        backend: ``"ollama"`` (default) or ``"none"``. Defaults to the
            ``AIF_AI_BACKEND`` env var.
        ollama_url: Ollama-compatible generate endpoint. Defaults to
            ``AIF_OLLAMA_URL`` or ``DEFAULT_OLLAMA_URL``.
        cache_ttl: Seconds to cache ollama results per image (0 disables).
            Defaults to ``AIF_CACHE_TTL`` or ``DEFAULT_CACHE_TTL``.
        api_key: Ollama API key (Bearer token). Defaults to the
            ``OLLAMA_API_KEY`` env var.

    Backends:
        - ``ollama`` — POSTs the image (base64) and prompt to the Ollama
          Cloud endpoint with a bearer token, retrying once on transient
          failures. The reply is parsed as structured JSON first
          (``{"weight_kg": ..., ...}``) and falls back to free-text weight
          extraction; see ``_parse_structured_response``.
        - ``none`` — deterministic SHA-256-derived estimate in the range
          250–900 kg, no network. Stable for a given input (tests rely on
          this). Reports ``source == "local_fallback"``.

    How to add a new backend:
        1. Write a private method ``_estimate_via_<name>(image_reference,
           prompt)`` returning a dict shaped like the ollama result
           (``estimated_weight_kg``, ``estimated_weight_lbs``, ``source``,
           ``prompt_used``, plus extras like ``model_response``). Raise
           ``ValueError`` on any failure.
        2. Accept the backend name in ``__init__`` (constructor arg and/or a
           new ``AIF_*`` env var) and dispatch to it in ``estimate()``.
        3. Document it in README.md, CONTRIBUTING.md, and .env.example, and
           add tests in tests/.
    """

    def __init__(
        self,
        model: str | None = None,
        backend: str | None = None,
        ollama_url: str | None = None,
        cache_ttl: int | None = None,
        api_key: str | None = None,
    ) -> None:
        self.ollama_url = ollama_url or os.environ.get("AIF_OLLAMA_URL", DEFAULT_OLLAMA_URL)
        self.ollama_api_key = api_key or os.environ.get("OLLAMA_API_KEY")
        self.model = model or os.environ.get("AIF_AI_MODEL", DEFAULT_OLLAMA_MODEL)
        self.backend = backend or os.environ.get("AIF_AI_BACKEND", "ollama")
        if cache_ttl is None:
            try:
                cache_ttl = int(os.environ.get("AIF_CACHE_TTL", str(DEFAULT_CACHE_TTL)))
            except ValueError:
                cache_ttl = DEFAULT_CACHE_TTL
        self.cache_ttl = max(0, cache_ttl)
        # key -> (expires_at, result_dict)
        self._cache: dict[str, tuple[float, dict[str, Any]]] = {}

    def estimate(self, image_reference: str, prompt: str | None = None) -> dict[str, Any]:
        """Estimate the weight of a cow from an image.

        Args:
            image_reference: An ``https://``/``http://`` URL, a
                ``data:<mime>;base64,...`` URI, or raw base64 image bytes.
            prompt: Optional prompt override; falls back to ``DEFAULT_PROMPT``.

        Returns:
            A dict with at least ``estimated_weight_kg``,
            ``estimated_weight_lbs``, ``source`` and ``prompt_used``; the
            ``ollama`` backend also includes ``model`` and ``model_response``
            plus optional structured extras (``confidence``, ``breed``,
            ``body_condition_score``).

        Raises:
            ImageValidationError: If the image bytes are not a supported
                format (JPEG, PNG, GIF, BMP, WebP) or not valid base64.
            ValueError: If the backend fails (network error, missing API
                key, unparseable reply).
        """
        prompt_to_use = prompt or DEFAULT_PROMPT
        if self.backend == "none":
            return self._estimate_fallback(image_reference, prompt_to_use)
        return self._estimate_via_ollama(image_reference, prompt_to_use)

    def _cache_get(self, key: str) -> dict[str, Any] | None:
        """Return a shallow copy of the cached result for ``key`` if it is
        still within its TTL, else None. Expired entries are removed."""
        if self.cache_ttl <= 0:
            return None
        entry = self._cache.get(key)
        if not entry:
            return None
        expires_at, result = entry
        if time.monotonic() > expires_at:
            self._cache.pop(key, None)
            return None
        return dict(result)  # shallow copy so callers can't mutate the cache

    def _cache_put(self, key: str, result: dict[str, Any]) -> None:
        """Store ``result`` under ``key`` with the configured TTL. No-op when
        ``cache_ttl <= 0``."""
        if self.cache_ttl <= 0:
            return
        self._cache[key] = (time.monotonic() + self.cache_ttl, result)

    def _estimate_via_ollama(self, image_reference: str, prompt: str) -> dict[str, Any]:
        """Estimate by sending the image to Ollama Cloud and parsing its reply.

        Sends ``{"model", "prompt", "images": [base64], "stream": false}`` to
        ``self.ollama_url``. The API key is required when the URL host is
        ``ollama.com`` and is sent as a ``Bearer`` token. Results are cached
        per image hash (see ``cache_ttl``).

        Raises:
            ValueError: Missing API key, network failure, HTTP error, empty
                reply, or a reply with no extractable weight.
        """
        if urlparse(self.ollama_url).hostname == "ollama.com" and not self.ollama_api_key:
            raise ValueError(
                "Ollama Cloud requires an API key. Set OLLAMA_API_KEY in .env "
                "to an API key created at https://ollama.com/settings/keys."
            )

        image_b64 = self._to_base64_image(image_reference)
        cache_key = hashlib.sha256(image_b64.encode("utf-8")).hexdigest()
        cached = self._cache_get(cache_key)
        if cached is not None:
            logger.info("cache hit for image %s", cache_key[:12])
            return cached

        payload = json.dumps(
            {
                "model": self.model,
                "prompt": prompt,
                "images": [image_b64],
                "stream": False,
            }
        ).encode("utf-8")
        request = urllib.request.Request(
            self.ollama_url,
            data=payload,
            method="POST",
            headers={"Content-Type": "application/json"},
        )
        if self.ollama_api_key:
            request.add_header("Authorization", f"Bearer {self.ollama_api_key}")

        text = self._call_ollama_with_retry(request)
        weight_kg, extras = self._parse_structured_response(text)
        if weight_kg is None:
            raise ValueError(f"Could not extract a weight from Ollama response: {text!r}")

        result: dict[str, Any] = {
            "estimated_weight_kg": weight_kg,
            "estimated_weight_lbs": _kg_to_lbs(weight_kg),
            "source": "ollama",
            "model": self.model,
            "prompt_used": prompt,
            "model_response": text,
        }
        result.update(extras)
        self._cache_put(cache_key, result)
        return result

    def _call_ollama_with_retry(self, request: urllib.request.Request) -> str:
        """Call the Ollama endpoint, retrying once on transient failures."""
        last_exc: Exception | None = None
        for attempt in range(OLLAMA_MAX_RETRIES + 1):
            try:
                with urllib.request.urlopen(request, timeout=60) as response:
                    body = response.read().decode("utf-8")
                    parsed = json.loads(body) if body else {}
                text = parsed.get("response")
                if not text and isinstance(parsed.get("message"), dict):
                    text = parsed["message"].get("content")
                if not text:
                    raise ValueError("Ollama response did not contain any text")
                return text
            except urllib.error.HTTPError as exc:
                error_body = exc.read().decode("utf-8", errors="replace")
                try:
                    error_detail = json.loads(error_body).get("error", error_body)
                except json.JSONDecodeError:
                    error_detail = error_body
                if isinstance(error_detail, str):
                    detail = error_detail.strip()
                else:
                    detail = str(error_detail)
                message = detail or exc.reason
                last_exc = ValueError(
                    f"Ollama request failed (HTTP {exc.code}): {message}"
                )
                # Only retry on 5xx (server-side / transient). 4xx is a real error.
                if exc.code < 500 or attempt == OLLAMA_MAX_RETRIES:
                    raise last_exc from exc
                logger.warning(
                    "Ollama returned HTTP %s, retrying in %.1fs (attempt %d/%d)",
                    exc.code,
                    OLLAMA_RETRY_BACKOFF,
                    attempt + 1,
                    OLLAMA_MAX_RETRIES,
                )
            except (urllib.error.URLError, TimeoutError) as exc:
                last_exc = ValueError(
                    f"Unable to reach Ollama at {self.ollama_url}: {exc}"
                )
                if attempt == OLLAMA_MAX_RETRIES:
                    raise last_exc from exc
                logger.warning(
                    "Ollama unreachable (%s), retrying in %.1fs (attempt %d/%d)",
                    exc,
                    OLLAMA_RETRY_BACKOFF,
                    attempt + 1,
                    OLLAMA_MAX_RETRIES,
                )
            except json.JSONDecodeError as exc:
                # Not transient — don't retry.
                raise ValueError(f"Ollama returned non-JSON body: {exc}") from exc

            time.sleep(OLLAMA_RETRY_BACKOFF)

        # Unreachable: every branch above either returns or raises.
        raise last_exc if last_exc else ValueError("Ollama call failed")

    @staticmethod
    def _parse_structured_response(text: str) -> tuple[float | None, dict[str, Any]]:
        """Pull a weight + extras out of the model's reply.

        Tries to find a JSON object with a ``weight_kg`` field first; if found,
        also extracts ``confidence``, ``breed`` and ``body_condition_score``
        when present. Falls back to ``_extract_weight_from_text`` (which prefers
        ``<n> kg`` then the first bare number) when no usable JSON is present.
        """
        # Find the first {...} block in the text.
        json_match = re.search(r"\{[^{}]*\}", text, re.DOTALL)
        if json_match:
            try:
                obj = json.loads(json_match.group(0))
            except json.JSONDecodeError:
                obj = None
            if isinstance(obj, dict):
                weight = obj.get("weight_kg")
                if weight is not None:
                    try:
                        weight_kg = float(weight)
                    except (TypeError, ValueError):
                        weight_kg = None
                    if weight_kg is not None:
                        extras: dict[str, Any] = {}
                        if "confidence" in obj:
                            try:
                                extras["confidence"] = float(obj["confidence"])
                            except (TypeError, ValueError):
                                pass
                        if "breed" in obj and isinstance(obj["breed"], str):
                            extras["breed"] = obj["breed"]
                        if "body_condition_score" in obj:
                            try:
                                extras["body_condition_score"] = float(
                                    obj["body_condition_score"]
                                )
                            except (TypeError, ValueError):
                                pass
                        return weight_kg, extras

        # No usable JSON — fall back to text extraction.
        return CowWeightEstimator._extract_weight_from_text(text), {}

    def _estimate_fallback(self, image_reference: str, prompt: str) -> dict[str, Any]:
        """Deterministic local estimate: hash the image reference to a stable
        weight in the range 250–900 kg. Never touches the network, so tests
        and air-gapped use can rely on it. Reports
        ``source == "local_fallback"``."""
        digest = hashlib.sha256(image_reference.encode("utf-8")).hexdigest()
        normalized = int(digest[:8], 16) / 0xFFFFFFFF
        estimated_weight_kg = round(250 + (normalized * 650), 1)
        return {
            "estimated_weight_kg": estimated_weight_kg,
            "estimated_weight_lbs": _kg_to_lbs(estimated_weight_kg),
            "source": "local_fallback",
            "prompt_used": prompt,
            "model_response": "",
        }

    @staticmethod
    def _to_base64_image(image_reference: str) -> str:
        """Return raw base64 image bytes from a URL or a base64/data-URI string.

        Validates that the decoded bytes look like a supported image format.
        Raises ImageValidationError if not.
        """
        if image_reference.startswith("http://") or image_reference.startswith("https://"):
            request = urllib.request.Request(
                image_reference, headers={"User-Agent": "aif-project/1.0"}
            )
            with urllib.request.urlopen(request, timeout=30) as response:
                image_bytes = response.read()
            _validate_image_bytes(image_bytes)
            return base64.b64encode(image_bytes).decode("ascii")

        stripped = image_reference
        # Accept any base64 data URI. Some Windows MIME databases do not know
        # image/webp and label WebP files as application/octet-stream.
        data_prefix_match = re.match(r"data:[^,]*;base64,", stripped, re.IGNORECASE)
        if data_prefix_match:
            stripped = stripped[data_prefix_match.end():]
        # Validate the decoded bytes.
        try:
            decoded = base64.b64decode(stripped, validate=True)
        except (ValueError, base64.binascii.Error) as exc:
            raise ImageValidationError("Image base64 payload is not valid base64") from exc
        _validate_image_bytes(decoded)
        return stripped

    @staticmethod
    def _extract_weight_from_text(text: str) -> float | None:
        """Pull a weight in kilograms out of free-form model output."""
        # Prefer an explicit "<number> kg" (case-insensitive).
        match = re.search(r"(\d+(?:\.\d+)?)\s*kg", text, re.IGNORECASE)
        if match:
            return float(match.group(1))
        # Fall back to the first bare number in the text.
        match = re.search(r"\d+(?:\.\d+)?", text)
        if match:
            return float(match.group(0))
        return None
