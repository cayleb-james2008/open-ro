// Single-file production bundle for OpenRO Studio Workbench: React + Tailwind 4,
// outputting ONE self-contained HTML file (JS, CSS and Geist woff2 fonts inlined) at ../assets/index.html.
import fs from "node:fs";
import path from "node:path";
import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

const root = import.meta.dirname;
const STAMP = "/* Hallmark Desktop-Workbench: OpenRO v1.3.0 Studio Copilot (Geist + Geist Mono, Light & Dark) */";

function localAlias(): Plugin {
  const exts = ["", ".tsx", ".ts", ".css", "/index.tsx", "/index.ts"];
  return {
    name: "local-alias",
    enforce: "pre",
    resolveId(id) {
      if (!id.startsWith("@/")) return null;
      const base = path.resolve(root, "src");
      for (const e of exts) {
        const p = path.join(base, id.slice(2) + e);
        if (fs.existsSync(p) && fs.statSync(p).isFile()) return p;
      }
      return null;
    },
  };
}

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
        return `<style>${STAMP}\n${css}</style>`;
      });
      html = html.replace(/--tw-ring-offset-color:\s*#fff\b/g, "--tw-ring-offset-color:var(--surface)");
      html = html.replace(/\.transition\{[^}]*\}/g, "");
      const out = path.resolve(root, "../assets/index.html");
      fs.writeFileSync(out, html);
      console.log(`single file: ${out} (${(html.length / 1024).toFixed(0)} kB)`);
    },
  };
}

export default defineConfig({
  plugins: [localAlias(), react(), tailwindcss(), singleFile()],
  base: "./",
  build: { assetsInlineLimit: 100_000_000, cssCodeSplit: false, chunkSizeWarningLimit: 4000, modulePreload: false },
  server: { fs: { allow: [path.resolve(root, "..")] }, proxy: process.env.API_PORT ? { "/api": { target: `http://127.0.0.1:${process.env.API_PORT}`, changeOrigin: true, headers: { Origin: `http://127.0.0.1:${process.env.API_PORT}` } } } : undefined },
});
