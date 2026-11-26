// Single-file production bundle for OpenRO Studio Workbench: React + Tailwind 4,
// outputting ONE self-contained HTML file (JS, CSS and Geist woff2 fonts inlined) at ../assets/index.html.
import fs from "node:fs";
import path from "node:path";
import zlib from "node:zlib";
import { defineConfig, type Plugin } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

const root = import.meta.dirname;
const STAMP = "/* Hallmark Desktop-Workbench: OpenRO v1.15.0 Studio Copilot (Geist + Geist Mono, Light & Dark) */";
const FORCE_SINGLE_FILE = process.env.OPENRO_SINGLE_FILE === "1";

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
        const css = fs
          .readFileSync(path.join(dist, href.replace(/^\.?\//, "")), "utf8")
          .replace(/url\((['"]?)(?:\.\.\/chunks\/|\.\/)(Geist[^)"']+)/g, "url($1./chunks/$2");
        return `<style>${STAMP}\n${css}</style>`;
      });
      html = html.replace(/--tw-ring-offset-color:\s*#fff\b/g, "--tw-ring-offset-color:var(--surface)");
      html = html.replace(/\.transition\{[^}]*\}/g, "");
      if (FORCE_SINGLE_FILE) {
        const singleOut = path.join(dist, "single.html");
        fs.writeFileSync(singleOut, html);
        return;
      }
      const chunksSrc = path.join(dist, "chunks");
      const chunksDst = path.resolve(root, "../assets/chunks");
      fs.rmSync(chunksDst, { recursive: true, force: true });
      fs.mkdirSync(chunksDst, { recursive: true });
      let chunkCount = 0;
      let gzBytes = 0;
      if (fs.existsSync(chunksSrc)) {
        for (const f of fs.readdirSync(chunksSrc)) {
          if (f.endsWith(".js") || f.endsWith(".css")) {
            const full = path.join(chunksSrc, f);
            const raw = fs.readFileSync(full);
            if (raw.toString("utf8").includes("entry.js")) {
              throw new Error(`Lazy chunk ${f} must not import inlined entry.js`);
            }
            const gz = zlib.gzipSync(raw, { level: 9 });
            fs.writeFileSync(path.join(chunksDst, `${f}.gz`), gz);
            gzBytes += gz.length;
            chunkCount++;
          } else if (f.endsWith(".woff2")) {
            const full = path.join(chunksSrc, f);
            const raw = fs.readFileSync(full);
            fs.writeFileSync(path.join(chunksDst, f), raw);
            chunkCount++;
          }
        }
      }
      const out = path.resolve(root, "../assets/index.html");
      fs.writeFileSync(out, html);
      const htmlGz = zlib.gzipSync(Buffer.from(html, "utf8"), { level: 9 });
      fs.writeFileSync(`${out}.gz`, htmlGz);
      console.log(`single file: ${out} (${(html.length / 1024).toFixed(0)} kB, gz ${(htmlGz.length / 1024).toFixed(0)} kB, ${chunkCount} lazy gzipped chunk(s): ${(gzBytes / 1024).toFixed(0)} kB)`);
    },
  };
}

export default defineConfig({
  plugins: [localAlias(), react(), tailwindcss(), singleFile()],
  base: "./",
  build: {
    assetsInlineLimit: FORCE_SINGLE_FILE ? 100_000_000 : 0,
    cssCodeSplit: false,
    chunkSizeWarningLimit: 4000,
    modulePreload: false,
    rolldownOptions: {
      output: FORCE_SINGLE_FILE
        ? { codeSplitting: false }
        : {
            entryFileNames: "entry.js",
            chunkFileNames: "chunks/[name]-[hash].js",
            assetFileNames: "chunks/[name]-[hash].[ext]",
            codeSplitting: {
              groups: [
                {
                  name: "shared",
                  test: /preload-helper|node_modules|src\/(components|motion\.ts|app\/(data|luauAudit|marketTaste|systems|rbxlxCodec|AdaptiveAgentCards|ship\/api|ship\/ProjectsRail|teamwork\/api)\.(ts|tsx))/,
                },
              ],
            },
          },
    },
  },
  server: { fs: { allow: [path.resolve(root, "..")] }, proxy: process.env.API_PORT ? { "/api": { target: `http://127.0.0.1:${process.env.API_PORT}`, changeOrigin: true, headers: { Origin: `http://127.0.0.1:${process.env.API_PORT}` } } } : undefined },
});
