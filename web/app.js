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
  const demoSelect = $("demo-select");
  const demoButton = $("demo-button");
  const status = $("status");
  const resultArea = $("result-area");
  const historyList = $("history-list");
  const historyEmpty = $("history-empty");
  // In-memory only: cleared on reload, never persisted anywhere.
  const history = [];

  function setStatus(message) {
    status.textContent = message;
  }

  function formatBytes(bytes) {
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  function formatWeight(value) {
    return typeof value === "number" && Number.isFinite(value) ? value.toFixed(1) : "—";
  }

  function supportedFile(file) {
    const name = file.name.toLowerCase();
    return ALLOWED_TYPES.has(file.type) || ALLOWED_EXTENSIONS.has(name.slice(name.lastIndexOf(".")));
  }

  function readAsDataUrl(file) {
    return new Promise((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = () => resolve(String(reader.result));
      reader.onerror = () => reject(new Error(`Could not read ${file.name}.`));
      reader.readAsDataURL(file);
    });
  }

  async function requestEstimate(body) {
    const controller = new AbortController();
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
      if (!response.ok) throw { status: response.status, payload: data };
      if (typeof data.estimated_weight_kg !== "number") throw { status: 0, payload: data };
      return data;
    } finally {
      window.clearTimeout(timeout);
    }
  }

  function errorMessage(error) {
    if (error && error.name === "AbortError") return "Request timed out. Try again.";
    if (error instanceof TypeError) return "The backend could not be reached.";
    const payload = (error && error.payload) || {};
    const code = typeof payload.code === "string" ? payload.code : "";
    if (code === "missing_image") return "Choose an image before estimating.";
    if (code === "invalid_image") return "That file is not a supported image. Use JPEG, PNG, WebP, BMP, or GIF.";
    if (code === "estimation_failed") return "The estimator could not complete the request. Try again.";
    return "Something went wrong. Try again.";
  }

  function showResult(title, detail) {
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
  }

  function renderHistory() {
    historyList.replaceChildren();
    historyEmpty.hidden = history.length > 0;
    historyList.hidden = history.length === 0;
    for (const entry of history) {
      const item = document.createElement("li");
      const name = document.createElement("strong");
      name.textContent = entry.filename;
      const weight = document.createElement("span");
      const lbs = entry.lbs === null ? "" : ` (${entry.lbs} lb)`;
      weight.textContent = `${entry.kg} kg${lbs} · ${entry.source}`;
      item.append(name, weight);
      historyList.append(item);
    }
  }

  async function runEstimates() {
    const files = Array.from(input.files || []);
    if (!files.length) {
      setStatus("Choose at least one image first.");
      return;
    }
    button.disabled = true;
    let succeeded = 0;
    let failed = 0;
    for (let index = 0; index < files.length; index += 1) {
      const file = files[index];
      setStatus(`Estimating ${index + 1} of ${files.length} — ${file.name}…`);
      if (!supportedFile(file)) {
        failed += 1;
        showResult(`${file.name}: unsupported file type.`, "Use JPEG, PNG, WebP, BMP, or GIF.");
        continue;
      }
      if (file.size > MAX_FILE_BYTES) {
        failed += 1;
        showResult(`${file.name}: file is too large.`, `Choose a file under ${formatBytes(MAX_FILE_BYTES)}.`);
        continue;
      }
      try {
        const result = await requestEstimate({ image_base64: await readAsDataUrl(file) });
        const kg = formatWeight(result.estimated_weight_kg);
        const lbs = typeof result.estimated_weight_lbs === "number"
          ? formatWeight(result.estimated_weight_lbs)
          : null;
        const source = typeof result.source === "string" ? result.source : "unknown";
        const breed = typeof result.breed === "string" && result.breed ? ` · ${result.breed}` : "";
        showResult(`${file.name}: ${kg} kg`, lbs === null ? `${source}${breed}` : `${lbs} lb · ${source}${breed}`);
        history.unshift({ filename: file.name, kg, lbs, source });
        if (history.length > MAX_HISTORY) history.length = MAX_HISTORY;
        renderHistory();
        succeeded += 1;
      } catch (error) {
        failed += 1;
        showResult(`${file.name}: estimate failed.`, errorMessage(error));
      }
    }
    button.disabled = false;
    if (files.length === 1) {
      setStatus(succeeded ? "Done." : "Failed. Try again.");
    } else {
      setStatus(`Done — ${succeeded} estimated, ${failed} failed.`);
    }
  }

  async function checkHealth() {
    try {
      const response = await fetch("/health", { cache: "no-store" });
      setStatus(response.ok ? "Backend online — choose images to begin." : "Backend unavailable — choose images to begin.");
    } catch (_error) {
      setStatus("Backend unavailable — choose images to begin.");
    }
  }

  async function loadDemos() {
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
        return;
      }
      for (const demo of demos) {
        const option = document.createElement("option");
        option.value = typeof demo.id === "string" ? demo.id : "";
        option.textContent = typeof demo.name === "string" ? demo.name : `Demo ${option.value}`;
        demoSelect.append(option);
      }
    } catch (_error) {
      demoSelect.replaceChildren();
      const option = document.createElement("option");
      option.value = "";
      option.textContent = "Demo cows unavailable";
      demoSelect.append(option);
    }
  }

  function pushHistory(filename, result) {
    const kg = formatWeight(result.estimated_weight_kg);
    const lbs = typeof result.estimated_weight_lbs === "number"
      ? formatWeight(result.estimated_weight_lbs)
      : null;
    const source = typeof result.source === "string" ? result.source : "unknown";
    const breed = typeof result.breed === "string" && result.breed ? ` · ${result.breed}` : "";
    showResult(`${filename}: ${kg} kg`, lbs === null ? `${source}${breed}` : `${lbs} lb · ${source}${breed}`);
    history.unshift({ filename, kg, lbs, source });
    if (history.length > MAX_HISTORY) history.length = MAX_HISTORY;
    renderHistory();
  }

  async function runDemo() {
    const id = demoSelect.value;
    if (!id) {
      setStatus("Choose a demo cow first.");
      return;
    }
    demoButton.disabled = true;
    const label = demoSelect.options[demoSelect.selectedIndex]?.textContent || `Demo ${id}`;
    setStatus(`Estimating ${label}…`);
    try {
      const url = new URL(`/demo-cows/${encodeURIComponent(id)}`, window.location.origin).toString();
      pushHistory(label, await requestEstimate({ image_url: url }));
      setStatus("Done.");
    } catch (error) {
      showResult(`${label}: estimate failed.`, errorMessage(error));
      setStatus("Failed. Try again.");
    } finally {
      demoButton.disabled = false;
    }
  }

  button.addEventListener("click", runEstimates);
  demoButton.addEventListener("click", runDemo);
  setStatus("Choose images to begin.");
  renderHistory();
  void checkHealth();
  void loadDemos();
})();
