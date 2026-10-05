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
  };

  function status(message) {
    const el = $("account-status");
    if (el) el.textContent = message;
  }

  function showView(name) {
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

  function describeSource(item) {
    if (item.source === "scale") return "scale measurement";
    if (item.placeholder || item.source === "local_fallback") return "offline placeholder";
    if (item.source === "tape_measure") return "tape measure";
    return "AI estimate";
  }

  // --- session / navigation -------------------------------------------

  async function refreshMe() {
    let data = {};
    try {
      data = await call("GET", "/api/me");
    } catch (_error) {
      data = { authenticated: false, auth_required: false };
    }
    state.authenticated = data.authenticated === true;
    state.authRequired = data.auth_required === true;
    state.production = data.production === true;
    state.user = (data.user && typeof data.user === "object") ? data.user : null;
    state.csrf = state.user && typeof state.user.csrf_token === "string" ? state.user.csrf_token : null;
    renderAuthBar();
    if (window.aifEstimator && typeof window.aifEstimator.setAuthMode === "function") {
      window.aifEstimator.setAuthMode(state.authenticated, state.authRequired);
    }
    if (state.authenticated) {
      await Promise.all([loadAnimals(), loadServerHistory(), loadAccountSummary()]);
      if (state.user && state.user.role === "operator") await loadOperator();
    } else {
      clearPrivateUI();
      if (state.authRequired) showView("login");
    }
    return state;
  }

  function renderAuthBar() {
    const bar = $("auth-bar");
    if (!bar) return;
    bar.replaceChildren();
    const home = document.createElement("a");
    home.className = "auth-link";
    home.href = "#";
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
      login.href = "#login";
      login.textContent = "Log in";
      login.dataset.nav = "login";
      login.addEventListener("click", (event) => {
        event.preventDefault();
        showView("login");
      });
      const invite = document.createElement("a");
      invite.className = "auth-link";
      invite.href = "#invite";
      invite.textContent = "Accept invite";
      invite.dataset.nav = "invite";
      invite.addEventListener("click", (event) => {
        event.preventDefault();
        showView("invite");
      });
      const recovery = document.createElement("a");
      recovery.className = "auth-link";
      recovery.href = "#recovery";
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
      account.href = "#account";
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
    for (const id of ["server-history-list", "animal-list", "invite-list", "user-list", "usage-list", "audit-list"]) {
      const el = $(id);
      if (el) el.replaceChildren();
    }
    const summary = $("account-summary");
    if (summary) summary.textContent = "";
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
      showView("estimator");
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
    try {
      window.history.replaceState(null, "", window.location.pathname);
    } catch (_error) {
      // Hash cleanup is cosmetic.
    }
    await refreshMe();
    showView("estimator");
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
      el.replaceChildren();
      const lines = [
        `Email: ${data.email || ""}`,
        `Display name: ${data.display_name || ""}`,
        `Role: ${data.role || ""}`,
        `Animals: ${data.animals ?? 0} · Saved estimates: ${data.estimates ?? 0}`,
        `Today: ${data.today_image_estimates ?? 0} of ${data.daily_limit ?? 0} photo estimates used`,
        `Photos: ${(typeof data.photo_policy === "string" && data.photo_policy) || "transient-only"}`,
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
    if (item.animal_breed) bits.push(item.animal_breed);
    if (item.model) bits.push(`model ${item.model}`);
    if (item.estimator_version) bits.push(`estimator ${item.estimator_version}`);
    if (item.prompt_version) bits.push(`prompt ${item.prompt_version}`);
    if (typeof item.disclaimer === "string" && item.disclaimer) bits.push(item.disclaimer);
    meta.textContent = bits.join(" · ");
    li.append(top, meta);
    if (item.id) {
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
    link.href = "/api/history/export?format=csv";
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
    showView("account");
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
    state.currentAnimal = id;
    state.animalHistoryPage = 1;
    await loadAnimalDetail();
    showView("animal-detail");
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
    }
  }

  function bind(id, event, handler) {
    const el = $(id);
    if (el) el.addEventListener(event, handler);
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
    bind("back-to-account", "click", () => showView("account"));
    document.addEventListener("aif-estimate-saved", onEstimateSaved);
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
  };

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
