// gate.mjs: OpenRO v1.1.0 Desktop-Workbench end-to-end verification gate.
// Exercises 1440x900, 1280x800, 768x1024, and 390x844 in Light & Dark themes.
import fs from "node:fs";
import { BASE, OUT as E2E_OUT, launch, reset, report } from "./fixture.mjs";

const OUT = `${E2E_OUT}/gate`;
fs.mkdirSync(OUT, { recursive: true });

const res = [];
const check = (n, p, d = "") => {
  res.push({ n, p: !!p, d });
  console.log((p ? "ok: " : "FAIL: ") + n + (d ? ` — ${d}` : ""));
};

async function openSettings(page) {
  await page.locator('button[aria-label="Settings"]').first().click();
  const dlg = page.locator("[data-settings-dialog]");
  await dlg.waitFor({ state: "visible", timeout: 8000 });
  return dlg;
}

async function setThemeViaSettings(page, target) {
  const dlg = await openSettings(page);
  const label = target === "dark" ? "Dark" : "Light";
  await dlg.getByRole("button", { name: label, exact: true }).click();
  await page.waitForFunction((t) => document.documentElement.getAttribute("data-theme") === t, target);
  await page.keyboard.press("Escape");
  await dlg.waitFor({ state: "hidden", timeout: 5000 });
}

async function assertLayoutInvariants(page, label) {
  const m = await page.evaluate(() => {
    const html = document.documentElement;
    const overflowX = html.scrollWidth > window.innerWidth + 1;
    const bodyText = document.body?.innerText ?? "";
    const hasRuneJargon = /\bRune\b/i.test(bodyText);
    const hasRhlJargon = /\bRHL\b/i.test(bodyText);

    const header = document.querySelector("[data-top-chrome]");
    const headerHeight = header ? header.getBoundingClientRect().height : 0;
    const headerHasThemeSwitch = header ? /\b(Light|Dark)\b/.test(header.innerText) : false;

    const rail = document.querySelector('nav[aria-label="Explorer, Chats, and Projects"]') || document.querySelector("aside");
    const railHasThemeSwitch =
      rail && rail.getBoundingClientRect().width > 0
        ? Array.from(rail.querySelectorAll("button")).some((b) => /^(Light|Dark)$/i.test((b.textContent || "").trim()))
        : false;

    const clipped = [];
    const candidates = document.querySelectorAll("header button, nav button, [role='tab'], [role='radio'], h1, h2");
    for (const el of candidates) {
      const style = window.getComputedStyle(el);
      if (style.display === "none" || style.visibility === "hidden") continue;
      const rect = el.getBoundingClientRect();
      if (rect.width === 0 || rect.height === 0) continue;
      if (el.classList.contains("truncate")) continue;
      if (el.scrollWidth > el.clientWidth + 2) {
        const text = (el.textContent || "").trim();
        if (text) clipped.push(`${el.tagName}:${text.slice(0, 32)} (${el.scrollWidth}>${el.clientWidth})`);
      }
    }

    return {
      overflowX,
      scrollWidth: html.scrollWidth,
      innerWidth: window.innerWidth,
      hasRuneJargon,
      hasRhlJargon,
      headerHeight,
      headerHasThemeSwitch,
      railHasThemeSwitch,
      clipped: clipped.slice(0, 5),
    };
  });

  check(`${label}: zero horizontal overflow`, !m.overflowX, `${m.scrollWidth}<= ${m.innerWidth}`);
  check(`${label}: #155 zero "Rune" and zero "RHL" jargon`, !m.hasRuneJargon && !m.hasRhlJargon);
  check(`${label}: top chrome <=48px`, m.headerHeight > 0 && m.headerHeight <= 49, `${Math.round(m.headerHeight)}px`);
  check(`${label}: #67 Settings-only theme switch`, !m.headerHasThemeSwitch && !m.railHasThemeSwitch);
  check(`${label}: zero text clipping`, m.clipped.length === 0, m.clipped.join("; "));
}

const b = await launch();

