// shots.mjs: OpenRO v1.1.0 at 1440x900, 1280x800, and 390x844, light and dark, to .out/shots.
import fs from "node:fs";
import { BASE, OUT as E2E_OUT, launch } from "./fixture.mjs";

const OUT = `${E2E_OUT}/shots`;
const TAG = "ui";
fs.mkdirSync(OUT, { recursive: true });

const b = await launch({ args: ["--no-sandbox"] });
const errs = [];

try {
  for (const scheme of ["light", "dark"]) {
    for (const [vn, w, h] of [
      ["1440", 1440, 900],
      ["1280", 1280, 800],
      ["390", 390, 844],
    ]) {
      const ctx = await b.newContext({
        viewport: { width: w, height: h },
        colorScheme: scheme,
        deviceScaleFactor: 1,
      });
      await ctx.addInitScript((s) => {
        try {
          localStorage.setItem("openro-theme", s);
        } catch {}
      }, scheme);
      const p = await ctx.newPage();
      p.on("console", (m) => m.type() === "error" && errs.push(`${vn}-${scheme}: ${m.text()}`));
      p.on("pageerror", (e) => errs.push(`${vn}-${scheme}: ${e.message}`));
      await p.goto(BASE, { waitUntil: "load" });
      await p.locator("[data-top-chrome]").waitFor({ state: "visible", timeout: 15000 });
      await p.waitForTimeout(800);
      await p.screenshot({ path: `${OUT}/${TAG}-${vn}-${scheme}.png` });
      if (vn !== "1280") {
        await p.addStyleTag({
          content:
            ".h-dvh{height:auto!important}.overflow-hidden,.overflow-y-auto{overflow:visible!important}.min-h-0{min-height:auto!important}",
        });
        await p.waitForTimeout(250);
        await p.screenshot({ path: `${OUT}/${TAG}-${vn}-${scheme}-full.png`, fullPage: true });
      }
      await ctx.close();
    }
  }
} finally {
  await b.close();
}

console.log(errs.length ? errs.join("\n") : "no console errors");
