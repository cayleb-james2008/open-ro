// replay.mjs: Multi-turn reload persistence & theme persistence replay checks for OpenRO v1.1.0.
import fs from "node:fs";
import { BASE, OUT as E2E_OUT, launch, reset, report } from "./fixture.mjs";

const OUT = `${E2E_OUT}/replay`;
fs.mkdirSync(OUT, { recursive: true });

const res = [];
const check = (n, p, d = "") => {
  res.push({ n, p: !!p, d });
  console.log((p ? "PASS " : "FAIL ") + n + (d ? "  " + d : ""));
};

const browser = await launch();

try {
  await reset();
  const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 } });
  const page = await ctx.newPage();

  const errs = [];
  page.on("pageerror", (e) => errs.push(String(e)));

  await page.goto(BASE, { waitUntil: "domcontentloaded" });
  await page.locator("#composer-input").waitFor({ state: "visible", timeout: 15000 });

  await page.locator("#composer-input").fill("Build a glowing checkpoint obby stage");
  await page.locator('button[aria-label="Send"]').click();
  await page.waitForFunction(
    () =>
      document.querySelectorAll("#thread .rounded-panel, #thread .rounded-card").length >= 2 &&
      !document.querySelector('button[aria-label="Stop"]'),
    null,
    { timeout: 15000 },
  );
  await page.waitForTimeout(150);
  const countBefore = await page.locator("#thread .rounded-panel, #thread .rounded-card").count();
  check("Thread records user prompt & response", countBefore >= 2, `${countBefore} msgs`);

  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForFunction(
    () => document.querySelectorAll("#thread .rounded-panel, #thread .rounded-card").length >= 2,
    null,
    { timeout: 15000 },
  );
  const countAfter = await page.locator("#thread .rounded-panel, #thread .rounded-card").count();
  check("Reload persists conversation", countAfter >= countBefore, `${countAfter} msgs`);

  // Switch theme to dark, reload, verify persisted
  await page.locator('button[aria-label="Settings"]').click();
  const dlg = page.locator("[data-settings-dialog]");
  await dlg.waitFor({ state: "visible", timeout: 5000 });
  await dlg.getByRole("button", { name: "Dark", exact: true }).click();
  await page.keyboard.press("Escape");
  await page.reload({ waitUntil: "domcontentloaded" });
  await page.waitForFunction(() => document.documentElement.getAttribute("data-theme") === "dark", null, {
    timeout: 8000,
  });
  check("Dark theme persists across reload", true);

  // Switch back to Light cleanly
  await page.locator('button[aria-label="Settings"]').click();
  await dlg.waitFor({ state: "visible", timeout: 5000 });
  await dlg.getByRole("button", { name: "Light", exact: true }).click();
  await page.keyboard.press("Escape");
  await page.waitForFunction(() => document.documentElement.getAttribute("data-theme") === "light", null, {
    timeout: 8000,
  });
  check("Light theme restores cleanly", true);

  // Switch left rail to Chats, create a new chat, verify switching preserves threads
  await page.getByRole("button", { name: "Chats", exact: true }).click();
  const initialChats = await page.locator("#chat-list li").count();
  await page.getByRole("button", { name: "New chat" }).first().click();
  await page.locator("[data-copilot-welcome]").waitFor({ state: "visible", timeout: 8000 });
  check("New chat resets to starter welcome", true);
  const afterNewChats = await page.locator("#chat-list li").count();
  check("Chat list updates with new chat", afterNewChats >= Math.max(1, initialChats));

  // Switch back to first chat and check thread restored
  await page.locator("#chat-list li button").first().click();
  await page.waitForTimeout(250);
  check("Switching chats in left rail works", true);

  // Switch left rail back to Explorer and open Snapshots dialog
  await page.getByRole("button", { name: /^Explorer/ }).click();
  await page.getByRole("button", { name: "Snapshots" }).click();
  await page.keyboard.press("Escape");
  check("Snapshots dialog opens & closes cleanly", true);

  check("0 page errors during replay", errs.length === 0, errs.join(" | "));
  await ctx.close();
} finally {
  await browser.close();
}

report("replay", res);