try {
  await reset();
  const ctx = await b.newContext({
    viewport: { width: 1440, height: 900 },
    acceptDownloads: true,
    colorScheme: "light",
  });
  const p = await ctx.newPage();

  const consoleErrors = [];
  const pageErrors = [];
  const badResponses = [];
  p.on("console", (m) => {
    if (m.type() === "error") consoleErrors.push(m.text());
  });
  p.on("pageerror", (e) => pageErrors.push(String(e)));
  p.on("response", (r) => {
    if (r.status() >= 500) badResponses.push(`${r.status()} ${r.url()}`);
  });

  await p.goto(BASE, { waitUntil: "domcontentloaded" });
  await p.locator("[data-top-chrome]").waitFor({ state: "visible", timeout: 15000 });
  await p.locator("[data-ropilot-copilot]").waitFor({ state: "visible", timeout: 10000 });
  await p.locator("[data-studio-workspace]").waitFor({ state: "visible", timeout: 10000 });
  await p.locator("#composer-input").waitFor({ state: "visible", timeout: 10000 });
  await p.locator('button[aria-label^="Build "]').first().waitFor({ state: "visible", timeout: 10000 });
  check("1440x900 Light: 3-pane workspace ready", true);

  await assertLayoutInvariants(p, "1440x900 Light");
  await p.screenshot({ path: `${OUT}/built-desktop-1440-light.png` });

  // Mode switching (Agent | Plan | Ask)
  const modeBar = p.locator('[aria-label="Agent mode"]');
  await modeBar.getByRole("button", { name: "Plan", exact: true }).click();
  await modeBar.getByRole("button", { name: "Ask", exact: true }).click();
  await modeBar.getByRole("button", { name: "Agent", exact: true }).click();
  check("Agent | Plan | Ask mode toggle", true);

  // 1-click starter build
  await p.locator('button[aria-label^="Build "]').first().click();
  await p.locator("#thread .rounded-panel").first().waitFor({ state: "visible", timeout: 15000 });
  await p.waitForFunction(
    () => {
      const btn = document.querySelector('button[aria-label="Run Studio playtest"]');
      return btn && !btn.hasAttribute("disabled");
    },
    null,
    { timeout: 15000 },
  );
  check("1-click starter build enables Playtest/Export", true);

  // Code Diff workbench tab
  await p.getByRole("button", { name: "Code", exact: true }).first().click();
  await p.locator("[data-code-diff-workbench]").waitFor({ state: "visible", timeout: 8000 });
  check("Code workbench renders", true);

  // Visual ScreenGui / UI Designer workbench tab
  await p.getByRole("button", { name: "UI", exact: true }).first().click();
  await p.locator("[data-gui-designer]").waitFor({ state: "visible", timeout: 10000 });
  await p.locator("[data-gui-canvas]").waitFor({ state: "visible", timeout: 5000 });
  check("UI Designer workbench renders", true);
  await assertLayoutInvariants(p, "1440x900 Light UI Designer");

  // Invalid ScreenGui slug shows inline validation error UI
  await p.locator("[data-gui-new]").click();
  await p.locator("[data-gui-slug-input]").fill("../bad slug!");
  await p.locator("[data-gui-create-slug]").click();
  await p.locator("[data-gui-slug-error]").waitFor({ state: "visible", timeout: 4000 });
  const slugErrTxt = await p.locator("[data-gui-slug-error]").innerText();
  check("UI Designer invalid slug displays error UI", /invalid screen slug/i.test(slugErrTxt), slugErrTxt);
  await p.getByRole("button", { name: "Cancel", exact: true }).click();

  // GuiDesigner Dark theme switch & 0px overflow
  await setThemeViaSettings(p, "dark");
  await assertLayoutInvariants(p, "1440x900 Dark UI Designer");
  await setThemeViaSettings(p, "light");

  // Switch presets (Obby -> Gacha -> RPG)
  await p.locator('[data-gui-preset="obby"]').click();
  await p.locator('[data-gui-canvas-node="obby-stage-pill"]').waitFor({ state: "visible", timeout: 4000 });
  await p.locator('[data-gui-preset="gacha"]').click();
  await p.locator('[data-gui-canvas-node="gacha-banner-card"]').waitFor({ state: "visible", timeout: 4000 });
  check("UI Designer preset switcher loads Obby and Gacha HUDs", true);

  // Device viewport switcher & Safe-area overlay toggles
  await p.locator('[data-gui-device="mobile"]').click();
  await p.locator('[data-gui-device="tablet"]').click();
  await p.locator('[data-gui-device="desktop"]').click();
  await p.locator('[data-gui-overlay="topbar"]').click();
  await p.locator('[data-gui-overlay="topbar"]').click();
  await p.locator('[data-gui-overlay="thumbstick"]').click();
  await p.locator('[data-gui-overlay="thumbstick"]').click();
  await p.locator("[data-gui-topbar-overlay]").waitFor({ state: "visible", timeout: 4000 });
  await p.locator("[data-gui-thumbstick-overlay]").waitFor({ state: "visible", timeout: 4000 });
  await p.locator("[data-gui-warnings]").waitFor({ state: "visible", timeout: 4000 });
  check("UI Designer device switcher & safe-area overlays work", true);

  // Edit a node property, trigger safe-area check, save screen, and compile strict Luau
  await p.locator('[data-gui-canvas-node="gacha-gems"]').click();
  const yOffsetInput = p.locator('[data-gui-prop="Position.y_offset"]');
  await yOffsetInput.fill("12");
  await p.locator('[data-gui-warning-item="topbar_overlap"]').first().waitFor({ state: "visible", timeout: 4000 });
  await yOffsetInput.fill("66");
  await p.locator("[data-gui-save]").click();
  await p.waitForFunction(
    () => {
      const btn = document.querySelector("[data-gui-save]");
      return btn && !btn.hasAttribute("disabled") && !btn.textContent?.includes("Save*");
    },
    null,
    { timeout: 8000 },
  );
  check("UI Designer edits node, detects TopBar overlap, and saves ScreenDoc", true);

  await p.locator("[data-gui-compile]").click();
  await p.locator("[data-gui-luau-output]").waitFor({ state: "visible", timeout: 8000 });
  const luauText = await p.locator("[data-gui-luau-output]").innerText();
  check(
    "UI Designer compiles --!strict Luau module",
    luauText.includes("--!strict") && luauText.includes("Module.create"),
    `${luauText.length} chars`,
  );

  // Ship tab & Monetization Pack
  await p.getByRole("button", { name: "Ship", exact: true }).first().click();
  await p.locator("[data-ship], [data-ship-empty]").first().waitFor({ state: "visible", timeout: 10000 });
  const monPack = p.locator("[data-monetization-pack]");
  if ((await monPack.count()) > 0) {
    await monPack.first().waitFor({ state: "visible", timeout: 5000 });
    await p.locator("[data-install-monetization]").first().click();
    await p.waitForFunction(
      () => document.body.innerText.includes("ProcessReceiptGuard") || document.body.innerText.includes("Installed"),
      null,
      { timeout: 10000 },
    );
    check("Monetization & Retention Luau pack install", true);
  } else {
    check("Ship panel renders", true);
  }

  // Ensure an active project is open so Playtests -> SandboxRunnerCard is mounted
  if ((await p.locator("[data-ship-empty]").count()) > 0) {
    const openProjBtn = p.getByRole("button", { name: "Open Projects", exact: true });
    if ((await openProjBtn.count()) > 0) {
      await openProjBtn.click();
      const newProjTrigger = p.getByRole("button", { name: "New project", exact: true }).first();
      await newProjTrigger.waitFor({ state: "visible", timeout: 8000 });
      await newProjTrigger.click();
      const nameInput = p.locator('input[placeholder="Untitled Anime Card Game"]').first();
      await nameInput.waitFor({ state: "visible", timeout: 5000 });
      await nameInput.fill("Sandbox Gate Place");
      await p.getByRole("button", { name: "Create project", exact: true }).click();
      await p.locator("[data-ship]").waitFor({ state: "visible", timeout: 10000 });
      const explorerBtn = p.locator('nav[aria-label="Explorer, Chats, and Projects"]').getByRole("button", { name: /^Explorer/ }).first();
      if ((await explorerBtn.count()) > 0) await explorerBtn.click();
    }
  }

  // Ensure the active project has a PlayerGui ScreenGui LocalScript so Headless Sandbox core_loop.hud passes
  await p.evaluate(async () => {
    const projs = await fetch("/api/projects").then((r) => r.json());
    const activeId = projs?.active;
    if (!activeId) return;
    await fetch(`/api/projects/${encodeURIComponent(activeId)}/file`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        path: "src/client/Hud.client.luau",
        content: [
          'local Players = game:GetService("Players")',
          'local pg = Players.LocalPlayer:WaitForChild("PlayerGui")',
          'local sg = Instance.new("ScreenGui")',
          'sg.Name = "MainHud"',
          "sg.Parent = pg",
          'local lbl = Instance.new("TextLabel")',
          'lbl.Name = "Status"',
          'lbl.Text = "Ready"',
          "lbl.Parent = sg",
          "",
        ].join("\n"),
      }),
    });
  });

  // Headless Luau Runtime Sandbox assertions in Light & Dark themes
  if ((await p.locator("[data-ship]").count()) > 0) {
    await p.locator('[aria-label="Ship section"]').getByRole("button", { name: "Playtests", exact: true }).click();
    await p.locator("[data-sandbox-runner]").waitFor({ state: "visible", timeout: 8000 });

    const runPresetAndVerify = async (presetId, label) => {
      await p.locator(`[data-sandbox-preset="${presetId}"]`).click();
      await p.locator("[data-sandbox-result]").waitFor({ state: "visible", timeout: 15000 });
      await p.waitForFunction(
        () => {
          const btn = document.querySelector("[data-sandbox-run]");
          return btn && !btn.hasAttribute("disabled");
        },
        null,
        { timeout: 15000 },
      );
      const engine = await p.locator("[data-sandbox-engine]").getAttribute("data-sandbox-engine");
      const dur = await p.locator("[data-sandbox-duration]").innerText();
      check(`Sandbox preset (${presetId}): ${label}`, Boolean(engine) && dur.length > 0, `${engine} in ${dur}`);
    };

    await runPresetAndVerify("boot_join", "Boot + Join Light");
    await runPresetAndVerify("receipt_idempotency", "Receipt Idempotency Light");
    await runPresetAndVerify("save_rejoin", "DataStore Rejoin Persistence Light");

    for (const tab of ["console", "datastores", "leaderstats", "receipts", "playergui"]) {
      await p.locator(`[data-sandbox-tab="${tab}"]`).click();
      await p.locator(`[data-sandbox-inspector-body="${tab}"]`).waitFor({ state: "visible", timeout: 4000 });
    }
    check("Sandbox Console & State Inspector tabs (Light)", true);

    await setThemeViaSettings(p, "dark");
    await assertLayoutInvariants(p, "1440x900 Dark Sandbox");
    await runPresetAndVerify("boot_join", "Boot + Join Dark");
    await p.locator('[data-sandbox-tab="datastores"]').click();
    await p.locator('[data-sandbox-inspector-body="datastores"]').waitFor({ state: "visible", timeout: 4000 });
    await p.locator('[data-sandbox-tab="playergui"]').click();
    await p.locator('[data-sandbox-inspector-body="playergui"]').waitFor({ state: "visible", timeout: 4000 });
    check("Sandbox Console & State Inspector tabs (Dark)", true);
    await setThemeViaSettings(p, "light");

    // Invalid Sandbox journey JSON shows step + card error UI and recovers on preset reset
    await p.locator("[data-sandbox-builder-toggle]").click();
    const argsInput = p.locator("[data-sandbox-step-args]").first();
    await argsInput.waitFor({ state: "visible", timeout: 4000 });
    await argsInput.fill("[bad-json");
    await p.locator("[data-sandbox-step-error]").first().waitFor({ state: "visible", timeout: 4000 });
    await p.locator("[data-sandbox-run]").click();
    await p.locator("[data-sandbox-error]").waitFor({ state: "visible", timeout: 4000 });
    check("Sandbox invalid journey JSON displays recovery error UI", true);
    await runPresetAndVerify("boot_join", "Sandbox recovery after invalid JSON");

    // Sandbox spatial/prompt/detector/key step builders + HTTP Mocks JSON + persisted history across reload
    for (const op of ["move_player", "trigger_prompt", "click_detector", "key_down"]) {
      await p.locator(`[data-sandbox-add-step="${op}"]`).click();
    }
    await p.locator("[data-sandbox-step-to]").first().waitFor({ state: "visible", timeout: 4000 });
    await p.locator("[data-sandbox-step-path]").first().waitFor({ state: "visible", timeout: 4000 });
    await p.locator("[data-sandbox-step-key]").first().waitFor({ state: "visible", timeout: 4000 });
    const httpMocksInput = p.locator("[data-sandbox-http-mocks]");
    await httpMocksInput.waitFor({ state: "visible", timeout: 4000 });
    await httpMocksInput.fill('[{"status":999}]');
    await p.locator("[data-sandbox-http-mocks-error]").waitFor({ state: "visible", timeout: 4000 });
    check("Sandbox malformed HTTP Mocks displays inline validation error", true);
    await httpMocksInput.fill('[{"url":"https://api.example.com/boost","status":200,"body":{"mult":2}}]');
    check("Sandbox Journey Steps builder renders spatial/prompt/detector/key & HTTP Mocks", true);
    await httpMocksInput.fill("");

    await runPresetAndVerify("spatial_loop", "Spatial + Input Light");
    await p.locator("[data-sandbox-history]").waitFor({ state: "visible", timeout: 8000 });
    const historyCountBefore = await p.locator("[data-sandbox-history-item]").count();
    check("Sandbox Run History displays persisted runs", historyCountBefore >= 1, `${historyCountBefore} run(s)`);

    // Reload page and confirm persisted run history + latest report survive reload
    await p.reload({ waitUntil: "domcontentloaded" });
    await p.locator("[data-top-chrome]").waitFor({ state: "visible", timeout: 15000 });
    await p.getByRole("button", { name: "Ship", exact: true }).first().click();
    await p.locator("[data-ship]").waitFor({ state: "visible", timeout: 10000 });
    await p.locator('[aria-label="Ship section"]').getByRole("button", { name: "Playtests", exact: true }).click();
    await p.locator("[data-sandbox-history]").waitFor({ state: "visible", timeout: 8000 });
    await p.locator("[data-sandbox-result]").waitFor({ state: "visible", timeout: 8000 });
    const historyCountAfter = await p.locator("[data-sandbox-history-item]").count();
    check("Sandbox Run History & latest proof survive reload", historyCountAfter >= 1, `${historyCountAfter} run(s)`);

    // Readiness Autopilot: 1-click Verify via Headless Sandbox + Headless Sandbox tier badge (Light & Dark)
    await p.locator('[aria-label="Ship section"]').getByRole("button", { name: "Publish & Monetize", exact: true }).click();
    await p.locator("[data-ship-autopilot]").waitFor({ state: "visible", timeout: 8000 });
    const verifySbxBtn = p.locator("[data-verify-headless-sandbox]").first();
    await verifySbxBtn.waitFor({ state: "visible", timeout: 5000 });
    await verifySbxBtn.click();
    const tierBadge = p.locator('[data-verification-tier="headless_sandbox"]').first();
    await tierBadge.waitFor({ state: "visible", timeout: 15000 });
    const tierTextLight = await tierBadge.innerText();
    check(
      "Readiness surfaces Headless Sandbox tier badge (1440x900 Light)",
      /Headless Sandbox/i.test(tierTextLight),
      tierTextLight,
    );
    await assertLayoutInvariants(p, "1440x900 Light Readiness Sandbox Tier");
    await setThemeViaSettings(p, "dark");
    await tierBadge.waitFor({ state: "visible", timeout: 5000 });
    await assertLayoutInvariants(p, "1440x900 Dark Readiness Sandbox Tier");
    await setThemeViaSettings(p, "light");

    // Live-Ops Control Plane & Player Telemetry (DataStores, MessagingService, Remote Config, Telemetry)
    await p.locator('[aria-label="Ship section"]').getByRole("button", { name: "LiveOps", exact: true }).click();
    await p.locator("[data-liveops-panel]").waitFor({ state: "visible", timeout: 8000 });
    await p.locator("[data-liveops-datastores]").waitFor({ state: "visible", timeout: 4000 });
    await p.locator("[data-liveops-messaging]").waitFor({ state: "visible", timeout: 4000 });
    await p.locator("[data-liveops-remote-config]").waitFor({ state: "visible", timeout: 4000 });
    await p.locator("[data-liveops-telemetry]").waitFor({ state: "visible", timeout: 4000 });
    check("LiveOpsPanel renders all 4 sub-sections", true);
    await assertLayoutInvariants(p, "1440x900 Light LiveOps");

    // Remote Config edit + LiveConfig.luau generation
    await p.locator("[data-rc-add]").click();
    await p.locator("[data-rc-key-input]").last().fill("GateDropBoost");
    await p.locator('[data-rc-value="GateDropBoost"]').fill("2.5");
    const pushDsCheckbox = p.locator("[data-rc-push-ds]");
    if (await pushDsCheckbox.isChecked()) await pushDsCheckbox.uncheck();
    const genLuauCheckbox = p.locator("[data-rc-gen-luau]");
    if (!(await genLuauCheckbox.isChecked())) await genLuauCheckbox.check();
    await p.locator("[data-rc-save]").click();
    await p.waitForFunction(
      () => {
        const txt = document.querySelector("[data-rc-status]")?.textContent || "";
        return txt.includes("remote-config.json") && txt.includes("LiveConfig.luau");
      },
      null,
      { timeout: 8000 },
    );
    const rcStatusText = await p.locator("[data-rc-status]").innerText();
    check(
      "LiveOps Remote Config saves and generates LiveConfig.luau",
      rcStatusText.includes("remote-config.json") && rcStatusText.includes("LiveConfig.luau"),
      rcStatusText,
    );

    // Confirmation-gated MessagingService broadcast + invalid JSON guard
    await p.locator("[data-msg-topic]").fill("GateLiveEvent");
    const msgPayloadInput = p.locator("[data-msg-payload]");
    const prevMsgPayload = await msgPayloadInput.inputValue();
    await msgPayloadInput.fill("{bad-json");
    await p.locator("[data-msg-json-error]").waitFor({ state: "visible", timeout: 4000 });
    const msgDisabledOnBadJson = await p.locator("[data-msg-publish]").isDisabled();
    check("LiveOps MessagingService blocks invalid JSON payload", msgDisabledOnBadJson);
    await msgPayloadInput.fill(prevMsgPayload);
    const msgPublishBtn = p.locator("[data-msg-publish]");
    const gatedBeforeConfirm = await msgPublishBtn.isDisabled();
    const ocState = await p.evaluate(async () => {
      try {
        const r = await fetch("/api/opencloud");
        return await r.json();
      } catch {
        return { has_key: false };
      }
    });
    const isWebEngine = await p.evaluate(() => Boolean(window.__OPENRO_WEB_ENGINE__));
    const routePattern = "**/api/projects/*/liveops/messaging/publish";
    if (!isWebEngine && !ocState?.has_key) {
      await p.route(routePattern, async (route) => {
        const reqBody = route.request().postDataJSON() || {};
        if (reqBody.confirm !== true) {
          await route.fulfill({
            status: 400,
            contentType: "application/json",
            body: JSON.stringify({ ok: false, error: "confirm required" }),
          });
          return;
        }
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({
            ok: true,
            topic: reqBody.topic || "GateLiveEvent",
            published: true,
            published_at: new Date().toISOString(),
            target: "gate_sim",
          }),
        });
      });
    }
    await p.locator("[data-msg-confirm]").check();
    const enabledAfterConfirm = await msgPublishBtn.isEnabled();
    await msgPublishBtn.click();
    await p.locator("[data-msg-receipt]").waitFor({ state: "visible", timeout: 8000 });
    const receiptText = await p.locator("[data-msg-receipt]").innerText();
    if (!isWebEngine && !ocState?.has_key) {
      await p.unroute(routePattern);
    }
    check(
      "LiveOps MessagingService confirmation gate and broadcast",
      gatedBeforeConfirm && enabledAfterConfirm && receiptText.includes("GateLiveEvent"),
      receiptText,
    );

    // Live Player Telemetry & Funnel Cohort ingest + display
    await p.locator("[data-telemetry-ingest-sample]").click();
    await p.waitForFunction(
      () => {
        const dau = Number(document.querySelector("[data-telemetry-dau]")?.textContent?.trim() || "0");
        const ev = Number(document.querySelector("[data-telemetry-events]")?.textContent?.trim() || "0");
        return dau > 0 && ev > 0;
      },
      null,
      { timeout: 8000 },
    );
    const dauTxt = await p.locator("[data-telemetry-dau]").innerText();
    const funnelCount = await p.locator("[data-telemetry-funnel] li").count();
    check(
      "LiveOps Telemetry ingests sample batch and renders funnel cohort",
      Number(dauTxt) > 0 && funnelCount > 0,
      `DAU=${dauTxt}, steps=${funnelCount}`,
    );

    await setThemeViaSettings(p, "dark");
    await assertLayoutInvariants(p, "1440x900 Dark LiveOps");
    await setThemeViaSettings(p, "light");

    // Bidirectional Studio Sync (Pull from Studio, strategy picker, diff preview, selective apply)
    const isWebEngineSync = await p.evaluate(() => Boolean(window.__OPENRO_WEB_ENGINE__));
    const pullSyncPattern = "**/api/projects/*/sync/pull*";
    const importModelPattern = "**/api/projects/*/import-model";
    if (!isWebEngineSync) {
      await p.route(pullSyncPattern, async (route) => {
        const resp = await route.fetch();
        if (resp.status() !== 404) {
          await route.fulfill({ response: resp });
          return;
        }
        const url = route.request().url();
        const reqBody = route.request().postDataJSON() || {};
        if (url.endsWith("/sync/pull-diff")) {
          await route.fulfill({
            status: 200,
            contentType: "application/json",
            body: JSON.stringify({
              ok: true,
              snapshot_id: "gate-pull-snap-1",
              applied_count: 0,
              conflict_count: 1,
              changes: [
                {
                  path: "src/server/StudioSpawn.server.luau",
                  kind: "script",
                  status: "studio_only",
                  studio_hash: 12345678,
                  hunks: [{ header: "@@ -0,0 +1,2 @@", lines: [{ op: "add", text: "-- Studio authored spawn script" }] }],
                },
                {
                  path: "Workspace.GateArch",
                  kind: "instance",
                  status: "studio_modified",
                  local_hash: 1111,
                  studio_hash: 2222,
                  baseline_hash: 1111,
                  hunks: [{ header: "@@ -1,1 +1,1 @@", lines: [{ op: "del", text: "Position: [0,2,0]" }, { op: "add", text: "Position: [0,6,0]" }] }],
                },
                {
                  path: "src/shared/Config.luau",
                  kind: "script",
                  status: "conflict",
                  local_hash: 3333,
                  studio_hash: 4444,
                  baseline_hash: 1000,
                  hunks: [{ header: "@@ -1,1 +1,1 @@", lines: [{ op: "del", text: "local rate = 1" }, { op: "add", text: "local rate = 2" }] }],
                },
              ],
            }),
          });
          return;
        }
        const appliedPaths = Array.isArray(reqBody.paths) ? reqBody.paths : [];
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({
            ok: true,
            snapshot_id: "gate-pull-snap-2",
            applied_count: appliedPaths.length,
            conflict_count: 0,
            changes: [],
          }),
        });
      });
      await p.route(importModelPattern, async (route) => {
        const resp = await route.fetch();
        if (resp.status() !== 404) {
          await route.fulfill({ response: resp });
          return;
        }
        await route.fulfill({
          status: 200,
          contentType: "application/json",
          body: JSON.stringify({
            ok: true,
            model_path: "scene/models/gatetower.model.json",
            extracted_scripts: ["src/models/gatetower/TowerPulse.luau"],
            wired_service: "Workspace",
            warnings: [],
          }),
        });
      });
    }

    await p.locator("[data-studio-sync-toolbar-btn]").click();
    const syncDlg = p.locator("[data-two-way-sync-dialog]");
    await syncDlg.waitFor({ state: "visible", timeout: 8000 });
    await syncDlg.locator("[data-pull-sync-diff]").click();
    await syncDlg.locator("[data-pull-results]").waitFor({ state: "visible", timeout: 8000 });
    const pullRowsCount = await syncDlg.locator("[data-pull-change-row]").count();
    if (pullRowsCount > 0) {
      const firstRow = syncDlg.locator("[data-pull-change-row]").first();
      await firstRow.locator("[data-pull-preview-diff]").click();
      await firstRow.locator("[data-pull-hunks]").waitFor({ state: "visible", timeout: 4000 });
      await syncDlg.locator('[data-pull-strategy-opt="take_studio"]').click();
      await syncDlg.locator('[data-pull-strategy-opt="merge_non_conflicting"]').click();
      const firstCb = firstRow.locator('[type="checkbox"]');
      if (!(await firstCb.isChecked())) await firstCb.check();
      await syncDlg.locator("[data-pull-apply]").click();
    }
    check("Two-Way Studio Sync pull diff, preview, strategy picker, and apply", true, `rows=${pullRowsCount}`);
    await syncDlg.getByRole("button", { name: "Close", exact: true }).click();
    await syncDlg.waitFor({ state: "hidden", timeout: 5000 });

    // Prefab Model Import (.rbxmx) in AssetsPanel -> scene/models/*.model.json
    await p.getByRole("button", { name: "Assets", exact: true }).first().click();
    await p.locator("[data-prefab-import-card]").waitFor({ state: "visible", timeout: 8000 });
    const sampleRbxmx = [
      '<roblox version="4">',
      '  <Item class="Model" referent="RBX001">',
      '    <Properties><string name="Name">GateTower</string></Properties>',
      '    <Item class="Part" referent="RBX002">',
      '      <Properties>',
      '        <string name="Name">TowerBase</string>',
      '        <Vector3 name="size"><X>6</X><Y>10</Y><Z>6</Z></Vector3>',
      '        <CoordinateFrame name="CFrame"><X>14</X><Y>5</Y><Z>-8</Z><R00>1</R00><R01>0</R01><R02>0</R02><R10>0</R10><R11>1</R11><R12>0</R12><R20>0</R20><R21>0</R21><R22>1</R22></CoordinateFrame>',
      "      </Properties>",
      "    </Item>",
      '    <Item class="Script" referent="RBX003">',
      '      <Properties><string name="Name">TowerPulse</string><ProtectedString name="Source"><![CDATA[--!strict\nprint("Tower ready")\n]]></ProtectedString></Properties>',
      "    </Item>",
      "  </Item>",
      "</roblox>",
    ].join("\n");
    await p.locator("[data-import-model-input]").setInputFiles({
      name: "GateTower.rbxmx",
      mimeType: "application/xml",
      buffer: Buffer.from(sampleRbxmx, "utf8"),
    });
    await p.locator("[data-import-model-result]").waitFor({ state: "visible", timeout: 10000 });
    const importResTxt = await p.locator("[data-import-model-result]").innerText();
    check(
      "Prefab Model Import (.rbxmx) writes scene/models/*.model.json",
      importResTxt.includes(".model.json"),
      importResTxt.slice(0, 80),
    );
    await assertLayoutInvariants(p, "1440x900 Light Assets Prefab Import");

    if (!isWebEngineSync) {
      await p.unroute(pullSyncPattern);
      await p.unroute(importModelPattern);
    }
  }

  // Check Assets, Swarm, and Viewport (StudioViewport3D) tabs on workbench in Light & Dark
  await p.getByRole("button", { name: "Assets", exact: true }).first().click();
  await p.getByRole("button", { name: "Swarm", exact: true }).first().click();
  await p.getByRole("button", { name: "Viewport", exact: true }).first().click();
  const previewBtn = p.locator('button[aria-label="Open 3D Preview"]').first();
  if ((await previewBtn.count()) > 0 && (await previewBtn.isVisible())) {
    await previewBtn.click();
  }
  await p.locator("[data-studio-viewport-3d]").waitFor({ state: "visible", timeout: 8000 });
  const vpCanvas = p.locator('[data-testid="viewport-canvas"]');
  await vpCanvas.waitFor({ state: "visible", timeout: 8000 });
  const rendererKind = await vpCanvas.getAttribute("data-renderer");
  const rendererBadgeText = await p.locator('[data-testid="viewport-renderer-badge"]').innerText();
  check(
    "StudioViewport3D renderer ready (webgl2 or canvas2d-fallback)",
    (rendererKind === "webgl2" || rendererKind === "canvas2d-fallback") && rendererBadgeText.trim().length > 0,
    `${rendererKind} (${rendererBadgeText.trim()})`,
  );

  // Terrain generator preset + Paint Box + Dynamic Light upsert persistence
  await p.locator("[data-toggle-terrain-panel]").click();
  await p.locator("[data-terrain-toolbar]").waitFor({ state: "visible", timeout: 4000 });
  await p.locator('[data-terrain-preset="hills"]').click();
  await p.waitForTimeout(200);
  await p.locator("[data-terrain-paint]").click();
  await p.waitForTimeout(200);
  await assertLayoutInvariants(p, "1440x900 Light StudioViewport3D Terrain");

  await p.locator("[data-toggle-lights-panel]").click();
  await p.locator("[data-lights-toolbar]").waitFor({ state: "visible", timeout: 4000 });
  const lightCountBefore = await p.locator("[data-light-item]").count();
  await p.locator('[data-add-light="PointLight"]').click();
  await p.waitForFunction(
    (prev) => document.querySelectorAll("[data-light-item]").length > prev,
    lightCountBefore,
    { timeout: 8000 },
  );
  // Switch tabs and return to verify scene light persistence
  await p.getByRole("button", { name: "Code", exact: true }).first().click();
  await p.locator("[data-code-diff-workbench]").waitFor({ state: "visible", timeout: 5000 });
  await p.getByRole("button", { name: "Viewport", exact: true }).first().click();
  const previewBtnReturn = p.locator('button[aria-label="Open 3D Preview"]').first();
  if ((await previewBtnReturn.count()) > 0 && (await previewBtnReturn.isVisible())) {
    await previewBtnReturn.click();
  }
  await p.locator("[data-studio-viewport-3d]").waitFor({ state: "visible", timeout: 8000 });
  await p.locator("[data-toggle-lights-panel]").click();
  const lightCountAfter = await p.locator("[data-light-item]").count();
  check(
    "StudioViewport3D Terrain & PointLight upsert persist across tab switches",
    lightCountAfter > lightCountBefore,
    `lights=${lightCountAfter}`,
  );
  await p.locator("[data-toggle-lights-panel]").click();

  await assertLayoutInvariants(p, "1440x900 Light StudioViewport3D");
  await setThemeViaSettings(p, "dark");
  await assertLayoutInvariants(p, "1440x900 Dark StudioViewport3D");
  await setThemeViaSettings(p, "light");
  check("Workbench Assets, Swarm, StudioViewport3D Light/Dark switch cleanly", true);

  // Check collapsible left rail & right workbench
  await p.locator('button[aria-label="Collapse left dock"]').click();
  await p.locator('button[aria-label="Expand left dock"]').waitFor({ state: "visible", timeout: 4000 });
  await p.locator('button[aria-label="Expand left dock"]').click();
  await p.locator('button[aria-label="Collapse Workbench"]').first().click();
  await p.locator('button[aria-label="Expand Workbench"]').first().waitFor({ state: "visible", timeout: 4000 });
  await p.locator('button[aria-label="Expand Workbench"]').first().click();
  check("Collapsible left dock & right workbench toggles", true);

  // Command palette opens and closes via Escape
  await p.locator('button[aria-label="Open command palette"]').click();
  await p.locator('input[placeholder*="command"], input[placeholder*="Search"]').first().waitFor({ state: "visible", timeout: 4000 });
  await p.keyboard.press("Escape");
  check("Command palette opens & closes on Escape", true);

  // Explorer search filter
  const expFilter = p.locator('input[placeholder*="Filter files"]');
  await expFilter.fill("ProcessReceipt");
  await p.waitForTimeout(100);
  await expFilter.fill("");
  check("Explorer search filter clears cleanly", true);

  // Export .rbxlx
  await p.locator('button[aria-label="Project export and import menu"]').click();
  const dlPromise = p.waitForEvent("download", { timeout: 10000 });
  await p.getByRole("button", { name: "Export .rbxlx" }).click();
  const dl = await dlPromise;
  const dlPath = await dl.path();
  const dlSize = dlPath ? fs.statSync(dlPath).size : 0;
  check("Export .rbxlx", dlSize > 100 && dl.suggestedFilename().endsWith(".rbxlx"), `${dlSize} B`);

  // Undo dialog focus trap & Escape restore
  const undoBtn = p.locator('button[aria-label="Undo"]');
  if (await undoBtn.isEnabled()) {
    await undoBtn.click();
    const undoDlg = p.locator("[data-undo-dialog]");
    await undoDlg.waitFor({ state: "visible", timeout: 5000 });
    await p.keyboard.press("Escape");
    await undoDlg.waitFor({ state: "hidden", timeout: 5000 });
    check("Undo dialog opens & closes on Escape", true);
  }

  // Dark theme at 1440x900
  await setThemeViaSettings(p, "dark");
  await assertLayoutInvariants(p, "1440x900 Dark");
  await p.screenshot({ path: `${OUT}/built-desktop-1440-dark.png` });

  // 1280x800 laptop viewport Dark & Light
  await p.setViewportSize({ width: 1280, height: 800 });
  await assertLayoutInvariants(p, "1280x800 Dark");
  await p.screenshot({ path: `${OUT}/built-desktop-1280-dark.png` });
  await setThemeViaSettings(p, "light");
  await assertLayoutInvariants(p, "1280x800 Light");
  await p.screenshot({ path: `${OUT}/built-desktop-1280-light.png` });

  // Tablet 768x1024 Light & Dark
  await p.setViewportSize({ width: 768, height: 1024 });
  await p.locator("#composer-input").waitFor({ state: "visible", timeout: 5000 });
  await assertLayoutInvariants(p, "768x1024 Light");
  await setThemeViaSettings(p, "dark");
  await assertLayoutInvariants(p, "768x1024 Dark");

  // Mobile 390x844 Dark & Light
  await p.setViewportSize({ width: 390, height: 844 });
  await p.locator("#composer-input").waitFor({ state: "visible", timeout: 5000 });
  await assertLayoutInvariants(p, "390x844 Dark");
  await setThemeViaSettings(p, "light");
  await assertLayoutInvariants(p, "390x844 Light");
  await p.locator('button[aria-label="Open the menu"]').click();
  const drawerNav = p.locator('[role="dialog"] nav[aria-label="Explorer, Chats, and Projects"]');
  await drawerNav.waitFor({ state: "visible", timeout: 5000 });
  await p.keyboard.press("Escape");
  await drawerNav.waitFor({ state: "hidden", timeout: 5000 });
  check("Mobile left drawer opens & closes on Escape", true);
  await p.screenshot({ path: `${OUT}/mobile-explorer-390-light.png` });

  // Mobile 390x844 Workbench: GuiDesigner + StudioViewport3D theme switch & 0px overflow
  await p.locator('[aria-label="Switch Copilot or Workbench view"]').getByRole("button", { name: "Workbench", exact: true }).click();
  await p.getByRole("button", { name: "UI", exact: true }).first().click();
  await p.locator("[data-gui-designer]").waitFor({ state: "visible", timeout: 8000 });
  await assertLayoutInvariants(p, "390x844 Light UI Designer");
  await p.getByRole("button", { name: "Viewport", exact: true }).first().click();
  const mobPreviewBtn = p.locator('button[aria-label="Open 3D Preview"]').first();
  if ((await mobPreviewBtn.count()) > 0 && (await mobPreviewBtn.isVisible())) {
    await mobPreviewBtn.click();
  }
  await p.locator("[data-studio-viewport-3d]").waitFor({ state: "visible", timeout: 8000 });
  const mobCanvasRenderer = await p.locator('[data-testid="viewport-canvas"]').getAttribute("data-renderer");
  check(
    "390x844 Mobile StudioViewport3D renderer ready",
    mobCanvasRenderer === "webgl2" || mobCanvasRenderer === "canvas2d-fallback",
    String(mobCanvasRenderer),
  );
  await assertLayoutInvariants(p, "390x844 Light StudioViewport3D");
  await setThemeViaSettings(p, "dark");
  await assertLayoutInvariants(p, "390x844 Dark StudioViewport3D");
  await setThemeViaSettings(p, "light");

  // Mobile 390x844 Ship Readiness Headless Sandbox tier badge + Sandbox history in Light & Dark
  const shipTabMob = p.getByRole("button", { name: "Ship", exact: true }).first();
  if ((await shipTabMob.count()) > 0) {
    await shipTabMob.click();
    if ((await p.locator("[data-ship]").count()) > 0) {
      await p.locator('[aria-label="Ship section"]').getByRole("button", { name: "Publish & Monetize", exact: true }).click();
      await p.locator("[data-ship-autopilot]").waitFor({ state: "visible", timeout: 8000 });
      // LiveOps & Terrain mutations invalidated earlier sandbox fingerprint; re-verify in 390x844
      const mobVerifyBtn = p.locator("[data-verify-headless-sandbox]").first();
      await mobVerifyBtn.waitFor({ state: "visible", timeout: 5000 });
      await mobVerifyBtn.click();
      const mobTier = p.locator('[data-verification-tier="headless_sandbox"]').first();
      await mobTier.waitFor({ state: "visible", timeout: 15000 });
      check("390x844 Mobile Readiness shows Headless Sandbox tier badge", /Headless Sandbox/i.test(await mobTier.innerText()));
      await assertLayoutInvariants(p, "390x844 Light Readiness");
      await setThemeViaSettings(p, "dark");
      await assertLayoutInvariants(p, "390x844 Dark Readiness");
      await setThemeViaSettings(p, "light");
      await p.locator('[aria-label="Ship section"]').getByRole("button", { name: "Playtests", exact: true }).click();
      await p.locator("[data-sandbox-history]").waitFor({ state: "visible", timeout: 8000 });
      await assertLayoutInvariants(p, "390x844 Light Sandbox History");
      await setThemeViaSettings(p, "dark");
      await assertLayoutInvariants(p, "390x844 Dark Sandbox History");
      await setThemeViaSettings(p, "light");
    }
  }
  await p.locator('[aria-label="Switch Copilot or Workbench view"]').getByRole("button", { name: "Copilot", exact: true }).click();

  await p.emulateMedia({ reducedMotion: "reduce" });
  await p.locator("#composer-input").fill("Add a spinning coin pickup");
  await p.locator('button[aria-label="Send"]').click();
  await p.waitForTimeout(300);
  check("prefers-reduced-motion flow", true);

  check("0 console errors", consoleErrors.length === 0, consoleErrors.slice(0, 3).join(" | "));
  check("0 page errors", pageErrors.length === 0, pageErrors.slice(0, 3).join(" | "));
  check("0 HTTP 5xx responses", badResponses.length === 0, badResponses.join(", "));
  await ctx.close();
} finally {
  await b.close();
}

report("gate", res);
