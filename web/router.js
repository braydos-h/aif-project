(() => {
  "use strict";

  const $ = (id) => document.getElementById(id);
  const content = $("page-content");
  const navigation = $("site-navigation");
  if (!content || !navigation) return;

  const routeNames = new Map([
    ["/", "Dashboard"], ["/dashboard", "Dashboard"], ["/login", "Log in"],
    ["/invite", "Accept invitation"], ["/recover", "Recover access"],
    ["/reset-password", "Reset password"], ["/estimate/photo", "Photo estimate"],
    ["/estimate/batch", "Batch upload"], ["/estimate/tape", "Tape estimate"],
    ["/uploads", "Uploads"], ["/history", "History"], ["/animals", "Animals"],
    ["/photos", "Retained photos"], ["/settings/profile", "Profile settings"],
    ["/settings/security", "Security settings"], ["/settings/privacy", "Privacy settings"],
    ["/operator", "Operator"], ["/operator/invites", "Invitations"],
    ["/operator/users", "Users"], ["/operator/activity", "Activity"],
    ["/help", "Help"], ["/privacy", "Privacy"],
  ]);
  const navItems = [
    ["/dashboard", "Home"], ["/estimate/photo", "Photo"],
    ["/estimate/batch", "Batch upload"], ["/estimate/tape", "Tape"],
    ["/uploads", "Uploads"], ["/history", "History"], ["/animals", "Animals"],
    ["/photos", "Photos"], ["/settings/profile", "Settings"], ["/help", "Help"],
  ];
  const authRoutes = new Map([
    ["/login", "login"], ["/invite", "invite"], ["/recover", "recovery"],
    ["/reset-password", "recovery-complete"],
  ]);
  const pageGroups = {
    "/settings/profile": ["overview", "profile"],
    "/settings/security": ["security"],
    "/settings/privacy": ["privacy-data", "delete", "photos"],
    "/animals": ["animals", "animal-form", "scale"],
    "/history": ["history"],
    "/uploads": ["uploads", "upload-batches"],
    "/photos": ["photos"],
  };
  const titles = {
    overview: "Account overview", profile: "Profile", security: "Change password",
    animals: "Your animals", "animal-form": "Add or edit an animal", scale: "Scale measurements",
    history: "Saved history", uploads: "Background estimates", photos: "Retained photos",
    "privacy-data": "Your data", delete: "Delete account", operator: "Operator controls",
  };

  let currentPath = "";
  let authState = null;
  let sessionResolved = false;
  let activeRoutePage = null;
  let operatorGroup = null;
  const routePages = new Map();
  const routeBuilt = new Set();
  const authViews = new Map();
  const accountGroups = new Map();
  const operatorSections = new Map();

  function slug(text) {
    return text.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
  }

  function splitAccountSections() {
    const root = $("account-page-root");
    if (!root || root.dataset.split === "true") return;
    root.dataset.split = "true";
    const operatorPanel = $("operator-panel");
    if (operatorPanel) operatorPanel.remove();
    let group = null;
    let key = "overview";
    for (const node of Array.from(root.childNodes)) {
      if (node.nodeType === Node.ELEMENT_NODE && node.tagName === "H3") {
        key = slug(node.textContent || "section");
        const aliases = {
          "change-password": "security", "your-animals": "animals",
          "add-an-animal": "animal-form", "record-a-scale-weight": "scale",
          "saved-history": "history", "background-jobs": "uploads",
          "retained-photos": "photos", "your-data": "privacy-data",
          "upload-batches": "upload-batches", "delete-account": "delete",
        };
        key = aliases[key] || key;
        group = document.createElement("section");
        group.className = "account-route-group";
        group.dataset.accountGroup = key;
        const heading = document.createElement("h2");
        heading.textContent = titles[key] || node.textContent || "Account";
        group.append(heading);
        root.append(group);
      }
      if (!group) {
        group = document.createElement("section");
        group.className = "account-route-group";
        group.dataset.accountGroup = "overview";
        root.append(group);
      }
      group.append(node);
    }
    if (!operatorPanel) return;
    operatorGroup = document.createElement("section");
    operatorGroup.className = "account-route-group";
    operatorGroup.dataset.accountGroup = "operator";
    const heading = document.createElement("h2");
    heading.textContent = titles.operator;
    operatorGroup.append(heading);
    let section = null;
    const aliases = { "create-an-invite": "invite-create", invites: "invites", accounts: "users", "usage-and-inference": "usage", "recent-activity": "activity" };
    for (const node of Array.from(operatorPanel.childNodes)) {
      if (node.nodeType === Node.ELEMENT_NODE && node.tagName === "H4") {
        const sectionKey = aliases[slug(node.textContent || "")] || slug(node.textContent || "section");
        section = document.createElement("section");
        section.className = "operator-route-section";
        section.dataset.operatorSection = sectionKey;
        const subheading = document.createElement("h3");
        subheading.textContent = node.textContent || "Operator";
        section.append(subheading);
        operatorGroup.append(section);
      }
      if (!section) {
        section = document.createElement("section");
        section.className = "operator-route-section";
        section.dataset.operatorSection = "overview";
        operatorGroup.append(section);
      }
      section.append(node);
    }
  }

  function makeNavLink(path, label) {
    const link = document.createElement("a");
    link.href = path;
    link.textContent = label;
    link.dataset.routeLink = "true";
    return link;
  }

  function renderNav() {
    navigation.replaceChildren();
    const loggedIn = authState && authState.authenticated === true;
    const authRequired = authState && authState.auth_required === true;
    for (const [path, label] of navItems) {
      if ((!authState || authState.session_probe_failed) && path !== "/help") continue;
      if (!loggedIn && authRequired && path !== "/help") continue;
      navigation.append(makeNavLink(path, label));
    }
    if (!loggedIn) navigation.append(makeNavLink("/login", "Log in"));
    if (loggedIn && authState.user && authState.user.role === "operator") {
      navigation.append(makeNavLink("/operator", "Operator"));
    }
    const menu = $("mobile-menu-toggle");
    if (menu) menu.setAttribute("aria-expanded", navigation.classList.contains("open") ? "true" : "false");
  }

  function detailRoute(path) {
    const match = path.match(/^\/(animals|history|uploads)\/([A-Za-z0-9_-]{1,64})$/);
    return match ? { kind: match[1], id: match[2] } : null;
  }

  function privatePath(path) {
    return path === "/dashboard" || path.startsWith("/estimate/") || Boolean(pageGroups[path])
      || path.startsWith("/operator") || Boolean(detailRoute(path));
  }

  function knownPath(path) {
    const pathname = path.split("?")[0];
    return routeNames.has(pathname) || pathname === "/account" || Boolean(detailRoute(pathname));
  }

  function normalizedPath(path) {
    if (path === "/" || path === "/account") return path === "/" ? "/dashboard" : "/settings/profile";
    return path;
  }

  function pageKey(path) {
    if (path.startsWith("/estimate/")) return "/estimate";
    if (path.startsWith("/operator")) return "/operator";
    if (path === "/photos" || path === "/settings/privacy") return "/settings/privacy";
    const detail = detailRoute(path);
    return detail ? `/${detail.kind}/detail` : path;
  }

  function ensureRoutePage(key) {
    if (!routePages.has(key)) {
      const page = document.createElement("section");
      page.className = "route-page";
      page.dataset.routeContainer = key;
      page.hidden = true;
      content.append(page);
      routePages.set(key, page);
    }
    return routePages.get(key);
  }

  function showOnly(page) {
    for (const candidate of routePages.values()) candidate.hidden = candidate !== page;
    page.hidden = false;
    activeRoutePage = page;
  }

  function configureEstimatePage(path, page) {
    const photo = page.querySelector("#photo-estimator-page");
    const tape = page.querySelector("#tape-estimator-page");
    if (photo) photo.hidden = path === "/estimate/tape";
    if (tape) tape.hidden = path !== "/estimate/tape";
    const button = page.querySelector("#estimate-button");
    if (button) button.textContent = path.endsWith("/batch") ? "Estimate all photos" : "Estimate photo";
    const input = page.querySelector("#image-input");
    if (input) input.multiple = path.endsWith("/batch");
    const background = page.querySelector("#background-button");
    if (background) background.hidden = !path.endsWith("/batch");
  }

  function updateOperatorPage(page, path) {
    const authorized = Boolean(authState && authState.authenticated && authState.user && authState.user.role === "operator");
    const message = page.querySelector('[data-operator-required="true"]');
    if (message) message.hidden = authorized;
    const selected = path === "/operator/invites" ? ["invite-create", "invites"]
      : path === "/operator/users" ? ["users"]
        : path === "/operator/activity" ? ["activity"]
          : ["overview", "usage"];
    for (const [key, section] of operatorSections) {
      if (authorized) {
        if (section.parentElement !== page) page.append(section);
        section.hidden = !selected.includes(key);
      } else {
        section.remove();
      }
    }
  }

  function addNotFound(path, page) {
    page.replaceChildren();
    const heading = document.createElement("h2");
    heading.textContent = "Page not found";
    const note = document.createElement("p");
    note.className = "note";
    note.textContent = `There is no page at ${path}.`;
    page.append(heading, note, makeNavLink("/dashboard", "Go to home"));
  }

  function buildRoutePage(path) {
    const key = pageKey(path);
    const page = ensureRoutePage(key);
    if (routeBuilt.has(key)) {
      if (path.startsWith("/estimate/")) configureEstimatePage(path, page);
      if (path.startsWith("/operator")) updateOperatorPage(page, path);
      if (path === "/photos" || path === "/settings/privacy") {
        for (const groupKey of ["privacy-data", "delete", "photos"]) {
          const group = accountGroups.get(groupKey);
          if (group) group.hidden = path === "/photos" && groupKey !== "photos";
        }
      }
      return page;
    }
    routeBuilt.add(key);

    const authName = authRoutes.get(path);
    if (authName) {
      const view = authViews.get(authName);
      if (view) { view.hidden = false; page.append(view); }
      return page;
    }

    const detail = detailRoute(path);
    if (detail && detail.kind === "animals") {
      const view = authViews.get("animal-detail");
      if (view) { view.hidden = false; page.append(view); }
      return page;
    }
    if (detail && detail.kind === "history") {
      const heading = document.createElement("h2");
      heading.textContent = "Estimate details";
      const body = document.createElement("div");
      body.id = "history-detail-body";
      body.setAttribute("aria-live", "polite");
      page.append(heading, body);
      return page;
    }
    if (detail && detail.kind === "uploads") {
      const heading = document.createElement("h2");
      heading.textContent = "Upload or job details";
      const body = document.createElement("div");
      body.id = "upload-batch-detail-body";
      body.setAttribute("aria-live", "polite");
      page.append(heading, body);
      return page;
    }

    if (path === "/dashboard") {
      page.append($("dashboard-page"));
      return page;
    }
    if (path.startsWith("/estimate/")) {
      page.append($("photo-estimator-page"), $("estimate-profile-page"), $("tape-estimator-page"));
      configureEstimatePage(path, page);
      return page;
    }
    if (path === "/help") {
      const intro = document.createElement("p");
      intro.className = "note";
      intro.textContent = "For a useful photo estimate, photograph one animal from the side in daylight. For tape estimates, measure girth just behind the front legs and body length from chest to tail head.";
      page.append(intro, $("photo-tips-page"), $("demo-page"));
      return page;
    }
    if (path === "/privacy") {
      const heading = document.createElement("h2");
      heading.textContent = "Your privacy";
      const note = document.createElement("p");
      note.className = "note";
      note.textContent = "Accounts are invitation-only. History and animal records belong to the signed-in account. Synchronous photos are processed in memory; background requests are temporarily stored in the private job queue and cleared when finished. Optional retained photos are private and expire automatically. Export or delete your account data in Privacy settings.";
      page.append(heading, note, makeNavLink("/settings/privacy", "Privacy settings"));
      return page;
    }
    if (path.startsWith("/operator")) {
      const subnav = document.createElement("nav");
      subnav.setAttribute("aria-label", "Operator pages");
      for (const [route, label] of [["/operator", "Overview"], ["/operator/invites", "Invitations"], ["/operator/users", "Users"], ["/operator/activity", "Activity"]]) {
        subnav.append(makeNavLink(route, label));
      }
      const message = document.createElement("p");
      message.className = "note";
      message.dataset.operatorRequired = "true";
      message.textContent = "Operator access is required for these controls.";
      page.append(subnav, message);
      updateOperatorPage(page, path);
      return page;
    }

    let groups = pageGroups[path];
    if (path === "/photos") groups = ["privacy-data", "delete", "photos"];
    if (path === "/settings/privacy") groups = ["privacy-data", "delete", "photos"];
    if (groups) {
      for (const groupKey of groups) {
        const group = accountGroups.get(groupKey);
        if (group) page.append(group);
      }
      if (path === "/history") {
        const localHistory = $("session-history-page");
        if (localHistory) page.append(localHistory);
      }
      if (path === "/photos") {
        for (const groupKey of ["privacy-data", "delete"]) {
          const group = accountGroups.get(groupKey);
          if (group) group.hidden = true;
        }
      }
      return page;
    }
    addNotFound(path, page);
    return page;
  }

  function showMessage(key, messageText, role, withRetry = false) {
    const page = ensureRoutePage(key);
    page.replaceChildren();
    const message = document.createElement("p");
    message.className = "note";
    message.setAttribute("role", role);
    message.textContent = messageText;
    page.append(message);
    if (withRetry) {
      const retry = document.createElement("button");
      retry.type = "button";
      retry.textContent = "Retry connection";
      retry.addEventListener("click", () => { if (window.aifAccount) void window.aifAccount.refresh(); });
      page.append(retry);
    }
    showOnly(page);
    return page;
  }

  function render(path, { replace = false } = {}) {
    path = normalizedPath(path);
    const requestedPathname = path.split("?")[0];
    document.body.dataset.route = requestedPathname;
    if (!knownPath(path)) {
      const page = ensureRoutePage("not-found");
      addNotFound(path, page);
      showOnly(page);
      document.title = "Page not found · Cow Weight Estimator";
      currentPath = requestedPathname;
      return;
    }
    if (privatePath(requestedPathname) && !sessionResolved) {
      showMessage("session-check", "Checking your session before opening private records…", "status");
      currentPath = requestedPathname;
      document.title = `${routeNames.get(requestedPathname) || "Cow Weight Estimator"} · Cow Weight Estimator`;
      return;
    }
    if (privatePath(requestedPathname) && authState && authState.session_probe_failed) {
      showMessage("connection-error", "The service could not verify your session, so private records are hidden. Restore the connection and retry.", "alert", true);
      currentPath = requestedPathname;
      document.title = `${routeNames.get(requestedPathname) || "Cow Weight Estimator"} · Cow Weight Estimator`;
      return;
    }
    if (authState && authState.authenticated !== true && authState.auth_required === true && privatePath(requestedPathname)) {
      const next = encodeURIComponent(path);
      path = `/login?next=${next}`;
      if (replace) window.history.replaceState(null, "", path);
      else window.history.pushState(null, "", path);
    }
    const pathname = path.split("?")[0];
    document.body.dataset.route = pathname;
    const page = buildRoutePage(pathname);
    showOnly(page);
    const detail = detailRoute(pathname);
    const detailLabel = detail ? ({ animals: "Animal details", history: "Estimate details", uploads: "Upload or job details" })[detail.kind] : null;
    const label = routeNames.get(pathname) || detailLabel || (pathname === "/account" ? "Profile settings" : "Cow Weight Estimator");
    document.title = `${label} · Cow Weight Estimator`;
    currentPath = pathname;
    for (const link of navigation.querySelectorAll("a")) {
      if (link.getAttribute("href") === pathname) link.setAttribute("aria-current", "page");
      else link.removeAttribute("aria-current");
    }
    const menu = $("mobile-menu-toggle");
    if (menu) {
      menu.setAttribute("aria-expanded", "false");
      navigation.classList.remove("open");
    }
    if (window.aifAccount && typeof window.aifAccount.loadForRoute === "function") {
      void window.aifAccount.loadForRoute(pathname);
    }
    document.dispatchEvent(new CustomEvent("aif-page-route", { detail: { path: pathname } }));
  }

  function navigate(path, options = {}) {
    const url = new URL(path, window.location.origin);
    if (url.origin !== window.location.origin || !knownPath(url.pathname)) return false;
    if (url.pathname === window.location.pathname && url.search === window.location.search) {
      render(url.pathname + url.search, options);
      return true;
    }
    const href = `${url.pathname}${url.search}${options.preserveHash === false ? "" : window.location.hash}`;
    if (options.replace) window.history.replaceState(null, "", href);
    else window.history.pushState(null, "", href);
    render(url.pathname + url.search, options);
    window.scrollTo(0, 0);
    const heading = activeRoutePage && activeRoutePage.querySelector("h2");
    if (heading) {
      heading.setAttribute("tabindex", "-1");
      heading.focus({ preventScroll: true });
    }
    return true;
  }

  function setAuthState(state) {
    authState = state || { authenticated: false, auth_required: false };
    const firstProbe = !sessionResolved;
    sessionResolved = true;
    renderNav();
    if (firstProbe && currentPath) {
      render(currentPath + window.location.search, { replace: true });
      return;
    }
    if (currentPath && !authState.authenticated && authState.auth_required && privatePath(currentPath)) {
      navigate(`/login?next=${encodeURIComponent(currentPath)}`, { replace: true });
    } else if (currentPath) {
      render(currentPath + window.location.search, { replace: true });
    }
    if (currentPath === "/dashboard") {
      const welcome = $("dashboard-welcome");
      if (welcome && authState.authenticated && authState.user) {
        welcome.textContent = `Welcome, ${authState.user.display_name || authState.user.email}. Your estimates and animal records are private to your account.`;
      }
    }
  }

  splitAccountSections();
  for (const view of document.querySelectorAll(".auth-view[data-view]")) authViews.set(view.dataset.view, view);
  for (const group of document.querySelectorAll("[data-account-group]")) accountGroups.set(group.dataset.accountGroup, group);
  if (operatorGroup) {
    for (const section of operatorGroup.querySelectorAll("[data-operator-section]")) operatorSections.set(section.dataset.operatorSection, section);
  }
  renderNav();

  for (const input of document.querySelectorAll('input[type="password"]')) {
    if (!input.id || input.dataset.visibilityToggle === "true") continue;
    input.dataset.visibilityToggle = "true";
    const toggle = document.createElement("button");
    toggle.type = "button";
    toggle.className = "password-toggle secondary-btn";
    toggle.setAttribute("aria-controls", input.id);
    toggle.setAttribute("aria-pressed", "false");
    toggle.textContent = "Show password";
    toggle.addEventListener("click", () => {
      const showing = input.type === "password";
      input.type = showing ? "text" : "password";
      toggle.textContent = showing ? "Hide password" : "Show password";
      toggle.setAttribute("aria-pressed", showing ? "true" : "false");
    });
    const label = input.closest("label");
    (label || input).insertAdjacentElement("afterend", toggle);
  }

  const menu = $("mobile-menu-toggle");
  if (menu) menu.addEventListener("click", () => {
    const open = !navigation.classList.contains("open");
    navigation.classList.toggle("open", open);
    menu.setAttribute("aria-expanded", open ? "true" : "false");
  });
  document.addEventListener("click", (event) => {
    const link = event.target.closest("a[href]");
    if (!link || event.defaultPrevented || event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
    const url = new URL(link.href, window.location.href);
    if (url.origin !== window.location.origin || !knownPath(url.pathname)) return;
    event.preventDefault();
    navigate(`${url.pathname}${url.search}`);
  });
  window.addEventListener("popstate", () => render(window.location.pathname + window.location.search, { replace: true }));
  window.addEventListener("hashchange", () => {
    if (window.location.hash.startsWith("#invite=")) navigate("/invite", { replace: true });
    else if (window.location.hash.startsWith("#recovery=")) navigate("/reset-password", { replace: true });
    else if (window.location.hash === "#account") navigate("/settings/profile", { replace: true });
    else if (window.location.hash === "#login") navigate("/login", { replace: true });
  });

  const hash = window.location.hash;
  let initialPath = window.location.pathname + window.location.search;
  if (hash.startsWith("#invite=")) initialPath = "/invite";
  else if (hash.startsWith("#recovery=")) initialPath = "/reset-password";
  else if (hash === "#login") initialPath = "/login";
  else if (hash === "#account") initialPath = "/settings/profile";
  if (initialPath === "/") {
    window.history.replaceState(null, "", `/dashboard${hash}`);
    initialPath = "/dashboard";
  }
  render(initialPath, { replace: true });

  window.aifPages = { navigate, setAuthState, currentPath: () => currentPath };
})();
