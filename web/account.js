(() => {
  "use strict";

  // Account, authentication, server history, animal records, and operator
  // UI. Same-origin fetch only; the session lives in an HttpOnly cookie
  // and the CSRF token stays in memory (never in browser storage).
  const $ = (id) => document.getElementById(id);
  const state = {
    authenticated: false,
    authRequired: false,
    production: false,
    user: null,
    csrf: null,
    historyPage: 1,
    historyPerPage: 20,
    historyTotal: 0,
    historyFilter: { animal_id: "", source: "" },
    animalHistoryPage: 1,
    currentAnimal: null,
    sessionProbeFailed: false,
  };
  const animalNames = new Map();
  let batchPollTimer = null;

  function status(message) {
    const el = $("account-status");
    if (el) el.textContent = message;
  }

  function showView(name) {
    if (window.aifPages && typeof window.aifPages.navigate === "function") {
      const paths = { estimator: "/dashboard", login: "/login", invite: "/invite", recovery: "/recover", "recovery-complete": "/reset-password", account: "/settings/profile", "animal-detail": state.currentAnimal ? `/animals/${encodeURIComponent(state.currentAnimal)}` : "/animals" };
      if (paths[name]) {
        window.aifPages.navigate(paths[name]);
        return;
      }
    }
    for (const view of document.querySelectorAll(".auth-view")) {
      view.hidden = view.dataset.view !== name;
    }
    const active = document.querySelector(`.auth-view[data-view="${name}"]`);
    if (active) {
      try {
        active.setAttribute("tabindex", "-1");
        active.focus({ preventScroll: true });
      } catch (_error) {
        // Focus is a convenience only.
      }
    }
    for (const link of document.querySelectorAll("[data-nav]")) {
      const on = link.dataset.nav === name;
      try {
        link.setAttribute("aria-current", on ? "page" : "false");
      } catch (_error) {
        // Cosmetic only.
      }
    }
  }

  async function call(method, path, body) {
    const headers = {};
    let payload;
    if (body !== undefined) {
      headers["Content-Type"] = "application/json";
      payload = JSON.stringify(body);
    }
    if (state.csrf && method !== "GET" && method !== "OPTIONS") {
      headers["X-CSRF-Token"] = state.csrf;
    }
    let response;
    try {
      response = await fetch(path, {
        method,
        headers,
        body: payload,
        credentials: "same-origin",
      });
    } catch (_error) {
      throw { status: 0, payload: {}, message: "The server could not be reached." };
    }
    let data = {};
    try {
      data = await response.json();
    } catch (_error) {
      data = {};
    }
    if (!response.ok) throw { status: response.status, payload: data };
    return data;
  }

  function apiError(error, fallback) {
    const payload = (error && error.payload) || {};
    if (typeof payload.error === "string" && payload.error) return payload.error;
    if (error && error.message) return error.message;
    return fallback;
  }

  function fmtDate(iso) {
    if (!iso) return "—";
    try {
      const date = new Date(iso);
      if (Number.isNaN(date.getTime())) return String(iso);
      return date.toLocaleString([], { dateStyle: "medium", timeStyle: "short" });
    } catch (_error) {
      return String(iso);
    }
  }

  function fmtKg(value) {
    return typeof value === "number" && Number.isFinite(value) ? value.toFixed(1) : "—";
  }

  function jobStatusLabel(statusValue) {
    return ({ active: "processing", success: "succeeded", failed: "failed", queued: "queued", cancelled: "cancelled", expired: "expired" })[statusValue] || statusValue || "unknown";
  }

  function describeSource(item) {
    if (item.source === "scale") return "scale measurement";
    if (item.placeholder || item.source === "local_fallback") return "offline placeholder";
    if (item.source === "tape_measure") return "tape measure";
    return "AI estimate";
  }

  // --- session / navigation -------------------------------------------

  async function refreshMe() {
    let data = {};
    let probeFailed = false;
    try {
      data = await call("GET", "/api/me");
    } catch (error) {
      probeFailed = true;
      data = { authenticated: false, auth_required: false, session_probe_failed: true };
      status(apiError(error, "Could not verify your session. Your private data is hidden until the server responds."));
    }
    const previousUserId = state.user && state.user.id;
    const nextUserId = data.user && data.user.id;
    if (previousUserId && previousUserId !== nextUserId) clearPrivateUI();
    state.sessionProbeFailed = probeFailed;
    state.authenticated = data.authenticated === true;
    state.authRequired = data.auth_required === true;
    state.production = data.production === true;
    state.user = (data.user && typeof data.user === "object") ? data.user : null;
    state.csrf = state.user && typeof state.user.csrf_token === "string" ? state.user.csrf_token : null;
    renderAuthBar();
    if (window.aifPages && typeof window.aifPages.setAuthState === "function") {
      window.aifPages.setAuthState({ ...data, authenticated: state.authenticated, auth_required: state.authRequired, user: state.user, session_probe_failed: probeFailed });
    }
    if (window.aifEstimator && typeof window.aifEstimator.setAuthMode === "function") {
      window.aifEstimator.setAuthMode(state.authenticated, state.authRequired);
    }
    if (state.authenticated) {
      await loadAnimals();
      await Promise.all([loadServerHistory(), loadAccountSummary(), loadJobs(), loadPhotos()]);
      if (state.user && state.user.role === "operator") await loadOperator();
    } else {
      clearPrivateUI();
    }
    return state;
  }

  function renderAuthBar() {
    const bar = $("auth-bar");
    if (!bar) return;
    bar.replaceChildren();
    const home = document.createElement("a");
    home.className = "auth-link";
    home.href = "/dashboard";
    home.textContent = "Estimator";
    home.dataset.nav = "estimator";
    home.addEventListener("click", (event) => {
      event.preventDefault();
      showView("estimator");
    });
    bar.append(home);
    if (!state.authenticated) {
      if (!state.authRequired) {
        const note = document.createElement("span");
        note.className = "auth-user";
        note.textContent = "Local open mode — log in is optional until the server requires it.";
        bar.append(note);
      }
      const login = document.createElement("a");
      login.className = "auth-link";
      login.href = "/login";
      login.textContent = "Log in";
      login.dataset.nav = "login";
      login.addEventListener("click", (event) => {
        event.preventDefault();
        showView("login");
      });
      const invite = document.createElement("a");
      invite.className = "auth-link";
      invite.href = "/invite";
      invite.textContent = "Accept invite";
      invite.dataset.nav = "invite";
      invite.addEventListener("click", (event) => {
        event.preventDefault();
        showView("invite");
      });
      const recovery = document.createElement("a");
      recovery.className = "auth-link";
      recovery.href = "/recover";
      recovery.textContent = "Recover access";
      recovery.dataset.nav = "recovery";
      recovery.addEventListener("click", (event) => {
        event.preventDefault();
        showView("recovery");
      });
      bar.append(login, invite, recovery);
    } else {
      const spacer = document.createElement("span");
      spacer.className = "spacer";
      bar.append(spacer);
      const who = document.createElement("span");
      who.className = "auth-user";
      who.textContent = state.user.email;
      who.title = state.user.email;
      const account = document.createElement("a");
      account.className = "auth-link";
      account.href = "/settings/profile";
      account.textContent = "Account";
      account.dataset.nav = "account";
      account.addEventListener("click", (event) => {
        event.preventDefault();
        showView("account");
      });
      const logout = document.createElement("button");
      logout.type = "button";
      logout.className = "auth-link";
      logout.textContent = "Log out";
      logout.addEventListener("click", doLogout);
      bar.append(who, account, logout);
    }
    const explainer = document.createElement("p");
    explainer.className = "note";
    explainer.textContent = state.authenticated
      ? "Signed in. Estimates you run are saved to your private server history."
      : "Accounts require an invitation from the operator. There is no public registration.";
    bar.append(explainer);
  }

  function clearPrivateUI() {
    if (batchPollTimer) window.clearTimeout(batchPollTimer);
    batchPollTimer = null;
    for (const id of ["server-history-list", "server-history-pager", "animal-list", "invite-list", "user-list", "usage-list", "usage-wrap", "audit-list", "job-list", "photo-list", "upload-batch-list", "upload-batch-detail-body", "history-detail-body", "animal-detail-body", "dashboard-recent", "dashboard-uploads", "invite-output"]) {
      const el = $(id);
      if (el) el.replaceChildren();
    }
    if (window.aifEstimator && typeof window.aifEstimator.clearPrivateBatchFiles === "function") {
      window.aifEstimator.clearPrivateBatchFiles();
    }
    animalNames.clear();
    state.currentAnimal = null;
    state.historyFilter.animal_id = "";
    state.historyTotal = 0;
    const summary = $("account-summary");
    if (summary) summary.textContent = "";
    for (const id of ["dashboard-summary", "server-history-count"]) {
      const el = $(id);
      if (el) el.textContent = "";
    }
    const animalSelect = $("animal-select");
    if (animalSelect) {
      animalSelect.replaceChildren();
      const option = document.createElement("option");
      option.value = "";
      option.textContent = "No animal — do not link";
      animalSelect.append(option);
    }
    const historyAnimal = $("history-filter-animal");
    if (historyAnimal) {
      historyAnimal.replaceChildren();
      const option = document.createElement("option");
      option.value = "";
      option.textContent = "All animals";
      historyAnimal.append(option);
    }
    for (const id of ["profile-name", "animal-id", "animal-name", "animal-breed", "animal-year", "animal-notes", "scale-weight", "scale-when", "delete-password", "invite-email"]) {
      const field = $(id);
      if (field) field.value = "";
    }
    for (const id of ["login-password", "invite-password", "recovery-password", "password-current", "password-new", "delete-password"]) {
      const field = $(id);
      if (field) field.value = "";
    }
    const sex = $("animal-sex");
    if (sex) sex.value = "";
    const confirm = $("delete-confirm");
    if (confirm) confirm.checked = false;
  }

  // --- auth forms ------------------------------------------------------

  async function doLogin(event) {
    event.preventDefault();
    const email = $("login-email").value.trim();
    const password = $("login-password").value;
    if (!email || !password) {
      status("Enter your email and password.");
      return;
    }
    status("Logging in…");
    try {
      await call("POST", "/api/auth/login", { email, password });
      $("login-password").value = "";
      status("Logged in.");
      await refreshMe();
      const next = new URLSearchParams(window.location.search).get("next");
      if (!next || !window.aifPages || !window.aifPages.navigate(next)) showView("estimator");
    } catch (error) {
      $("login-password").value = "";
      if (error && error.status === 429) {
        status("Too many login attempts — wait a few minutes and try again.");
      } else {
        status(apiError(error, "Login failed. Check your email and password."));
      }
    }
  }

  async function doLogout() {
    try {
      await call("POST", "/api/auth/logout", {});
    } catch (_error) {
      // Logging out is best-effort; clear local state regardless.
    }
    state.authenticated = false;
    state.user = null;
    state.csrf = null;
    if (window.aifEstimator && typeof window.aifEstimator.clearPrivateBatchFiles === "function") {
      window.aifEstimator.clearPrivateBatchFiles();
    }
    try {
      window.history.replaceState(null, "", window.location.pathname);
    } catch (_error) {
      // Hash cleanup is cosmetic.
    }
    await refreshMe();
    showView(state.authRequired ? "login" : "estimator");
    status("Logged out.");
  }

  function tokenFromHash(prefix) {
    try {
      const hash = window.location.hash || "";
      const marker = `#${prefix}=`;
      if (hash.startsWith(marker)) return decodeURIComponent(hash.slice(marker.length));
    } catch (_error) {
      // Malformed hash means no token.
    }
    return "";
  }

  function prefillInviteToken() {
    const token = tokenFromHash("invite");
    if (token) {
      $("invite-token").value = token;
      showView("invite");
      status("Invitation link detected — choose a password to activate your account.");
    }
  }

  function prefillRecoveryToken() {
    const token = tokenFromHash("recovery");
    if (token) {
      $("recovery-token").value = token;
      showView("recovery-complete");
      status("Recovery link detected — choose a new password.");
    }
  }

  async function doAcceptInvite(event) {
    event.preventDefault();
    const token = $("invite-token").value.trim();
    const password = $("invite-password").value;
    const displayName = $("invite-name").value.trim();
    if (!token) {
      status("Paste the invitation link or token first.");
      return;
    }
    if (password.length < 10) {
      status("Choose a password of at least 10 characters.");
      return;
    }
    status("Activating your account…");
    try {
      await call("POST", "/api/auth/accept-invite", {
        token,
        password,
        display_name: displayName,
      });
      $("invite-password").value = "";
      try {
        window.history.replaceState(null, "", window.location.pathname);
      } catch (_error) {
        // Hash cleanup is cosmetic.
      }
      status("Account activated — welcome.");
      await refreshMe();
      showView("estimator");
    } catch (error) {
      $("invite-password").value = "";
      status(apiError(error, "That invitation could not be used."));
    }
  }

  async function doRecoveryRequest(event) {
    event.preventDefault();
    const email = $("recovery-email").value.trim();
    if (!email) {
      status("Enter your account email.");
      return;
    }
    status("Sending recovery instructions…");
    try {
      const data = await call("POST", "/api/auth/recovery/request", { email });
      status(typeof data.message === "string" && data.message
        ? data.message
        : "If that email has an account, recovery steps are on the way.");
    } catch (error) {
      status(apiError(error, "Recovery request failed. Try again later."));
    }
  }

  async function doRecoveryComplete(event) {
    event.preventDefault();
    const token = $("recovery-token").value.trim();
    const password = $("recovery-password").value;
    if (!token) {
      status("Paste the recovery link or token first.");
      return;
    }
    if (password.length < 10) {
      status("Choose a password of at least 10 characters.");
      return;
    }
    status("Updating your password…");
    try {
      await call("POST", "/api/auth/recovery/complete", { token, new_password: password });
      $("recovery-password").value = "";
      try {
        window.history.replaceState(null, "", window.location.pathname);
      } catch (_error) {
        // Hash cleanup is cosmetic.
      }
      status("Password updated — log in with your new password.");
      showView("login");
    } catch (error) {
      $("recovery-password").value = "";
      status(apiError(error, "That recovery link is invalid, expired, or already used."));
    }
  }

  // --- account ----------------------------------------------------------

  async function loadAccountSummary() {
    const el = $("account-summary");
    if (!el || !state.authenticated) return;
    try {
      const data = await call("GET", "/api/account");
      const profileName = $("profile-name");
      if (profileName) profileName.value = typeof data.display_name === "string" ? data.display_name : "";
      el.replaceChildren();
      const lines = [
        `Email: ${data.email || ""}`,
        `Display name: ${data.display_name || ""}`,
        `Role: ${data.role || ""}`,
        `Animals: ${data.animals ?? 0} · Saved estimates: ${data.estimates ?? 0}`,
        `Today: ${data.today_image_estimates ?? 0} of ${data.daily_limit ?? 0} photo estimates used`,
        `Photos: ${(typeof data.photo_policy === "string" && data.photo_policy) || "Photo policy unavailable"}`,
      ];
      for (const line of lines) {
        const p = document.createElement("p");
        p.className = "small-meta";
        p.textContent = line;
        el.append(p);
      }
    } catch (error) {
      el.textContent = apiError(error, "Could not load account summary.");
    }
  }

  async function doUpdateProfile(event) {
    event.preventDefault();
    const name = $("profile-name").value.trim();
    if (!name) {
      status("Enter a display name.");
      return;
    }
    try {
      await call("POST", "/api/auth/profile", { display_name: name });
      status("Display name updated.");
      await refreshMe();
    } catch (error) {
      status(apiError(error, "Could not update your display name."));
    }
  }

  async function doChangePassword(event) {
    event.preventDefault();
    const current = $("password-current").value;
    const next = $("password-new").value;
    if (next.length < 10) {
      status("The new password must be at least 10 characters.");
      return;
    }
    try {
      await call("POST", "/api/auth/change-password", { current_password: current, new_password: next });
      $("password-current").value = "";
      $("password-new").value = "";
      status("Password updated — log in again with your new password.");
      await refreshMe();
      showView("login");
    } catch (error) {
      $("password-current").value = "";
      $("password-new").value = "";
      status(apiError(error, "Could not update your password."));
    }
  }

  function doExportAccount() {
    if (!state.authenticated) {
      status("Log in to export your data.");
      return;
    }
    const link = document.createElement("a");
    link.href = "/api/account/export";
    link.download = "cow-weight-account.json";
    document.body.append(link);
    link.click();
    link.remove();
    status("Account export downloading.");
  }

  async function doDeleteAccount(event) {
    event.preventDefault();
    const password = $("delete-password").value;
    const confirm = $("delete-confirm");
    if (!confirm || !confirm.checked) {
      status("Tick the confirmation box to delete your account.");
      return;
    }
    if (!password) {
      status("Enter your password to confirm deletion.");
      return;
    }
    try {
      await call("DELETE", "/api/account", { password });
      $("delete-password").value = "";
      if (confirm) confirm.checked = false;
      status("Your account and all of its data were deleted.");
      await refreshMe();
      showView("estimator");
    } catch (error) {
      $("delete-password").value = "";
      status(apiError(error, "Could not delete your account."));
    }
  }

  // --- server history ----------------------------------------------------

  function historyQuery() {
    const params = new URLSearchParams();
    params.set("page", String(state.historyPage));
    params.set("per_page", String(state.historyPerPage));
    if (state.historyFilter.animal_id) params.set("animal_id", state.historyFilter.animal_id);
    if (state.historyFilter.source) params.set("source", state.historyFilter.source);
    const from = $("history-filter-from");
    const to = $("history-filter-to");
    if (from && from.value) params.set("from", `${from.value}T00:00:00Z`);
    if (to && to.value) params.set("to", `${to.value}T23:59:59Z`);
    return params.toString();
  }

  async function loadServerHistory() {
    const list = $("server-history-list");
    if (!list || !state.authenticated) return;
    list.replaceChildren();
    const empty = $("server-history-empty");
    try {
      const data = await call("GET", `/api/history?${historyQuery()}`);
      const items = Array.isArray(data.items) ? data.items : [];
      state.historyTotal = typeof data.total === "number" ? data.total : items.length;
      if (empty) empty.hidden = items.length > 0;
      for (const item of items) {
        list.append(historyRow(item));
      }
      renderHistoryPager();
      const count = $("server-history-count");
      if (count) {
        count.textContent = state.historyTotal
          ? `Showing page ${data.page || state.historyPage} of ${state.historyTotal} saved estimates.`
          : "No saved estimates yet — run an estimate while logged in.";
      }
    } catch (error) {
      if (empty) empty.hidden = false;
      if (empty) empty.textContent = apiError(error, "Could not load history.");
    }
  }

  function historyRow(item) {
    const li = document.createElement("li");
    const top = document.createElement("div");
    const title = document.createElement("strong");
    const source = describeSource(item);
    const when = fmtDate(item.measured_at || item.created_at);
    title.textContent = `${fmtKg(item.weight_kg)} kg · ${source} · ${when}`;
    const weight = document.createElement("span");
    weight.textContent = `range ${fmtKg(item.weight_min_kg)}–${fmtKg(item.weight_max_kg)} kg`;
    top.append(title, weight);
    const meta = document.createElement("p");
    meta.className = "small-meta";
    const bits = [];
    const animalName = item.animal_id ? animalNames.get(item.animal_id) : "";
    if (animalName) bits.push(`animal ${animalName}`);
    else if (item.animal_breed) bits.push(item.animal_breed);
    if (item.model) bits.push(`model ${item.model}`);
    if (item.estimator_version) bits.push(`estimator ${item.estimator_version}`);
    if (item.prompt_version) bits.push(`prompt ${item.prompt_version}`);
    if (typeof item.disclaimer === "string" && item.disclaimer) bits.push(item.disclaimer);
    meta.textContent = bits.join(" · ");
    li.append(top, meta);
    if (item.id) {
      const detail = document.createElement("a");
      detail.className = "auth-link";
      detail.href = `/history/${encodeURIComponent(item.id)}`;
      detail.textContent = "View details";
      li.append(detail);
      const del = document.createElement("button");
      del.type = "button";
      del.className = "secondary-btn";
      del.textContent = "Delete";
      del.setAttribute("aria-label", `Delete estimate from ${when}`);
      del.addEventListener("click", async () => {
        try {
          await call("DELETE", `/api/history/${encodeURIComponent(item.id)}`);
          status("Estimate deleted.");
          await loadServerHistory();
        } catch (error) {
          status(apiError(error, "Could not delete that estimate."));
        }
      });
      li.append(del);
    }
    return li;
  }

  function renderHistoryPager() {
    const pager = $("server-history-pager");
    if (!pager) return;
    pager.replaceChildren();
    const pages = Math.max(1, Math.ceil(state.historyTotal / state.historyPerPage));
    const info = document.createElement("span");
    info.className = "small-meta";
    info.textContent = `Page ${state.historyPage} of ${pages}`;
    const prev = document.createElement("button");
    prev.type = "button";
    prev.className = "secondary-btn";
    prev.textContent = "Previous";
    prev.disabled = state.historyPage <= 1;
    prev.addEventListener("click", () => {
      if (state.historyPage > 1) {
        state.historyPage -= 1;
        void loadServerHistory();
      }
    });
    const next = document.createElement("button");
    next.type = "button";
    next.className = "secondary-btn";
    next.textContent = "Next";
    next.disabled = state.historyPage >= pages;
    next.addEventListener("click", () => {
      if (state.historyPage < pages) {
        state.historyPage += 1;
        void loadServerHistory();
      }
    });
    pager.append(prev, info, next);
  }

  function applyHistoryFilters(event) {
    event.preventDefault();
    state.historyFilter.animal_id = $("history-filter-animal").value.trim();
    state.historyFilter.source = $("history-filter-source").value;
    state.historyPage = 1;
    void loadServerHistory();
  }

  function doExportHistoryCsv() {
    if (!state.authenticated) {
      status("Log in to export your history.");
      return;
    }
    const link = document.createElement("a");
    const params = new URLSearchParams(historyQuery());
    params.delete("page");
    params.delete("per_page");
    params.set("format", "csv");
    link.href = `/api/history/export?${params.toString()}`;
    link.download = "cow-weight-history.csv";
    document.body.append(link);
    link.click();
    link.remove();
    status("History CSV downloading.");
  }

  // --- animals ------------------------------------------------------------

  async function loadAnimals() {
    const list = $("animal-list");
    const select = $("animal-select");
    const filter = $("history-filter-animal");
    if ((!list && !select) || !state.authenticated) return;
    let animals = [];
    try {
      const data = await call("GET", "/api/animals?include_archived=1");
      animals = Array.isArray(data.animals) ? data.animals : [];
    } catch (error) {
      status(apiError(error, "Could not load animals."));
      return;
    }
    animalNames.clear();
    for (const animal of animals) if (typeof animal.id === "string" && typeof animal.name === "string") animalNames.set(animal.id, animal.name);
    if (list) {
      list.replaceChildren();
      const empty = $("animal-empty");
      if (empty) empty.hidden = animals.length > 0;
      for (const animal of animals) list.append(animalRow(animal));
    }
    if (select) {
      const previous = select.value;
      select.replaceChildren();
      const none = document.createElement("option");
      none.value = "";
      none.textContent = "No animal — do not link";
      select.append(none);
      for (const animal of animals) {
        if (animal.archived) continue;
        const option = document.createElement("option");
        option.value = animal.id;
        option.textContent = `${animal.name} (${animal.estimate_count ?? 0} records)`;
        select.append(option);
      }
      if (previous) select.value = previous;
    }
    if (filter) {
      const previous = state.historyFilter.animal_id;
      filter.replaceChildren();
      const all = document.createElement("option");
      all.value = "";
      all.textContent = "All animals";
      filter.append(all);
      for (const animal of animals) {
        const option = document.createElement("option");
        option.value = animal.id;
        option.textContent = animal.name;
        filter.append(option);
      }
      filter.value = previous;
    }
    document.dispatchEvent(new CustomEvent("aif-animals-loaded"));
  }

  function animalRow(animal) {
    const li = document.createElement("li");
    const name = document.createElement("strong");
    name.textContent = animal.archived ? `${animal.name} (archived)` : animal.name;
    const meta = document.createElement("p");
    meta.className = "small-meta";
    const bits = [];
    if (animal.breed) bits.push(animal.breed);
    if (animal.sex) bits.push(animal.sex);
    if (animal.birth_year) bits.push(`born ${animal.birth_year}`);
    bits.push(`${animal.estimate_count ?? 0} records`);
    meta.textContent = bits.join(" · ");
    li.append(name, meta);
    if (animal.notes) {
      const notes = document.createElement("p");
      notes.className = "small-meta";
      notes.textContent = animal.notes;
      li.append(notes);
    }
    const row = document.createElement("div");
    row.className = "auth-row";
    const view = document.createElement("button");
    view.type = "button";
    view.className = "secondary-btn";
    view.textContent = "View trend";
    view.addEventListener("click", () => void viewAnimal(animal.id));
    const edit = document.createElement("button");
    edit.type = "button";
    edit.className = "secondary-btn";
    edit.textContent = "Edit";
    edit.addEventListener("click", () => fillAnimalForm(animal));
    const scale = document.createElement("button");
    scale.type = "button";
    scale.className = "secondary-btn";
    scale.textContent = "Add scale weight";
    scale.addEventListener("click", () => {
      fillAnimalForm(animal);
      const input = $("scale-weight");
      if (input) input.focus();
    });
    const del = document.createElement("button");
    del.type = "button";
    del.className = "danger-btn";
    del.textContent = "Delete";
    del.setAttribute("aria-label", `Delete ${animal.name}`);
    del.addEventListener("click", async () => {
      try {
        await call("DELETE", `/api/animals/${encodeURIComponent(animal.id)}`);
        status(`Deleted ${animal.name}. Linked estimates were kept, unlinked.`);
        await loadAnimals();
        await loadServerHistory();
      } catch (error) {
        status(apiError(error, "Could not delete that animal."));
      }
    });
    row.append(view, edit, scale, del);
    li.append(row);
    return li;
  }

  function fillAnimalForm(animal) {
    $("animal-id").value = animal.id;
    $("animal-name").value = animal.name || "";
    $("animal-breed").value = animal.breed || "";
    $("animal-sex").value = animal.sex || "";
    $("animal-year").value = animal.birth_year || "";
    $("animal-notes").value = animal.notes || "";
    $("animal-archived").checked = animal.archived === true;
    $("animal-form-title").textContent = `Edit ${animal.name}`;
    if (window.aifPages) window.aifPages.navigate("/animals");
    else showView("account");
  }

  async function doSaveAnimal(event) {
    event.preventDefault();
    const id = $("animal-id").value.trim();
    const body = {
      name: $("animal-name").value.trim(),
      breed: $("animal-breed").value.trim(),
      sex: $("animal-sex").value,
      notes: $("animal-notes").value.trim(),
      archived: $("animal-archived").checked,
    };
    const yearRaw = $("animal-year").value.trim();
    if (yearRaw) {
      const year = Number.parseInt(yearRaw, 10);
      if (!Number.isInteger(year)) {
        status("Birth year must be a whole year.");
        return;
      }
      body.birth_year = year;
    }
    if (!body.name) {
      status("Give the animal a name.");
      return;
    }
    try {
      if (id) {
        await call("PUT", `/api/animals/${encodeURIComponent(id)}`, body);
        status("Animal updated.");
      } else {
        await call("POST", "/api/animals", body);
        status("Animal added.");
      }
      $("animal-id").value = "";
      const form = $("animal-form");
      if (form) form.reset();
      $("animal-form-title").textContent = "Add an animal";
      await loadAnimals();
    } catch (error) {
      status(apiError(error, "Could not save that animal."));
    }
  }

  async function viewAnimal(id) {
    if (window.aifPages) {
      window.aifPages.navigate(`/animals/${encodeURIComponent(id)}`);
      return;
    }
    await viewAnimalPage(id, true);
  }

  async function viewAnimalPage(id, fromRoute) {
    state.currentAnimal = id;
    state.animalHistoryPage = 1;
    await loadAnimalDetail();
    if (!fromRoute && window.aifPages) window.aifPages.navigate(`/animals/${encodeURIComponent(id)}`);
  }

  async function loadHistoryDetail(id) {
    const wrap = $("history-detail-body");
    if (!wrap) return;
    wrap.replaceChildren();
    try {
      const item = await call("GET", `/api/history/${encodeURIComponent(id)}`);
      const title = document.createElement("p");
      title.className = "answer";
      title.textContent = `${fmtKg(item.weight_kg)} kg`;
      const range = document.createElement("p");
      range.textContent = `Estimated range ${fmtKg(item.weight_min_kg)}–${fmtKg(item.weight_max_kg)} kg`;
      const details = document.createElement("dl");
      const pairs = [
        ["Method", describeSource(item)], ["Date", fmtDate(item.measured_at || item.created_at)],
        ["Animal", (item.animal_id && animalNames.get(item.animal_id)) || item.animal_breed || "Not linked"], ["Model", item.model || "Not recorded"],
        ["Estimator version", item.estimator_version || "Not recorded"],
        ["Prompt version", item.prompt_version || "Not recorded"],
        ["Request id", item.request_id || "Not recorded"],
      ];
      for (const [label, value] of pairs) {
        const dt = document.createElement("dt");
        dt.textContent = label;
        const dd = document.createElement("dd");
        dd.textContent = String(value);
        details.append(dt, dd);
      }
      wrap.append(title, range, details);
      if (item.disclaimer) {
        const note = document.createElement("p");
        note.className = "note";
        note.textContent = item.disclaimer;
        wrap.append(note);
      }
      if (item.placeholder || item.source === "local_fallback") {
        const warning = document.createElement("p");
        warning.className = "warning-note";
        warning.textContent = "Offline placeholder — this is not an AI estimate.";
        wrap.append(warning);
      }
    } catch (error) {
      wrap.textContent = apiError(error, "Could not load that estimate.");
    }
  }

  async function loadAnimalDetail() {
    const wrap = $("animal-detail-body");
    if (!wrap || !state.currentAnimal) return;
    wrap.replaceChildren();
    try {
      const data = await call(
        "GET",
        `/api/animals/${encodeURIComponent(state.currentAnimal)}?page=${state.animalHistoryPage}`
      );
      const heading = document.createElement("h3");
      heading.textContent = data.name || "Animal";
      wrap.append(heading);
      const meta = document.createElement("p");
      meta.className = "small-meta";
      const bits = [];
      if (data.breed) bits.push(data.breed);
      if (data.sex) bits.push(data.sex);
      if (data.birth_year) bits.push(`born ${data.birth_year}`);
      bits.push(`${data.estimates_total ?? 0} records`);
      meta.textContent = bits.join(" · ");
      wrap.append(meta);
      const items = Array.isArray(data.estimates) ? data.estimates : [];
      if (!items.length) {
        const empty = document.createElement("p");
        empty.className = "small-meta";
        empty.textContent = "No measurements yet — run an estimate with this animal selected, or add a scale weight.";
        wrap.append(empty);
      }
      for (const item of items) {
        const row = document.createElement("p");
        row.className = "small-meta";
        const label = item.source === "scale" ? "scale" : describeSource(item);
        row.textContent = `${fmtDate(item.measured_at || item.created_at)} · ${fmtKg(item.weight_kg)} kg (${label})`;
        wrap.append(row);
      }
    } catch (error) {
      wrap.textContent = apiError(error, "Could not load that animal.");
    }
  }

  async function doAddScale(event) {
    event.preventDefault();
    const id = $("animal-id").value.trim() || state.currentAnimal;
    if (!id) {
      status("Choose an animal first (Edit it, or open its trend).");
      return;
    }
    const weight = Number.parseFloat($("scale-weight").value);
    if (!Number.isFinite(weight) || weight < 20 || weight > 2500) {
      status("Scale weight must be between 20 and 2500 kg.");
      return;
    }
    const body = { scale_weight_kg: weight };
    const when = $("scale-when").value.trim();
    if (when) body.measured_at = when;
    try {
      await call("POST", `/api/animals/${encodeURIComponent(id)}/measurements`, body);
      status("Scale weight recorded.");
      const form = $("scale-form");
      if (form) form.reset();
      await loadAnimals();
      await loadServerHistory();
      if (state.currentAnimal === id) await loadAnimalDetail();
    } catch (error) {
      status(apiError(error, "Could not record that scale weight."));
    }
  }

  // --- background jobs -----------------------------------------------------

  async function loadJobs() {
    const list = $("job-list");
    if (!list || !state.authenticated) return;
    list.replaceChildren();
    const empty = $("job-empty");
    try {
      const data = await call("GET", "/api/jobs");
      const jobs = Array.isArray(data.jobs) ? data.jobs : [];
      if (empty) empty.hidden = jobs.length > 0;
      for (const job of jobs) list.append(jobRow(job));
    } catch (error) {
      if (empty) {
        empty.hidden = false;
        empty.textContent = apiError(error, "Could not load jobs.");
      }
    }
  }

  async function loadUploadBatches() {
    const list = $("upload-batch-list");
    if (!list || !state.authenticated) return;
    list.replaceChildren();
    const empty = $("upload-batch-empty");
    try {
      const data = await call("GET", "/api/upload-batches");
      const batches = Array.isArray(data.batches) ? data.batches : [];
      if (empty) empty.hidden = batches.length > 0;
      for (const batch of batches) {
        const li = document.createElement("li");
        const link = document.createElement("a");
        link.className = "auth-link";
        link.href = `/uploads/${encodeURIComponent(batch.id)}`;
        link.textContent = `Batch ${String(batch.id).slice(0, 8)} · ${batch.status || "pending"}`;
        const meta = document.createElement("p");
        meta.className = "small-meta";
        meta.textContent = `${batch.succeeded_count ?? 0} succeeded · ${batch.failed_count ?? 0} failed · ${batch.submitted_count ?? 0} of ${batch.expected_count ?? 0} submitted · ${fmtDate(batch.created_at)}`;
        li.append(link, meta);
        list.append(li);
      }
    } catch (error) {
      if (empty) { empty.hidden = false; empty.textContent = apiError(error, "Could not load upload batches."); }
    }
  }

  async function loadUploadBatch(id) {
    if (batchPollTimer) window.clearTimeout(batchPollTimer);
    batchPollTimer = null;
    const wrap = $("upload-batch-detail-body");
    if (!wrap) return;
    wrap.replaceChildren();
    try {
      const batch = await call("GET", `/api/upload-batches/${encodeURIComponent(id)}`);
      const summary = document.createElement("p");
      summary.className = "note";
      summary.textContent = `${batch.status} · ${batch.succeeded_count ?? 0} succeeded · ${batch.failed_count ?? 0} failed · ${batch.cancelled_count ?? 0} cancelled · ${batch.submitted_count ?? 0} of ${batch.expected_count ?? 0} submitted.`;
      if ((batch.submitted_count ?? 0) < (batch.expected_count ?? 0)) {
        const missing = document.createElement("p");
        missing.className = "note";
        missing.textContent = "Some selected photos did not reach the server. Select those photos again to submit them.";
        wrap.append(missing);
      }
      const list = document.createElement("ul");
      for (const item of Array.isArray(batch.items) ? batch.items : []) {
        const li = document.createElement("li");
        const title = document.createElement("strong");
        title.textContent = `${item.filename || "Photo"} · ${jobStatusLabel(item.status)}`;
        const detail = document.createElement("p");
        detail.className = "small-meta";
        const weight = item.result && typeof item.result.estimated_weight_kg === "number" ? `${fmtKg(item.result.estimated_weight_kg)} kg` : "";
        const source = item.result && typeof item.result.source === "string" ? item.result.source : "";
        detail.textContent = [weight, source, item.error_message, `attempts ${item.attempts ?? 0}`].filter(Boolean).join(" · ");
        li.append(title, detail);
        if (item.history_id) {
          const link = document.createElement("a");
          link.className = "auth-link";
          link.href = `/history/${encodeURIComponent(item.history_id)}`;
          link.textContent = "Open saved result";
          li.append(link);
        }
        const retryable = ["failed", "cancelled", "expired"].includes(item.status);
        if (item.status === "success" && window.aifEstimator && typeof window.aifEstimator.forgetBatchItem === "function") {
          window.aifEstimator.forgetBatchItem(id, item.index);
        }
        if (retryable && window.aifEstimator && window.aifEstimator.canRetryBatchItem(id, item.index)) {
          const retry = document.createElement("button");
          retry.type = "button";
          retry.className = "secondary-btn";
          retry.textContent = "Retry this photo";
          retry.addEventListener("click", async () => {
            retry.disabled = true;
            retry.textContent = "Retrying…";
            try {
              await window.aifEstimator.retryBatchItem(id, item.index);
              await loadUploadBatch(id);
            } catch (error) {
              status(error instanceof Error ? error.message : "Could not retry this photo.");
              retry.disabled = false;
              retry.textContent = "Retry this photo";
            }
          });
          li.append(retry);
        } else if (retryable) {
          const unavailable = document.createElement("p");
          unavailable.className = "small-meta";
          unavailable.textContent = "The photo is no longer available in this tab. Reselect it to submit a new attempt.";
          li.append(unavailable);
        }
        list.append(li);
      }
      wrap.append(summary, list);
      const retryableItems = (Array.isArray(batch.items) ? batch.items : []).filter((item) =>
        ["failed", "cancelled", "expired"].includes(item.status)
        && window.aifEstimator && window.aifEstimator.canRetryBatchItem(id, item.index));
      if (retryableItems.length > 1) {
        const retryFailed = document.createElement("button");
        retryFailed.type = "button";
        retryFailed.className = "secondary-btn";
        retryFailed.textContent = `Retry ${retryableItems.length} failed photos`;
        retryFailed.addEventListener("click", async () => {
          retryFailed.disabled = true;
          retryFailed.textContent = "Retrying failed photos…";
          const outcomes = await Promise.allSettled(retryableItems.map((item) => window.aifEstimator.retryBatchItem(id, item.index)));
          const failed = outcomes.filter((result) => result.status === "rejected").length;
          status(failed ? `${failed} photo${failed === 1 ? "" : "s"} could not be retried; review each item.` : "Failed photos requeued.");
          await loadUploadBatch(id);
        });
        wrap.append(retryFailed);
      }
      if (batch.items && batch.items.some((item) => item.status === "queued")) {
        const cancelNote = document.createElement("p");
        cancelNote.className = "note";
        cancelNote.textContent = "Cancelling removes pending photos. Active inference may finish and its result can still be saved.";
        wrap.append(cancelNote);
        const cancel = document.createElement("button");
        cancel.type = "button";
        cancel.className = "secondary-btn";
        cancel.textContent = "Cancel pending photos";
        cancel.addEventListener("click", async () => {
          try {
            await call("POST", `/api/upload-batches/${encodeURIComponent(id)}/cancel`, {});
            await loadUploadBatch(id);
          } catch (error) { status(apiError(error, "Could not cancel pending photos.")); }
        });
        wrap.append(cancel);
      }
      if (batch.items && batch.items.some((item) => item.status === "queued" || item.status === "active")) {
        batchPollTimer = window.setTimeout(() => {
          if (window.location.pathname === `/uploads/${encodeURIComponent(id)}`) void loadUploadBatch(id);
        }, 2500);
      }
    } catch (error) {
      if (error && error.status === 404) {
        try {
          const job = await call("GET", `/api/jobs/${encodeURIComponent(id)}`);
          const heading = document.createElement("h3");
          heading.textContent = `Background estimate · ${jobStatusLabel(job.status)}`;
          const meta = document.createElement("p");
          meta.className = "small-meta";
          meta.textContent = [job.filename, `created ${fmtDate(job.created_at)}`, `attempts ${job.attempts ?? 0}`, job.error_message].filter(Boolean).join(" · ");
          wrap.append(heading, meta);
          if (job.result && typeof job.result.estimated_weight_kg === "number") {
            const result = document.createElement("p");
            result.className = "answer";
            result.textContent = `${fmtKg(job.result.estimated_weight_kg)} kg · ${fmtKg(job.result.weight_min_kg)}–${fmtKg(job.result.weight_max_kg)} kg range`;
            wrap.append(result);
            if (job.result.history_id) {
              const link = document.createElement("a");
              link.className = "auth-link";
              link.href = `/history/${encodeURIComponent(job.result.history_id)}`;
              link.textContent = "Open saved result";
              wrap.append(link);
            }
          }
        } catch (jobError) {
          wrap.textContent = apiError(jobError, "Could not load this upload or job.");
        }
      } else {
        wrap.textContent = apiError(error, "Could not load this upload batch.");
      }
    }
  }

  async function loadDashboard() {
    const summary = $("dashboard-summary");
    const recent = $("dashboard-recent");
    const uploads = $("dashboard-uploads");
    if (!state.authenticated) return;
    try {
      const account = await call("GET", "/api/account");
      if (summary) summary.textContent = `${account.animals ?? 0} animals · ${account.estimates ?? 0} saved estimates · ${account.today_image_estimates ?? 0} of ${account.daily_limit ?? 0} photo estimates used today.`;
      const [history, batchData, jobData] = await Promise.all([
        call("GET", "/api/history?page=1&per_page=5"),
        call("GET", "/api/upload-batches"),
        call("GET", "/api/jobs"),
      ]);
      const historyItems = Array.isArray(history.items) ? history.items : [];
      if (recent) {
        recent.replaceChildren();
        for (const item of historyItems) {
          const row = document.createElement("li");
          const link = document.createElement("a");
          link.href = `/history/${encodeURIComponent(item.id)}`;
          link.textContent = `${fmtKg(item.weight_kg)} kg · ${describeSource(item)} · ${fmtDate(item.measured_at || item.created_at)}`;
          row.append(link);
          recent.append(row);
        }
      }
      const recentEmpty = $("dashboard-recent-empty");
      if (recentEmpty) recentEmpty.hidden = historyItems.length > 0;
      const activeBatches = (Array.isArray(batchData.batches) ? batchData.batches : []).filter((batch) => batch.status === "processing" || batch.status === "pending");
      const activeJobs = (Array.isArray(jobData.jobs) ? jobData.jobs : []).filter((job) => !job.batch_id && (job.status === "queued" || job.status === "active"));
      if (uploads) {
        uploads.replaceChildren();
        for (const batch of activeBatches) {
          const row = document.createElement("li");
          const link = document.createElement("a");
          link.href = `/uploads/${encodeURIComponent(batch.id)}`;
          link.textContent = `Batch ${String(batch.id).slice(0, 8)} · ${batch.status} · ${batch.submitted_count ?? 0} of ${batch.expected_count ?? 0} photos`;
          row.append(link);
          uploads.append(row);
        }
        for (const job of activeJobs) {
          const row = document.createElement("li");
          const link = document.createElement("a");
          link.href = `/uploads/${encodeURIComponent(job.id)}`;
          link.textContent = `Job ${String(job.id).slice(0, 8)} · ${jobStatusLabel(job.status)}`;
          row.append(link);
          uploads.append(row);
        }
      }
      const uploadEmpty = $("dashboard-uploads-empty");
      if (uploadEmpty) uploadEmpty.hidden = activeBatches.length + activeJobs.length > 0;
    } catch (error) {
      if (summary) summary.textContent = apiError(error, "Could not load the dashboard.");
    }
  }

  async function loadForRoute(path) {
    if (!state.authenticated) return;
    const batchDetail = path.match(/^\/uploads\/([A-Za-z0-9_-]{1,64})$/);
    if (batchDetail) return loadUploadBatch(batchDetail[1]);
    const animalDetail = path.match(/^\/animals\/([A-Za-z0-9_-]{1,64})$/);
    if (animalDetail) return viewAnimalPage(animalDetail[1], true);
    const historyDetail = path.match(/^\/history\/([A-Za-z0-9_-]{1,64})$/);
    if (historyDetail) { await loadAnimals(); return loadHistoryDetail(historyDetail[1]); }
    if (path === "/dashboard") return loadDashboard();
    else if (path === "/settings/profile") await Promise.all([loadAccountSummary(), loadAnimals()]);
    else if (path === "/settings/security") await loadAccountSummary();
    else if (path === "/settings/privacy") await Promise.all([loadAccountSummary(), loadPhotos()]);
    else if (path === "/animals") await loadAnimals();
    else if (path === "/history") { await loadAnimals(); await loadServerHistory(); }
    else if (path === "/uploads") await Promise.all([loadJobs(), loadUploadBatches()]);
    else if (path === "/photos") await loadPhotos();
    else if (path.startsWith("/operator") && state.user && state.user.role === "operator") await loadOperator();
  }

  function jobRow(job) {
    const li = document.createElement("li");
    const title = document.createElement("strong");
    const id = typeof job.id === "string" ? job.id : "";
    const short = id ? id.slice(0, 8) : "job";
    title.textContent = `Job ${short} · ${jobStatusLabel(job.status)}`;
    const meta = document.createElement("p");
    meta.className = "small-meta";
    const bits = [`created ${fmtDate(job.created_at)}`];
    if (job.status === "success" && job.result && typeof job.result.estimated_weight_kg === "number") {
      bits.push(`${fmtKg(job.result.estimated_weight_kg)} kg`);
      if (job.result.source === "local_fallback") bits.push("offline placeholder");
    }
    if (job.status === "failed" && job.error_code) bits.push(`failed: ${job.error_code}`);
    if (typeof job.attempts === "number") bits.push(`${job.attempts} attempt${job.attempts === 1 ? "" : "s"}`);
    meta.textContent = bits.join(" · ");
    li.append(title, meta);
    if (job.status === "queued" && id) {
      const cancel = document.createElement("button");
      cancel.type = "button";
      cancel.className = "secondary-btn";
      cancel.textContent = "Cancel";
      cancel.setAttribute("aria-label", `Cancel job ${short}`);
      cancel.addEventListener("click", async () => {
        try {
          await call("POST", `/api/jobs/${encodeURIComponent(id)}/cancel`, {});
          status("Job cancelled.");
          await loadJobs();
        } catch (error) {
          status(apiError(error, "Could not cancel that job."));
        }
      });
      li.append(cancel);
    }
    return li;
  }

  // --- retained photos ------------------------------------------------------

  async function loadPhotos() {
    const list = $("photo-list");
    if (!list || !state.authenticated) return;
    list.replaceChildren();
    const empty = $("photo-empty");
    try {
      const data = await call("GET", "/api/photos");
      const photos = Array.isArray(data.photos) ? data.photos : [];
      if (empty) empty.hidden = photos.length > 0;
      for (const photo of photos) list.append(photoRow(photo));
    } catch (error) {
      if (empty) {
        empty.hidden = false;
        empty.textContent = apiError(error, "Could not load photos.");
      }
    }
  }

  function photoRow(photo) {
    const li = document.createElement("li");
    const title = document.createElement("strong");
    title.textContent = `Photo · ${fmtDate(photo.created_at)}`;
    const meta = document.createElement("p");
    meta.className = "small-meta";
    const kb = typeof photo.bytes === "number" ? `${(photo.bytes / 1024).toFixed(1)} KB` : "";
    meta.textContent = `expires ${fmtDate(photo.expires_at)}${kb ? ` · ${kb}` : ""}`;
    li.append(title, meta);
    if (typeof photo.url === "string" && photo.url) {
      const open = document.createElement("a");
      open.className = "auth-link";
      open.href = photo.url;
      open.textContent = "Download";
      open.setAttribute("download", "");
      li.append(open);
    }
    if (typeof photo.id === "string" && photo.id) {
      const del = document.createElement("button");
      del.type = "button";
      del.className = "danger-btn";
      del.textContent = "Delete";
      del.setAttribute("aria-label", "Delete retained photo");
      del.addEventListener("click", async () => {
        try {
          await call("DELETE", `/api/photos/${encodeURIComponent(photo.id)}`);
          status("Photo deleted.");
          await loadPhotos();
        } catch (error) {
          status(apiError(error, "Could not delete that photo."));
        }
      });
      li.append(del);
    }
    return li;
  }

  // --- operator ------------------------------------------------------------

  async function loadOperator() {
    if (!state.user || state.user.role !== "operator") return;
    const panel = $("operator-panel");
    if (panel) panel.hidden = false;
    await Promise.all([loadInvites(), loadUsers(), loadUsage(), loadAudit()]);
  }

  async function loadInvites() {
    const list = $("invite-list");
    if (!list) return;
    list.replaceChildren();
    try {
      const data = await call("GET", "/api/operator/invites");
      const invites = Array.isArray(data.invites) ? data.invites : [];
      for (const inv of invites) {
        const li = document.createElement("li");
        const title = document.createElement("strong");
        title.textContent = `${inv.email} · ${inv.role} · ${inv.status}`;
        const meta = document.createElement("p");
        meta.className = "small-meta";
        meta.textContent = `created ${fmtDate(inv.created_at)} · expires ${fmtDate(inv.expires_at)}`;
        li.append(title, meta);
        if (inv.status === "active") {
          const revoke = document.createElement("button");
          revoke.type = "button";
          revoke.className = "secondary-btn";
          revoke.textContent = "Revoke";
          revoke.addEventListener("click", async () => {
            try {
              await call("POST", `/api/operator/invites/${encodeURIComponent(inv.id)}/revoke`, {});
              status(`Invite for ${inv.email} revoked.`);
              await loadInvites();
            } catch (error) {
              status(apiError(error, "Could not revoke that invite."));
            }
          });
          li.append(revoke);
        }
        list.append(li);
      }
    } catch (error) {
      status(apiError(error, "Could not load invites."));
    }
  }

  async function doCreateInvite(event) {
    event.preventDefault();
    const email = $("invite-email").value.trim();
    const role = $("invite-role").value;
    if (!email) {
      status("Enter the recipient's email.");
      return;
    }
    try {
      const data = await call("POST", "/api/operator/invites", { email, role });
      const out = $("invite-output");
      if (out) {
        out.replaceChildren();
        const warn = document.createElement("p");
        warn.className = "small-meta";
        warn.textContent = "One-time link — deliver it over a private channel, then clear this line:";
        const link = document.createElement("p");
        link.className = "small-meta";
        const code = document.createElement("code");
        code.textContent = typeof data.invite_url === "string" ? data.invite_url : "";
        link.append(code);
        out.append(warn, link);
      }
      const form = $("invite-create-form");
      if (form) form.reset();
      status(`Invite created for ${email}.`);
      await loadInvites();
    } catch (error) {
      status(apiError(error, "Could not create that invite."));
    }
  }

  async function loadUsers() {
    const list = $("user-list");
    if (!list) return;
    list.replaceChildren();
    try {
      const data = await call("GET", "/api/operator/users");
      const users = Array.isArray(data.users) ? data.users : [];
      for (const user of users) {
        const li = document.createElement("li");
        const title = document.createElement("strong");
        title.textContent = `${user.email} · ${user.role} · ${user.status}`;
        const meta = document.createElement("p");
        meta.className = "small-meta";
        meta.textContent = `created ${fmtDate(user.created_at)} · last login ${fmtDate(user.last_login_at)}`;
        li.append(title, meta);
        list.append(li);
      }
    } catch (error) {
      status(apiError(error, "Could not load users."));
    }
  }

  async function loadUsage() {
    const wrap = $("usage-wrap");
    if (!wrap) return;
    wrap.replaceChildren();
    try {
      const data = await call("GET", "/api/operator/usage");
      const summary = document.createElement("p");
      summary.className = "small-meta";
      summary.textContent = `Inference ${data.inference_paused ? "PAUSED" : "running"} · daily limit ${data.daily_limit_per_user ?? 0} photo estimates per user`;
      wrap.append(summary);
      const pause = document.createElement("button");
      pause.type = "button";
      pause.className = "secondary-btn";
      pause.textContent = data.inference_paused ? "Resume inference" : "Pause inference";
      pause.addEventListener("click", async () => {
        try {
          await call("POST", "/api/operator/pause", { paused: !data.inference_paused });
          status(data.inference_paused ? "Inference resumed." : "Inference paused — photo estimates now answer 503.");
          await loadUsage();
        } catch (error) {
          status(apiError(error, "Could not flip the pause switch."));
        }
      });
      wrap.append(pause);
      const rows = Array.isArray(data.usage) ? data.usage : [];
      const list = document.createElement("ul");
      list.id = "usage-list";
      for (const row of rows) {
        const li = document.createElement("li");
        li.textContent = `${row.day} · ${row.user_id} · ${row.image_estimates} photo estimates`;
        list.append(li);
      }
      wrap.append(list);
      try {
        const statusData = await call("GET", "/api/operator/status");
        const alerts = Array.isArray(statusData.alerts) ? statusData.alerts : [];
        const heading = document.createElement("p");
        heading.className = "small-meta";
        heading.textContent = alerts.length ? "Active alerts:" : "No active alerts.";
        wrap.append(heading);
        for (const alert of alerts) {
          const line = document.createElement("p");
          line.className = "small-meta";
          line.textContent = `[${alert.kind || "notice"}] ${alert.message || ""}`;
          wrap.append(line);
        }
      } catch (_error) {
        // Usage already rendered; alerts are enrichment only.
      }
    } catch (error) {
      wrap.textContent = apiError(error, "Could not load usage.");
    }
  }

  async function loadAudit() {
    const list = $("audit-list");
    if (!list) return;
    list.replaceChildren();
    try {
      const data = await call("GET", "/api/operator/audit?limit=50");
      const entries = Array.isArray(data.audit) ? data.audit : [];
      for (const entry of entries) {
        const li = document.createElement("li");
        li.textContent = `${entry.ts} · ${entry.action} · ${entry.detail || ""}`;
        list.append(li);
      }
    } catch (error) {
      status(apiError(error, "Could not load the audit log."));
    }
  }

  // --- wiring ---------------------------------------------------------------

  function onEstimateSaved() {
    if (state.authenticated) {
      state.historyPage = 1;
      void loadServerHistory();
      void loadAccountSummary();
      void loadAnimals();
      void loadJobs();
      void loadPhotos();
    }
  }

  function bind(id, event, handler) {
    const el = $(id);
    if (!el) return;
    if (event !== "submit") {
      el.addEventListener(event, handler);
      return;
    }
    el.addEventListener(event, async (formEvent) => {
      const buttons = Array.from(el.querySelectorAll('button[type="submit"], input[type="submit"]'));
      const original = buttons.map((button) => ({ button, text: button.textContent }));
      for (const { button } of original) {
        button.disabled = true;
        button.setAttribute("aria-busy", "true");
        if (button.tagName === "BUTTON") button.textContent = "Please wait…";
      }
      el.setAttribute("aria-busy", "true");
      try {
        await handler(formEvent);
      } finally {
        el.removeAttribute("aria-busy");
        for (const { button, text } of original) {
          button.disabled = false;
          button.removeAttribute("aria-busy");
          if (button.tagName === "BUTTON") button.textContent = text;
        }
      }
    });
  }

  function init() {
    bind("login-form", "submit", doLogin);
    bind("invite-form", "submit", doAcceptInvite);
    bind("recovery-form", "submit", doRecoveryRequest);
    bind("recovery-complete-form", "submit", doRecoveryComplete);
    bind("profile-form", "submit", doUpdateProfile);
    bind("password-form", "submit", doChangePassword);
    bind("delete-form", "submit", doDeleteAccount);
    bind("animal-form", "submit", doSaveAnimal);
    bind("scale-form", "submit", doAddScale);
    bind("history-filter-form", "submit", applyHistoryFilters);
    bind("invite-create-form", "submit", doCreateInvite);
    bind("account-export", "click", doExportAccount);
    bind("history-export-server", "click", doExportHistoryCsv);
    bind("animal-new", "click", () => {
      const form = $("animal-form");
      if (form) form.reset();
      $("animal-id").value = "";
      $("animal-form-title").textContent = "Add an animal";
    });
    bind("back-to-account", "click", () => window.aifPages ? window.aifPages.navigate("/animals") : showView("account"));
    document.addEventListener("aif-estimate-saved", onEstimateSaved);
    document.addEventListener("aif-jobs-changed", () => {
      if (state.authenticated) void loadJobs();
    });
    window.addEventListener("hashchange", () => {
      prefillInviteToken();
      prefillRecoveryToken();
    });
    prefillInviteToken();
    prefillRecoveryToken();
    void refreshMe();
  }

  window.aifAccount = {
    refresh: refreshMe,
    isLoggedIn: () => state.authenticated,
    csrf: () => state.csrf,
    loadForRoute,
    viewAnimal: viewAnimalPage,
    viewHistory: loadHistoryDetail,
    viewUploadBatch: loadUploadBatch,
  };

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
