(() => {
  "use strict";

  const FALLBACK_PROMPT =
    "Estimate this cow's weight in kilograms from the provided image. " +
    'Reply with ONLY a JSON object of the form ' +
    '{"weight_kg": <number>, "confidence": <0..1>, ' +
    '"breed": <string>, "body_condition_score": <1..9>} ' +
    'where confidence is your confidence in the estimate (0..1), breed is your ' +
    'best guess of the breed (or "unknown"), and body_condition_score is a ' +
    "1-9 score. Do not include any text outside the JSON object.";
  const MAX_FILE_BYTES = 15 * 1024 * 1024;
  const ALLOWED_TYPES = new Set([
    "image/jpeg",
    "image/png",
    "image/webp",
    "image/bmp",
    "image/gif",
  ]);
  const ALLOWED_EXTENSIONS = new Set([".jpg", ".jpeg", ".png", ".webp", ".bmp", ".gif"]);

  const $ = (id) => document.getElementById(id);
  const refs = {
    dropzone: $("dropzone"),
    dropzoneEmpty: $("dropzone-empty"),
    previewWrap: $("preview-wrap"),
    previewImage: $("preview-image"),
    imageInput: $("image-input"),
    fileMeta: $("file-meta"),
    fileName: $("file-name"),
    fileSize: $("file-size"),
    removeImage: $("remove-image"),
    estimate: $("estimate-button"),
    demo: $("demo-button"),
    status: $("status"),
    healthDot: $("health-dot"),
    healthText: $("health-text"),
    backend: $("backend-select"),
    model: $("model-input"),
    ollamaUrl: $("ollama-url-input"),
    apiKey: $("api-key-input"),
    toggleKey: $("toggle-key"),
    prompt: $("prompt-input"),
    resetPrompt: $("reset-prompt"),
    emptyResult: $("empty-result"),
    resultContent: $("result-content"),
    source: $("result-source"),
    weightKg: $("weight-kg"),
    weightLbs: $("weight-lbs"),
    confidence: $("confidence-value"),
    breed: $("breed-value"),
    condition: $("condition-value"),
    elapsed: $("elapsed-value"),
    requestId: $("request-id-value"),
    backendResult: $("backend-value"),
    modelResponse: $("model-response"),
    errorContent: $("error-content"),
    errorTitle: $("error-title"),
    errorMessage: $("error-message"),
    errorRequestId: $("error-request-id"),
    copy: $("copy-button"),
    retry: $("retry-button"),
    clearHistory: $("clear-history"),
    historyEmpty: $("history-empty"),
    historyList: $("history-list"),
  };
  const state = {
    file: null,
    previewUrl: "",
    defaultPrompt: FALLBACK_PROMPT,
    busy: false,
    history: [],
    lastRequest: null,
    lastResult: null,
  };

  function setStatus(message, kind = "") {
    refs.status.textContent = message;
    refs.status.className = `status${kind ? ` ${kind}` : ""}`;
  }

  function setEstimateLabel(busy) {
    // Support both legacy plain-text button and new span+arrow layout.
    const label = busy ? "Estimating…" : "Estimate Weight";
    const span = refs.estimate.querySelector("span");
    if (span) {
      span.textContent = label;
      const arrow = refs.estimate.querySelector(".btn-arrow");
      if (arrow) arrow.hidden = busy;
    } else {
      refs.estimate.textContent = label;
    }
  }

  function setBusy(busy) {
    state.busy = busy;
    refs.estimate.disabled = busy;
    refs.demo.disabled = busy;
    refs.removeImage.disabled = busy;
    refs.copy.disabled = busy || !state.lastResult;
    refs.retry.disabled = busy || !state.lastRequest;
    setEstimateLabel(busy);
  }

  function formatBytes(bytes) {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  function formatNumber(value) {
    return typeof value === "number" && Number.isFinite(value)
      ? new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 }).format(value)
      : "—";
  }

  function supportedFile(file) {
    const name = file.name.toLowerCase();
    const extension = name.slice(name.lastIndexOf("."));
    return ALLOWED_TYPES.has(file.type) || ALLOWED_EXTENSIONS.has(extension);
  }

  function selectFile(file) {
    if (!file) return;
    if (!supportedFile(file)) {
      setStatus("Choose a JPEG, PNG, WebP, BMP, or GIF image.", "error");
      return;
    }
    if (file.size > MAX_FILE_BYTES) {
      setStatus(`That image is too large. Choose a file under ${formatBytes(MAX_FILE_BYTES)}.`, "error");
      return;
    }
    if (state.previewUrl) URL.revokeObjectURL(state.previewUrl);
    state.file = file;
    state.previewUrl = URL.createObjectURL(file);
    state.lastResult = null;
    refs.previewImage.src = state.previewUrl;
    refs.previewImage.alt = `Preview of ${file.name}`;
    refs.dropzoneEmpty.hidden = true;
    refs.previewWrap.hidden = false;
    refs.fileMeta.hidden = false;
    refs.fileName.textContent = file.name;
    refs.fileSize.textContent = formatBytes(file.size);
    refs.emptyResult.hidden = false;
    refs.resultContent.hidden = true;
    refs.errorContent.hidden = true;
    refs.source.hidden = true;
    refs.copy.disabled = true;
    setStatus("Ready to estimate — hit Estimate Weight or Ctrl+Enter.");
  }

  function clearFile() {
    if (state.previewUrl) URL.revokeObjectURL(state.previewUrl);
    state.file = null;
    state.previewUrl = "";
    refs.imageInput.value = "";
    refs.dropzoneEmpty.hidden = false;
    refs.previewWrap.hidden = true;
    refs.fileMeta.hidden = true;
    setStatus("Choose an image to begin.");
  }

  function readAsDataUrl(blob) {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result));
      reader.onerror = () => reject(new Error("The image could not be read."));
      reader.readAsDataURL(blob);
    });
  }

  function settingsPayload(imageBase64) {
    const payload = {
      image_base64: imageBase64,
      backend: refs.backend.value,
      model: refs.model.value.trim(),
      ollama_url: refs.ollamaUrl.value.trim(),
      prompt: refs.prompt.value.trim(),
    };
    const key = refs.apiKey.value;
    if (key) payload.ollama_api_key = key;
    return payload;
  }

  async function postEstimate(imageBase64) {
    const controller = new AbortController();
    const timeout = window.setTimeout(() => controller.abort(), 90_000);
    try {
      const response = await fetch("/estimate-weight", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(settingsPayload(imageBase64)),
        signal: controller.signal,
      });
      let payload = {};
      try {
        payload = await response.json();
      } catch (_error) {
        payload = {};
      }
      if (!response.ok) throw { kind: "api", status: response.status, payload };
      if (typeof payload.estimated_weight_kg !== "number") {
        throw { kind: "malformed", payload };
      }
      return payload;
    } finally {
      window.clearTimeout(timeout);
    }
  }

  function errorDetails(error) {
    if (error && error.name === "AbortError") {
      return { title: "Estimation timed out", message: "The request took too long. Check the backend or try again." };
    }
    if (error && error.kind === "client") return error;
    if (error instanceof TypeError || !error || (!error.kind && !error.payload)) {
      return { title: "Backend unavailable", message: "The backend could not be reached. Start the server or try again." };
    }
    if (error && error.kind === "malformed") {
      return { title: "Unexpected response", message: "The backend returned a response the WebUI could not read." };
    }
    const payload = error && error.payload ? error.payload : {};
    const requestId = typeof payload.request_id === "string" ? payload.request_id : "";
    const messages = {
      missing_body: "The request body was empty.",
      missing_image: "Choose an image before estimating.",
      invalid_image: "That file is not a supported image. Choose a JPEG, PNG, WebP, BMP, or GIF.",
      invalid_json: "The request could not be read. Try again.",
      invalid_options: "One of the advanced settings is invalid. Check the URL, model, and prompt.",
      estimation_failed: "The estimator could not complete the request. Check Ollama settings and backend health.",
      not_found: "The requested backend route was not found.",
    };
    const code = typeof payload.code === "string" ? payload.code : "backend_error";
    return {
      title: code === "estimation_failed" ? "Estimator unavailable" : "Request unavailable",
      message: messages[code] || "The backend returned an error. Try again.",
      requestId,
    };
  }

  function renderError(error) {
    const details = errorDetails(error);
    refs.emptyResult.hidden = true;
    refs.resultContent.hidden = true;
    refs.source.hidden = true;
    refs.errorContent.hidden = false;
    refs.errorTitle.textContent = details.title;
    refs.errorMessage.textContent = details.message;
    refs.errorRequestId.textContent = details.requestId ? `Request ID: ${details.requestId}` : "";
    refs.copy.disabled = true;
    setStatus(details.message, "error");
  }

  function renderResult(result, elapsedMs) {
    const source = result.source || "unknown";
    refs.emptyResult.hidden = true;
    refs.errorContent.hidden = true;
    refs.resultContent.hidden = false;
    refs.source.hidden = false;
    refs.source.textContent = source === "local_fallback" ? "local fallback" : source;
    refs.weightKg.textContent = formatNumber(result.estimated_weight_kg);
    refs.weightLbs.textContent = result.estimated_weight_lbs == null ? "— lb" : `${formatNumber(result.estimated_weight_lbs)} lb`;
    refs.confidence.textContent = typeof result.confidence === "number" ? `${Math.round(result.confidence * 100)}%` : "—";
    refs.breed.textContent = result.breed || "—";
    refs.condition.textContent = result.body_condition_score == null ? "—" : formatNumber(result.body_condition_score);
    refs.elapsed.textContent = `${(elapsedMs / 1000).toFixed(2)} s`;
    refs.requestId.textContent = result.request_id || "—";
    refs.backendResult.textContent = source;
    refs.modelResponse.textContent = result.model_response || "No raw model response returned.";
    refs.copy.disabled = false;
    refs.retry.disabled = false;
  }

  function addHistory(result, meta, elapsedMs) {
    state.history.unshift({
      time: new Date(),
      filename: meta.filename,
      thumbnailUrl: meta.thumbnailUrl,
      weightKg: result.estimated_weight_kg,
      breed: result.breed || "",
      confidence: result.confidence,
      source: result.source || "unknown",
      elapsedMs,
    });
    state.history = state.history.slice(0, 20);
    renderHistory();
  }

  function renderHistory() {
    refs.historyList.replaceChildren();
    refs.historyEmpty.hidden = state.history.length > 0;
    refs.historyList.hidden = state.history.length === 0;
    for (const entry of state.history) {
      const item = document.createElement("article");
      item.className = "history-item";
      item.setAttribute("role", "listitem");
      const image = document.createElement("img");
      image.className = "history-thumb";
      image.src = entry.thumbnailUrl;
      image.alt = `Thumbnail of ${entry.filename}`;
      const detail = document.createElement("div");
      detail.className = "history-detail";
      const title = document.createElement("strong");
      title.textContent = entry.filename;
      const subtitle = document.createElement("span");
      const confidence = typeof entry.confidence === "number" ? ` · ${Math.round(entry.confidence * 100)}% confidence` : "";
      subtitle.textContent = `${entry.time.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })} · ${entry.source}${entry.breed ? ` · ${entry.breed}` : ""}${confidence}`;
      detail.append(title, subtitle);
      const weight = document.createElement("div");
      weight.className = "history-weight";
      const weightValue = document.createElement("strong");
      weightValue.textContent = `${formatNumber(entry.weightKg)} kg`;
      const elapsed = document.createElement("span");
      elapsed.textContent = `${(entry.elapsedMs / 1000).toFixed(2)} s`;
      weight.append(weightValue, elapsed);
      item.append(image, detail, weight);
      refs.historyList.append(item);
    }
  }

  async function sendEstimate(imageBase64, meta) {
    const started = performance.now();
    try {
      const result = await postEstimate(imageBase64);
      const elapsedMs = performance.now() - started;
      state.lastResult = result;
      renderResult(result, elapsedMs);
      addHistory(result, meta, elapsedMs);
      return true;
    } catch (error) {
      state.lastResult = null;
      renderError(error);
      return false;
    }
  }

  async function runSelected() {
    if (state.busy) return;
    if (!state.file) {
      setStatus("Choose an image before estimating.", "error");
      refs.dropzone.focus();
      return;
    }
    const file = state.file;
    state.lastRequest = { kind: "file", file, filename: file.name, thumbnailUrl: state.previewUrl };
    setBusy(true);
    setStatus("Estimating weight…", "loading");
    try {
      const imageBase64 = await readAsDataUrl(file);
      const ok = await sendEstimate(imageBase64, { filename: file.name, thumbnailUrl: state.previewUrl });
      if (ok) setStatus(`Estimate completed — ${state.lastResult.source} · ${formatNumber(state.lastResult.estimated_weight_kg)} kg`, "success");
    } catch (error) {
      renderError({ kind: "client", title: "Image unavailable", message: error.message });
    } finally {
      setBusy(false);
    }
  }

  async function runDemoCows() {
    if (state.busy) return;
    state.lastRequest = { kind: "demo" };
    setBusy(true);
    refs.emptyResult.hidden = true;
    setStatus("Loading demo cows…", "loading");
    let demos;
    try {
      const response = await fetch("/demo-cows", { cache: "no-store" });
      if (!response.ok) throw { kind: "client", title: "Demo cows unavailable", message: "The bundled demo list could not be loaded." };
      const data = await response.json();
      demos = Array.isArray(data.demos) ? data.demos : [];
      if (!demos.length) throw { kind: "client", title: "No demo cows found", message: "The backend did not return any demo images." };
    } catch (error) {
      renderError(error);
      setBusy(false);
      return;
    }

    let successful = 0;
    let failed = 0;
    for (let index = 0; index < demos.length; index += 1) {
      const demo = demos[index];
      setStatus(`Estimating ${demo.name} (${index + 1}/${demos.length})…`, "loading");
      try {
        const imageResponse = await fetch(demo.url, { cache: "no-store" });
        if (!imageResponse.ok) throw { kind: "client", title: "Demo image unavailable", message: `Could not load ${demo.name}.` };
        const blob = await imageResponse.blob();
        const thumbnailUrl = URL.createObjectURL(blob);
        const imageBase64 = await readAsDataUrl(blob);
        if (await sendEstimate(imageBase64, { filename: demo.name, thumbnailUrl })) successful += 1;
        else failed += 1;
      } catch (error) {
        failed += 1;
        renderError(error);
      }
    }
    setBusy(false);
    setStatus(
      `Demo run complete — ${successful} succeeded${failed ? `, ${failed} failed` : ""}.`,
      failed ? "warning" : "success",
    );
  }

  async function retry() {
    if (state.busy || !state.lastRequest) return;
    if (state.lastRequest.kind === "demo") return runDemoCows();
    const request = state.lastRequest;
    state.file = request.file;
    setBusy(true);
    setStatus("Retrying estimate…", "loading");
    try {
      const imageBase64 = await readAsDataUrl(request.file);
      const ok = await sendEstimate(imageBase64, { filename: request.filename, thumbnailUrl: request.thumbnailUrl });
      if (ok) setStatus(`Estimate completed — ${state.lastResult.source} · ${formatNumber(state.lastResult.estimated_weight_kg)} kg`, "success");
    } catch (error) {
      renderError({ kind: "client", title: "Image unavailable", message: error.message });
    } finally {
      setBusy(false);
    }
  }

  async function copyResult() {
    if (!state.lastResult) return;
    const kg = formatNumber(state.lastResult.estimated_weight_kg);
    const lbs = state.lastResult.estimated_weight_lbs == null ? "" : ` / ${formatNumber(state.lastResult.estimated_weight_lbs)} lb`;
    const breed = state.lastResult.breed ? ` · ${state.lastResult.breed}` : "";
    const text = `Estimated weight: ${kg} kg${lbs}${breed} — via ${state.lastResult.source}`;
    try {
      await navigator.clipboard.writeText(text);
      setStatus("Result copied to the clipboard.", "success");
    } catch (_error) {
      const fallback = document.createElement("textarea");
      fallback.value = text;
      fallback.setAttribute("readonly", "");
      fallback.style.position = "fixed";
      fallback.style.opacity = "0";
      document.body.append(fallback);
      fallback.select();
      const copied = document.execCommand("copy");
      fallback.remove();
      setStatus(copied ? "Result copied to the clipboard." : "Clipboard access is unavailable in this browser.", copied ? "success" : "error");
    }
  }

  async function loadInfo() {
    try {
      const response = await fetch("/info", { cache: "no-store" });
      if (!response.ok) throw new Error("info unavailable");
      const info = await response.json();
      if (info.backend === "ollama" || info.backend === "none") refs.backend.value = info.backend;
      if (typeof info.model === "string") refs.model.value = info.model;
      if (typeof info.ollama_url === "string") refs.ollamaUrl.value = info.ollama_url;
      if (typeof info.default_prompt === "string") state.defaultPrompt = info.default_prompt;
      refs.prompt.value = state.defaultPrompt;
    } catch (_error) {
      refs.prompt.value = state.defaultPrompt;
    }
  }

  async function loadHealth() {
    try {
      const response = await fetch("/health", { cache: "no-store" });
      if (!response.ok) throw new Error("health unavailable");
      const health = await response.json();
      refs.healthDot.className = "health-dot online";
      const configured = health.ollama_configured ? " · Ollama configured" : "";
      const model = health.model ? ` · ${health.model}` : "";
      refs.healthText.textContent = `Backend online · ${health.backend || "unknown"}${model}${configured}`;
    } catch (_error) {
      refs.healthDot.className = "health-dot offline";
      refs.healthText.textContent = "Backend unavailable";
    }
  }

  function wireEvents() {
    refs.dropzone.addEventListener("click", () => refs.imageInput.click());
    refs.dropzone.addEventListener("keydown", (event) => {
      if (event.key === "Enter" || event.key === " ") {
        event.preventDefault();
        refs.imageInput.click();
      }
    });
    refs.dropzone.addEventListener("dragover", (event) => {
      event.preventDefault();
      refs.dropzone.classList.add("dragging");
    });
    refs.dropzone.addEventListener("dragleave", (event) => {
      if (!refs.dropzone.contains(event.relatedTarget)) refs.dropzone.classList.remove("dragging");
    });
    refs.dropzone.addEventListener("drop", (event) => {
      event.preventDefault();
      refs.dropzone.classList.remove("dragging");
      selectFile(event.dataTransfer.files[0]);
    });
    refs.imageInput.addEventListener("change", () => selectFile(refs.imageInput.files[0]));
    refs.removeImage.addEventListener("click", clearFile);
    refs.estimate.addEventListener("click", runSelected);
    refs.demo.addEventListener("click", runDemoCows);
    refs.retry.addEventListener("click", retry);
    refs.copy.addEventListener("click", copyResult);
    refs.clearHistory.addEventListener("click", () => {
      state.history = [];
      renderHistory();
      setStatus("Session history cleared.");
    });
    refs.toggleKey.addEventListener("click", () => {
      const visible = refs.apiKey.type === "text";
      refs.apiKey.type = visible ? "password" : "text";
      refs.toggleKey.textContent = visible ? "Show" : "Hide";
      refs.toggleKey.setAttribute("aria-pressed", String(!visible));
    });
    refs.resetPrompt.addEventListener("click", () => {
      refs.prompt.value = state.defaultPrompt;
      setStatus("Prompt reset to the default.");
    });
    refs.prompt.addEventListener("keydown", (event) => {
      if (event.key === "Enter" && event.ctrlKey) {
        event.preventDefault();
        runSelected();
      }
    });
    // Paste from clipboard (instrument feel: Cmd+V anywhere drops image)
    document.addEventListener("paste", (event) => {
      const file = Array.from(event.clipboardData?.files || []).find((f) => supportedFile(f));
      if (file) selectFile(file);
    });
  }

  wireEvents();
  renderHistory();
  setStatus("Choose an image to begin.");
  setBusy(false);
  void loadInfo();
  void loadHealth();
})();
