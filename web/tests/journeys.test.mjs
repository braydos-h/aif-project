import assert from "node:assert/strict";
import { spawn, spawnSync } from "node:child_process";
import { mkdtemp, rm } from "node:fs/promises";
import { existsSync } from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { once } from "node:events";
import { fileURLToPath } from "node:url";
import test from "node:test";
import { chromium } from "playwright";

const repo = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const binary = path.resolve(process.env.AIF_BACKEND_BIN || path.join(repo, "backend/target/release/aif-backend"));

async function freePort() {
  const server = net.createServer();
  await new Promise((resolve, reject) => server.once("error", reject).listen(0, "127.0.0.1", resolve));
  const { port } = server.address();
  await new Promise((resolve) => server.close(resolve));
  return port;
}

async function waitFor(check, message, timeout = 30000) {
  const until = Date.now() + timeout;
  while (Date.now() < until) {
    if (await check()) return;
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  throw new Error(`Timed out waiting for ${message}`);
}

function createInvite(dataDir, email) {
  const env = {
    ...process.env,
    AIF_AI_BACKEND: "none",
    AIF_DATA_DIR: dataDir,
    AIF_REQUIRE_AUTH: "1",
    AIF_PRODUCTION: "0",
  };
  const result = spawnSync(binary, ["--create-invite", email, "--role", "user"], {
    cwd: repo,
    env,
    encoding: "utf8",
  });
  if (result.status !== 0) throw new Error(`Could not create browser-test invite: ${result.stdout}\n${result.stderr}`);
  const token = result.stdout.match(/#invite=([^\s]+)/)?.[1];
  if (!token) throw new Error("Invite CLI did not print an invitation token.");
  return token;
}

test("invited user can navigate, submit a photo batch, review records, and log out", { timeout: 180000 }, async () => {
  const dataDir = await mkdtemp(path.join(os.tmpdir(), "aif-browser-"));
  const port = await freePort();
  const origin = `http://127.0.0.1:${port}`;
  const token = createInvite(dataDir, "browser-user@example.com");
  const env = {
    ...process.env,
    AIF_AI_BACKEND: "none",
    AIF_DATA_DIR: dataDir,
    AIF_REQUIRE_AUTH: "1",
    AIF_PRODUCTION: "0",
    AIF_COOKIE_SECURE: "0",
    AIF_JOB_WORKER: "1",
  };
  const server = spawn(binary, ["--host", "127.0.0.1", "--port", String(port)], {
    cwd: repo,
    env,
    stdio: "ignore",
  });
  let browser;
  try {
    await waitFor(async () => {
      try { return (await fetch(`${origin}/health`)).ok; } catch (_error) { return false; }
    }, "backend health endpoint");
    const systemChromium = process.env.CHROMIUM_BIN || (existsSync("/usr/bin/chromium") ? "/usr/bin/chromium" : undefined);
    browser = await chromium.launch({ headless: true, args: ["--no-sandbox"], executablePath: systemChromium });
    const context = await browser.newContext({ viewport: { width: 1280, height: 900 } });
    const page = await context.newPage();
    const consoleErrors = [];
    page.on("pageerror", (error) => consoleErrors.push(error.message));

    await page.goto(`${origin}/invite#invite=${encodeURIComponent(token)}`);
    await page.waitForTimeout(1000);
    const initialRoute = await page.evaluate(() => ({
      title: document.title,
      href: window.location.href.replace(/#invite=.*/, "#invite=[redacted]"),
      route: window.aifPages?.currentPath(),
      body: document.body.innerText.slice(0, 250),
    }));
    assert.equal(initialRoute.route, "/invite", `invite route failed to mount: ${JSON.stringify(initialRoute)}; page errors: ${consoleErrors.join("; ")}`);
    await page.waitForFunction(() => document.querySelector("#invite-token")?.value.length > 0, null, { timeout: 5000 }).catch(async () => {
      const debug = await page.evaluate(() => ({
        route: window.aifPages?.currentPath(),
        hashPresent: Boolean(window.location.hash),
        tokenFieldPresent: Boolean(document.querySelector("#invite-token")),
        tokenLength: document.querySelector("#invite-token")?.value.length,
        pageText: document.querySelector("#page-content")?.textContent.slice(0, 300),
      }));
      throw new Error(`Invitation route did not prefill its token: ${JSON.stringify(debug)}; page errors: ${consoleErrors.join("; ")}`);
    });
    await page.locator("#invite-name").fill("Browser User");
    await page.locator("#invite-password").fill("browser test password");
    await page.getByRole("button", { name: "Activate account" }).click();
    await page.waitForFunction(() => window.aifAccount?.isLoggedIn() === true);
    await page.waitForFunction(() => window.aifPages?.currentPath() === "/dashboard");
    assert.match(await page.title(), /Dashboard/);
    assert.equal(await page.locator("#site-navigation a[href='/operator']").count(), 0, "ordinary user should not see operator navigation");

    await page.goto(`${origin}/animals`);
    await page.getByRole("button", { name: "Add an animal" }).click();
    await page.getByLabel("Name", { exact: true }).fill("Daisy");
    await page.locator("#animal-breed").fill("Angus");
    await page.getByRole("button", { name: "Save animal" }).click();
    await page.waitForFunction(() => document.querySelector("#animal-list")?.textContent.includes("Daisy"));
    const animals = await page.evaluate(async () => (await (await fetch("/api/animals")).json()).animals);
    const animalId = animals.find((animal) => animal.name === "Daisy")?.id;
    assert.ok(animalId, "animal form should save an owned animal");

    await page.goto(`${origin}/estimate/batch`);
    await page.waitForFunction(() => document.body.dataset.route === "/estimate/batch");
    assert.equal(await page.locator("#image-input").evaluate((input) => input.multiple), true);
    await page.locator("#image-input").setInputFiles([
      path.join(repo, "cows/cow 2.jpg"),
      path.join(repo, "cows/cow 3.jpg"),
    ]);
    await page.waitForFunction(() => document.querySelectorAll("#file-list select").length === 2);
    for (const select of await page.locator("#file-list select").all()) await select.selectOption(animalId);
    await page.getByRole("button", { name: "Estimate in background" }).click();
    await page.waitForURL(/\/uploads\/[A-Za-z0-9_-]+$/);
    const batchId = new URL(page.url()).pathname.split("/").at(-1);
    await page.waitForFunction(async (id) => {
      const response = await fetch(`/api/upload-batches/${encodeURIComponent(id)}`);
      if (!response.ok) return false;
      const batch = await response.json();
      return batch.items?.length === 2 && batch.items.every((item) => item.status === "success");
    }, batchId, { timeout: 45000 });
    await page.waitForFunction(() => {
      const rows = Array.from(document.querySelectorAll("#upload-batch-detail-body li"));
      const body = document.querySelector("#upload-batch-detail-body")?.textContent || "";
      return rows.length === 2 && rows.every((row) => row.textContent.includes("succeeded"))
        && body.includes("cow 2.jpg") && body.includes("cow 3.jpg")
        && document.querySelectorAll("#upload-batch-detail-body a[href^='/history/']").length === 2;
    });
    const batchText = await page.locator("#upload-batch-detail-body").innerText();
    assert.match(batchText, /cow 2\.jpg/);
    assert.match(batchText, /cow 3\.jpg/);
    assert.equal((await page.locator("#upload-batch-detail-body a[href^='/history/']").count()), 2);

    await page.goto(`${origin}/history`);
    await page.waitForFunction(() => document.querySelector("#server-history-list")?.textContent.includes("Daisy"));
    assert.ok((await page.locator("#server-history-list li").count()) >= 2);
    await page.goto(`${origin}/animals/${encodeURIComponent(animalId)}`);
    await page.waitForFunction(() => document.querySelector("#animal-detail-body")?.textContent.includes("Daisy"));
    await page.waitForFunction(() => document.querySelector("#animal-detail-body")?.textContent.includes("2 records"));

    // Direct loading and refresh work on all explicit routes. The saved ids
    // exercise each supported detail URL as well.
    const historyId = await page.evaluate(async (id) => {
      const batch = await (await fetch(`/api/upload-batches/${encodeURIComponent(id)}`)).json();
      return batch.items[0].history_id;
    }, batchId);
    const routes = [
      "/login", "/invite", "/recover", "/reset-password", "/dashboard",
      "/estimate/photo", "/estimate/batch", "/estimate/tape", "/uploads",
      `/uploads/${batchId}`, "/history", `/history/${historyId}`, "/animals",
      `/animals/${animalId}`, "/photos", "/settings/profile", "/settings/security",
      "/settings/privacy", "/operator", "/operator/invites", "/operator/users",
      "/operator/activity", "/help", "/privacy",
    ];
    for (const route of routes) {
      await page.goto(`${origin}${route}`);
      await page.waitForFunction((pathname) => document.body.dataset.route === pathname, route);
      assert.doesNotMatch(await page.locator("#page-content").innerText(), /Page not found/);
      await page.reload();
      await page.waitForFunction((pathname) => document.body.dataset.route === pathname, route);
    }
    await page.goto(`${origin}/operator/invites`);
    await page.waitForFunction(() => document.body.dataset.route === "/operator/invites");
    assert.equal(await page.locator("#invite-create-form").count(), 0, "ordinary user must not receive operator controls");

    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(`${origin}/dashboard`);
    await page.getByRole("button", { name: "Menu" }).click();
    assert.equal(await page.locator("#site-navigation").getAttribute("aria-label"), "Main navigation");
    assert.equal(await page.locator("#mobile-menu-toggle").getAttribute("aria-expanded"), "true");
    assert.equal(await page.locator("#site-navigation").evaluate((nav) => getComputedStyle(nav).display), "flex");

    await page.getByRole("button", { name: "Log out" }).click();
    await page.waitForFunction(() => window.aifAccount?.isLoggedIn() === false);
    await page.goto(`${origin}/history`);
    await page.waitForURL(/\/login\?next=/);
    await page.locator("#login-email").fill("browser-user@example.com");
    await page.locator("#login-password").fill("browser test password");
    await page.getByRole("button", { name: "Log in" }).click();
    await page.waitForFunction(() => window.aifAccount?.isLoggedIn() === true);
    await page.waitForFunction(() => window.aifPages?.currentPath() === "/history");
    await page.getByRole("button", { name: "Log out" }).click();
    await page.waitForFunction(() => window.aifAccount?.isLoggedIn() === false);
    assert.deepEqual(consoleErrors, [], `browser page errors: ${consoleErrors.join("; ")}`);
    await context.close();
  } finally {
    if (browser) await browser.close();
    if (server.exitCode === null && server.signalCode === null) {
      const stopped = once(server, "exit");
      server.kill("SIGTERM");
      await Promise.race([stopped, new Promise((resolve) => setTimeout(resolve, 5000))]);
    }
    await rm(dataDir, { recursive: true, force: true });
  }
});
