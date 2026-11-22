// build-web.mjs: compiles ../web-engine/install.ts via Vite library mode (IIFE) and embeds it
// into ../assets/index.html to produce a self-contained, zero-backend functional OpenRO Studio
// web app (../web-dist/index.html and optional WEB_EXPORT_DIR/index.html).
// Zero fixture demos, zero canned mocks: real parametric Luau compiler, Luau syntax/wiring
// playtest validator, interactive Stage Preview, .rbxlx XML exporter, IndexedDB persistence,
// optional BYOK LLM (sessionStorage only), and one-click Connect to Local openro-rs.
import fs from "node:fs";
import path from "node:path";
import { build } from "vite";

const feRoot = path.resolve(import.meta.dirname, "..");
const repoRoot = path.resolve(feRoot, "..");
const srcHtml = path.join(repoRoot, "assets", "index.html");

if (!fs.existsSync(srcHtml)) {
  throw new Error(`Missing ${srcHtml} — run npm run build first.`);
}

const outTmp = path.join(feRoot, ".web-engine-dist");
fs.rmSync(outTmp, { recursive: true, force: true });

await build({
  configFile: false,
  root: feRoot,
  logLevel: "silent",
  build: {
    outDir: outTmp,
    emptyOutDir: true,
    minify: true,
    lib: {
      entry: path.join(feRoot, "web-engine", "install.ts"),
      name: "OpenROWebEngine",
      formats: ["iife"],
      fileName: () => "engine.js",
    },
  },
});

const engineCode = fs
  .readFileSync(path.join(outTmp, "engine.js"), "utf8")
  .replace(/<\/script/gi, "<\\/script");
fs.rmSync(outTmp, { recursive: true, force: true });

const html = fs
  .readFileSync(srcHtml, "utf8")
  .replace(/__KEY_BANNER__/g, "")
  .replace(/__CHIP_CLASS__/g, "on")
  .replace(/__CHIP__/g, "Browser Place ready")
  .replace(/__CAN_BUILD__/g, "true")
  .replace(/__STUDIO_CONNECTED__/g, "true")
  .replace(/__WORKSPACE_MODE__/g, "browser_place")
  .replace(/__GAME_NAME__/g, "OpenRO Studio")
  .replace(/<script type="module">/, `<script>${engineCode}</script>\n<script type="module">`);

const targets = [
  path.join(repoRoot, "web-dist", "index.html"),
  ...(process.env.WEB_EXPORT_DIR ? [path.join(process.env.WEB_EXPORT_DIR, "index.html")] : []),
];

for (const out of targets) {
  fs.mkdirSync(path.dirname(out), { recursive: true });
  fs.writeFileSync(out, html);
  console.log(`live web bundle: ${out} (${(html.length / 1024).toFixed(0)} kB)`);
}
