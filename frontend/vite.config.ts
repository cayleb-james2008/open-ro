// House-frontend build for a Rune (Rust) app: React + Tailwind 4, the library parts live in ../rune/used,
// and the result is ONE html file (js, css and fonts inline) at ../assets/index.html, which the Rust binary embeds.
import fs from "node:fs";
import path from "node:path";
import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

const root = import.meta.dirname;
const used = path.resolve(root, "../rune/used");

// "@/x" is the app's own src/x first, then the copied library part rune/used/x.
function runeUsed(): Plugin {
  const exts = ["", ".tsx", ".ts", ".css", "/index.tsx", "/index.ts"];
  return {
    name: "rune-used",
    enforce: "pre",
    resolveId(id) {
      if (!id.startsWith("@/")) return null;
      for (const base of [path.resolve(root, "src"), used]) {
        for (const e of exts) {
          const p = path.join(base, id.slice(2) + e);
          if (fs.existsSync(p) && fs.statSync(p).isFile()) return p;
        }
      }
      return null;
    },
  };
}

// After the build: inline the script, the stylesheet and (through assetsInlineLimit) the fonts into one page.
function singleFile(): Plugin {
  return {
    name: "single-file",
    enforce: "post",
    closeBundle() {
      const dist = path.resolve(root, "dist");
      let html = fs.readFileSync(path.join(dist, "index.html"), "utf8");
      html = html.replace(/<script[^>]*src="([^"]+)"[^>]*><\/script>/g, (_m, src) => {
        const js = fs.readFileSync(path.join(dist, src.replace(/^\.?\//, "")), "utf8").replace(/<\/script/gi, "<\\/script");
        return `<script type="module">${js}</script>`;
      });
      html = html.replace(/<link[^>]*rel="stylesheet"[^>]*href="([^"]+)"[^>]*>/g, (_m, href) => {
        const css = fs.readFileSync(path.join(dist, href.replace(/^\.?\//, "")), "utf8");
        return `<style>${css}</style>`;
      });
      // Hallmark: the stamp is the first line of the page CSS (the lint reads that line); Tailwind's fixed #fff ring offset becomes the surface token.
      html = html.replace(/<style>/, "<style>/* Hallmark · app screen · genre modern-minimal · scope app · theme Hum (native light, dark second) · critique: paper grain, card-stock rims, one torn edge, no gradients on the page */\n");
      html = html.replace(/--tw-ring-offset-color:\s*#fff\b/g, "--tw-ring-offset-color:var(--surface)");
      // The bare `transition` utility comes only from motion prop names in source, never from an element of this page;
      // Tailwind merges the core rule into the override, which would fade focus rings (Hallmark 15). Drop the rule.
      html = html.replace(/\.transition\{[^}]*\}/g, "");
      const out = path.resolve(root, "../assets/index.html");
      fs.writeFileSync(out, html);
      console.log(`single file: ${out} (${(html.length / 1024).toFixed(0)} kB)`);
    },
  };
}

export default defineConfig({
  plugins: [runeUsed(), react(), tailwindcss(), singleFile()],
  base: "./",
  build: { assetsInlineLimit: 100_000_000, cssCodeSplit: false, chunkSizeWarningLimit: 4000, modulePreload: false },
  server: { fs: { allow: [path.resolve(root, "..")] }, proxy: process.env.API_PORT ? { "/api": { target: `http://127.0.0.1:${process.env.API_PORT}`, changeOrigin: true, headers: { Origin: `http://127.0.0.1:${process.env.API_PORT}` } } } : undefined },
});
