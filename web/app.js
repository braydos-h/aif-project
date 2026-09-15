(() => {
  "use strict";

  const MAX_FILE_BYTES = 20 * 1024 * 1024;
  const MAX_HISTORY = 20;
  const ALLOWED_TYPES = new Set([
    "image/jpeg",
    "image/png",
    "image/webp",
    "image/bmp",
    "image/gif",
  ]);
  const ALLOWED_EXTENSIONS = new Set([".jpg", ".jpeg", ".png", ".webp", ".bmp", ".gif"]);

  const $ = (id) => document.getElementById(id);
  const input = $("image-input");
  const button = $("estimate-button");
  const cancelButton = $("cancel-button");
  const progress = $("progress");
  const fileList = $("file-list");
  const batchList = $("batch-list");
  const demoSelect = $("demo-select");
  const demoButton = $("demo-button");
  const demoRetry = $("demo-retry");
  const demoPreview = $("demo-preview");
  const demoStatus = $("demo-status");
  const unitToggle = $("unit-toggle");
  const status = $("status");
  const healthPill = $("health-pill");
  const backendLabel = $("backend-label");
  const resultArea = $("result-area");
  const historyList = $("history-list");
  const historyEmpty = $("history-empty");
  const historyClear = $("history-clear");
  // In-memory only: cleared on reload, never persisted anywhere.
  const history = [];
  let displayUnit = "kg";
  let lastShown = null;
  let busy = false;
  let cancelRequested = false;
  let currentController = null;
  let previewUrls = [];
  let demoEntries = [];

  function setStatus(message) {
    status.textContent = message;
  }

  function setDemoStatus(message) {
    demoStatus.textContent = message;
  }

  function formatBytes(bytes) {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  function formatWeight(value) {
    return typeof value === "number" && Number.isFinite(value) ? value.toFixed(1) : "—";
  }

  function formatTime(date) {
    try {
      return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
    } catch (_error) {
      return "";
    }
  }

  function supportedFile(file) {
    const name = file.name.toLowerCase();
    return ALLOWED_TYPES.has(file.type) || ALLOWED_EXTENSIONS.has(name.slice(name.lastIndexOf(".")));
  }

  function fileProblem(file) {
    if (!supportedFile(file)) return "unsupported type";
    if (file.size > MAX_FILE_BYTES) return `too large (${formatBytes(file.size)})`;
    return null;
  }

  function readAsDataUrl(file) {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result));
      reader.onerror = () => reject(new Error(`Could not read ${file.name}.`));
      reader.readAsDataURL(file);
    });
  }

  function revokePreviews() {
    for (const url of previewUrls) {
      try {
        URL.revokeObjectURL(url);
      } catch (_error) {
        // Ignore revocation failures for already-cleared previews.
      }
    }
    previewUrls = [];
  }

  function renderFileList() {
    revokePreviews();
    const files = Array.from(input.files || []);
    fileList.replaceChildren();
    if (!files.length) {
      fileList.hidden = true;
      return;
    }
    fileList.hidden = false;
    let ready = 0;
    for (const file of files) {
      const item = document.createElement("li");
      const problem = fileProblem(file);
      if (!problem) ready += 1;
      if (file.type.startsWith("image/")) {
        const thumb = document.createElement("img");
        const url = URL.createObjectURL(file);
        previewUrls.push(url);
        thumb.src = url;
        thumb.alt = "";
        thumb.loading = "lazy";
        item.append(thumb);
      }
      const name = document.createElement("strong");
      name.textContent = file.name;
      const meta = document.createElement("span");
      meta.textContent = problem ? `${formatBytes(file.size)} · ${problem}` : `${formatBytes(file.size)} · ready`;
      item.append(name, meta);
      if (problem) item.classList.add("invalid");
      fileList.append(item);
    }
    const blocked = files.length - ready;
    if (blocked > 0) {
      setStatus(`${ready} of ${files.length} ready — ${blocked} need attention before estimating.`);
    }
  }

  async function requestEstimate(body, externalSignal) {
    const controller = new AbortController();
    currentController = controller;
    const onAbort = () => controller.abort();
    if (externalSignal) {
      if (externalSignal.aborted) controller.abort();
      else externalSignal.addEventListener("abort", onAbort, { once: true });
    }
    const timeout = window.setTimeout(() => controller.abort(), 90_000);
    try {
      const response = await fetch("/estimate-weight", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(body),
        signal: controller.signal,
      });
      let data = {};
      try {
        data = await response.json();
      } catch (_error) {
        data = {};
      }
      let headerId = "";
      try {
        headerId = response.headers.get("x-request-id") || "";
      } catch (_error) {
        headerId = "";
      }
      const requestId = headerId || (typeof data.request_id === "string" ? data.request_id : "");
      if (!response.ok) throw { status: response.status, payload: data, requestId };
      if (typeof data.estimated_weight_kg !== "number") throw { status: 0, payload: data, requestId };
      data._requestId = requestId;
      return data;
    } finally {
      window.clearTimeout(timeout);
      if (externalSignal) externalSignal.removeEventListener("abort", onAbort);
      if (currentController === controller) currentController = null;
    }
  }

  function errorMessage(error) {
    if (error && error.name === "AbortError") {
      if (cancelRequested) return "Cancelled.";
      return "Request timed out. Try again.";
    }
    if (error instanceof TypeError) return "The backend could not be reached.";
    if (error instanceof Error && !error.payload) return error.message || "Something went wrong. Try again.";
    const payload = (error && error.payload) || {};
    const code = typeof payload.code === "string" ? payload.code : "";
    const rid = (error && error.requestId) || (typeof payload.request_id === "string" ? payload.request_id : "");
    const suffix = [code ? `code: ${code}` : "", rid ? `request: ${rid}` : ""].filter(Boolean).join(", ");
    const withRef = (base) => (suffix ? `${base} (${suffix})` : base);
    if (code === "missing_image") return withRef("Choose an image before estimating.");
    if (code === "invalid_image") return withRef("That file is not a supported image. Use JPEG, PNG, WebP, BMP, or GIF.");
    if (code === "estimation_failed") return withRef("The estimator could not complete the request. Try again.");
    if (code === "invalid_options" || code === "missing_body" || code === "invalid_json") {
      return withRef("The request was rejected. Check the file and try again.");
    }
    if (code) return withRef("Something went wrong. Try again.");
    if (rid) return `Something went wrong. Try again. (request: ${rid})`;
    return "Something went wrong. Try again.";
  }

  function showResult(title, detail, meta) {
    resultArea.replaceChildren();
    const heading = document.createElement("p");
    heading.className = "answer";
    heading.textContent = title;
    resultArea.append(heading);
    if (detail) {
      const sub = document.createElement("p");
      sub.className = "detail";
      sub.textContent = detail;
      resultArea.append(sub);
    }
    if (meta) {
      const ref = document.createElement("p");
      ref.className = "detail meta";
      ref.textContent = meta;
      resultArea.append(ref);
    }
  }

  function renderBatch(entries) {
    batchList.replaceChildren();
    if (!entries.length) {
      batchList.hidden = true;
      return;
    }
    batchList.hidden = false;
    for (const entry of entries) {
      const item = document.createElement("li");
      const name = document.createElement("strong");
      name.textContent = entry.filename;
      const weight = document.createElement("span");
      weight.textContent = entry.label;
      item.append(name, weight);
      if (!entry.ok) item.classList.add("invalid");
      batchList.append(item);
    }
  }

  function renderHistory() {
    historyList.replaceChildren();
    historyEmpty.hidden = history.length > 0;
    historyList.hidden = history.length === 0;
    historyClear.hidden = history.length === 0;
    for (const entry of history) {
      const item = document.createElement("li");
      const top = document.createElement("div");
      top.className = "history-top";
      const name = document.createElement("strong");
      name.textContent = entry.filename;
      const weight = document.createElement("span");
      weight.textContent = historyLabel(entry);
      top.append(name, weight);
      const meta = document.createElement("p");
      meta.className = "history-meta";
      const bits = [entry.time, entry.kind, entry.requestId ? `request ${entry.requestId}` : ""].filter(Boolean);
      meta.textContent = bits.join(" · ");
      item.append(top, meta);
      historyList.append(item);
    }
  }

  function formatConfidence(value) {
    if (typeof value !== "number" || !Number.isFinite(value)) return null;
    return `${Math.round(value * 100)}% confident`;
  }

  function formatScore(value) {
    return typeof value === "number" && Number.isFinite(value) ? `BCS ${value.toFixed(1)}` : null;
  }

  function sanityNotes(result) {
    const notes = [];
    const kg = result.estimated_weight_kg;
    if (typeof kg === "number" && Number.isFinite(kg) && (kg < 200 || kg > 1200)) {
      notes.push("Outside the typical 200–1200 kg range — retake a side photo and verify with a scale.");
    }
    const confidence = result.confidence;
    if (typeof confidence === "number" && Number.isFinite(confidence) && confidence < 0.5) {
      notes.push("Low confidence — retake in better light with the full body visible.");
    }
    if (result.source === "local_fallback") {
      notes.push("Offline placeholder — connect Ollama for an AI estimate.");
    }
    return notes;
  }

  function describeResult(filename, result) {
    const kg = formatWeight(result.estimated_weight_kg);
    const lbs = typeof result.estimated_weight_lbs === "number"
      ? formatWeight(result.estimated_weight_lbs)
      : null;
    const source = typeof result.source === "string" ? result.source : "unknown";
    const breed = typeof result.breed === "string" && result.breed ? result.breed : null;
    const confidence = formatConfidence(result.confidence);
    const score = formatScore(result.body_condition_score);
    const notes = sanityNotes(result);
    const primary = displayUnit === "lb" && lbs !== null ? `${lbs} lb` : `${kg} kg`;
    const secondary = displayUnit === "lb" && lbs !== null ? `${kg} kg` : (lbs === null ? null : `${lbs} lb`);
    const parts = [secondary, source, breed, confidence, score].filter(Boolean);
    const detail = parts.join(" · ") + (notes.length ? ` — ${notes.join(" ")}` : "");
    const model = typeof result.model === "string" && result.model ? result.model : null;
    const rid = typeof result._requestId === "string" && result._requestId
      ? result._requestId
      : (typeof result.request_id === "string" ? result.request_id : null);
    const metaBits = [model ? `model ${model}` : "", rid ? `request ${rid}` : ""].filter(Boolean);
    return { title: `${filename}: ${primary}`, detail, meta: metaBits.join(" · ") };
  }

  function historyLabel(entry) {
    const primary = displayUnit === "lb" && entry.lbs !== null ? `${entry.lbs} lb` : `${entry.kg} kg`;
    const rest = [entry.source, entry.breed, entry.confidence, entry.score].filter(Boolean);
    return rest.length ? `${primary} · ${rest.join(" · ")}` : primary;
  }

  function setBusy(next) {
    busy = next;
    button.disabled = next;
    demoButton.disabled = next || !demoSelect.value;
    input.disabled = next;
    cancelButton.hidden = !next;
    progress.hidden = !next;
    if (!next) {
      currentController = null;
      progress.setAttribute("aria-valuenow", "0");
    }
  }

  function updateProgress(done, total) {
    const pct = total ? Math.round((done / total) * 100) : 0;
    progress.setAttribute("aria-valuenow", String(pct));
    progress.textContent = total ? `Estimating ${Math.min(done + 1, total)} of ${total}… ${pct}%` : "";
  }

  async function runEstimates() {
    if (busy) return;
    const files = Array.from(input.files || []);
    if (!files.length) {
      setStatus("Choose at least one image first.");
      return;
    }
    setBusy(true);
    cancelRequested = false;
    const batchSignal = new AbortController();
    const batchEntries = [];
    renderBatch([]);
    let succeeded = 0;
    let failed = 0;
    let cancelled = false;
    for (let index = 0; index < files.length; index += 1) {
      if (cancelRequested) {
        cancelled = true;
        break;
      }
      const file = files[index];
      updateProgress(index, files.length);
      setStatus(`Estimating ${index + 1} of ${files.length} — ${file.name}…`);
      const problem = fileProblem(file);
      if (problem) {
        failed += 1;
        const label = problem === "unsupported type" ? "unsupported file type" : "file is too large";
        const hint = problem === "unsupported type"
          ? "Use JPEG, PNG, WebP, BMP, or GIF."
          : `Choose a file under ${formatBytes(MAX_FILE_BYTES)}.`;
        showResult(`${file.name}: ${label}.`, hint);
        batchEntries.push({ filename: file.name, label: `${label}`, ok: false });
        renderBatch(batchEntries);
        continue;
      }
      try {
        const dataUrl = await readAsDataUrl(file);
        if (cancelRequested) {
          cancelled = true;
          break;
        }
        const result = await requestEstimate({ image_base64: dataUrl }, batchSignal.signal);
        pushHistory(file.name, result, "upload");
        succeeded += 1;
        batchEntries.push({ filename: file.name, label: historyLabel(history[0]), ok: true });
        renderBatch(batchEntries);
      } catch (error) {
        if (cancelRequested) {
          cancelled = true;
          break;
        }
        failed += 1;
        const message = errorMessage(error);
        showResult(`${file.name}: estimate failed.`, message);
        batchEntries.push({ filename: file.name, label: "failed", ok: false });
        renderBatch(batchEntries);
      }
    }
    progress.setAttribute("aria-valuenow", "100");
    setBusy(false);
    if (cancelled) {
      setStatus(`Cancelled — ${succeeded} estimated, ${failed} failed.`);
    } else if (files.length === 1) {
      setStatus(succeeded ? "Done." : "Failed. Try again.");
    } else {
      setStatus(`Done — ${succeeded} estimated, ${failed} failed.`);
    }
    try {
      resultArea.focus({ preventScroll: false });
    } catch (_error) {
      // Focus is a convenience only; ignore when unavailable.
    }
  }

  function requestCancel() {
    if (!busy) return;
    cancelRequested = true;
    if (currentController) {
      try {
        currentController.abort();
      } catch (_error) {
        // Abort is best-effort; the loop also checks the flag.
      }
    }
    setStatus("Cancelling…");
  }

  async function checkHealth() {
    healthPill.textContent = "Checking backend…";
    backendLabel.textContent = "";
    try {
      const response = await fetch("/health", { cache: "no-store" });
      let data = {};
      try {
        data = await response.json();
      } catch (_error) {
        data = {};
      }
      if (!response.ok) throw new Error("unhealthy");
      healthPill.textContent = "Backend online";
      const backend = typeof data.backend === "string" ? data.backend : "";
      const model = typeof data.model === "string" ? data.model : "";
      const parts = [backend, model].filter(Boolean).join(" · ");
      backendLabel.textContent = parts ? `(${parts})` : "";
      if (!busy) setStatus("Backend online — choose images to begin.");
      void loadInfo();
    } catch (_error) {
      healthPill.textContent = "Backend unavailable";
      backendLabel.textContent = "";
      if (!busy) setStatus("Backend unavailable — choose images to begin.");
    }
  }

  async function loadInfo() {
    try {
      const response = await fetch("/info", { cache: "no-store" });
      const data = await response.json();
      const backend = typeof data.backend === "string" ? data.backend : "";
      const model = typeof data.model === "string" ? data.model : "";
      const configured = data.ollama_configured === true ? "key set" : "no key";
      const parts = [backend, model, configured].filter(Boolean).join(" · ");
      if (parts) backendLabel.textContent = `(${parts})`;
    } catch (_error) {
      // Health already covers the offline case; info is enrichment only.
    }
  }

  function updateDemoPreview() {
    const id = demoSelect.value;
    const entry = demoEntries.find((d) => d.id === id);
    if (!entry) {
      demoPreview.hidden = true;
      demoPreview.removeAttribute("src");
      return;
    }
    demoPreview.src = `/demo-cows/${encodeURIComponent(entry.id)}`;
    demoPreview.alt = `Preview of ${entry.name}`;
    demoPreview.hidden = false;
  }

  async function loadDemos() {
    demoButton.disabled = true;
    demoRetry.hidden = true;
    setDemoStatus("Loading demo cows…");
    try {
      const response = await fetch("/demo-cows", { cache: "no-store" });
      const payload = await response.json();
      const demos = Array.isArray(payload.demos) ? payload.demos : [];
      demoSelect.replaceChildren();
      if (!demos.length) {
        const option = document.createElement("option");
        option.value = "";
        option.textContent = "Demo cows unavailable";
        demoSelect.append(option);
        demoEntries = [];
        updateDemoPreview();
        setDemoStatus("Demo cows unavailable.");
        demoRetry.hidden = false;
        return;
      }
      demoEntries = demos
        .filter((d) => typeof d.id === "string" && d.id)
        .map((d) => ({
          id: d.id,
          name: typeof d.name === "string" && d.name ? d.name : `Demo ${d.id}`,
        }));
      for (const demo of demoEntries) {
        const option = document.createElement("option");
        option.value = demo.id;
        option.textContent = demo.name;
        demoSelect.append(option);
      }
      setDemoStatus("");
      updateDemoPreview();
    } catch (_error) {
      demoSelect.replaceChildren();
      const option = document.createElement("option");
      option.value = "";
      option.textContent = "Demo cows unavailable";
      demoSelect.append(option);
      demoEntries = [];
      updateDemoPreview();
      setDemoStatus("Could not load demo cows.");
      demoRetry.hidden = false;
    } finally {
      demoButton.disabled = busy || !demoSelect.value;
    }
  }

  function pushHistory(filename, result, kind) {
    const described = describeResult(filename, result);
    const stamp = formatTime(new Date());
    const rid = typeof result._requestId === "string" && result._requestId
      ? result._requestId
      : (typeof result.request_id === "string" ? result.request_id : "");
    const model = typeof result.model === "string" && result.model ? result.model : null;
    const meta = [stamp, described.meta].filter(Boolean).join(" · ");
    showResult(described.title, described.detail, meta || null);
    lastShown = { filename, result };
    const breed = typeof result.breed === "string" && result.breed ? result.breed : null;
    history.unshift({
      filename,
      kg: formatWeight(result.estimated_weight_kg),
      lbs: typeof result.estimated_weight_lbs === "number"
        ? formatWeight(result.estimated_weight_lbs)
        : null,
      source: typeof result.source === "string" ? result.source : "unknown",
      breed,
      confidence: formatConfidence(result.confidence),
      score: formatScore(result.body_condition_score),
      model,
      requestId: rid,
      time: stamp,
      kind: kind === "demo" ? "demo" : "upload",
    });
    if (history.length > MAX_HISTORY) history.length = MAX_HISTORY;
    renderHistory();
  }

  function clearHistory() {
    history.length = 0;
    lastShown = null;
    renderHistory();
    setStatus("History cleared.");
  }

  function toggleUnits() {
    displayUnit = displayUnit === "kg" ? "lb" : "kg";
    unitToggle.textContent = displayUnit === "kg" ? "Show in lb" : "Show in kg";
    if (lastShown) {
      const described = describeResult(lastShown.filename, lastShown.result);
      const stamp = formatTime(new Date());
      showResult(described.title, described.detail, described.meta || stamp);
    }
    renderHistory();
    const batch = history.slice().reverse().map((e) => ({ filename: e.filename, label: historyLabel(e), ok: true }));
    if (batch.length) renderBatch(batch.slice(-Math.min(batch.length, 20)));
  }

  async function runDemo() {
    if (busy) return;
    const id = demoSelect.value;
    if (!id) {
      setStatus("Choose a demo cow first.");
      return;
    }
    setBusy(true);
    cancelRequested = false;
    const label = demoSelect.options[demoSelect.selectedIndex]?.textContent || `Demo ${id}`;
    setStatus(`Estimating ${label}…`);
    setDemoStatus(`Estimating ${label}…`);
    try {
      const url = new URL(`/demo-cows/${encodeURIComponent(id)}`, window.location.origin).toString();
      const result = await requestEstimate({ image_url: url });
      pushHistory(label, result, "demo");
      setStatus("Done.");
      setDemoStatus("Done.");
    } catch (error) {
      showResult(`${label}: estimate failed.`, errorMessage(error));
      setStatus("Failed. Try again.");
      setDemoStatus("Estimate failed. Try again.");
    } finally {
      setBusy(false);
      demoButton.disabled = !demoSelect.value;
    }
    try {
      resultArea.focus({ preventScroll: false });
    } catch (_error) {
      // Focus is a convenience only.
    }
  }

  button.addEventListener("click", runEstimates);
  cancelButton.addEventListener("click", requestCancel);
  demoButton.addEventListener("click", runDemo);
  demoRetry.addEventListener("click", loadDemos);
  demoSelect.addEventListener("change", () => {
    updateDemoPreview();
    if (!busy) demoButton.disabled = !demoSelect.value;
  });
  unitToggle.addEventListener("click", toggleUnits);
  historyClear.addEventListener("click", clearHistory);
  input.addEventListener("change", renderFileList);
  setStatus("Choose images to begin.");
  setDemoStatus("");
  renderHistory();
  renderBatch([]);
  void checkHealth();
  void loadDemos();
})();
