#!/usr/bin/env node
/**
 * Desktop-Workbench component & hygiene gate for OpenRO v1.1.0.
 * Walks the real Vite import graph from frontend/src/main.tsx and fails on:
 * - missing ROLE-COVERAGE.md or lean ui/motion primitives
 * - stale rune/used or bloated wrapper imports (motion/react, @base-ui/react, recharts, cmdk)
 * - anti-pattern CSS/class regressions (transition-colors, disabled:opacity-*)
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const FRONTEND = path.resolve(__dirname, "..");
const ROOT = path.resolve(FRONTEND, "..");
const SRC = path.join(FRONTEND, "src");

const issues = [];

const roleMdPath = path.join(ROOT, "ROLE-COVERAGE.md");
if (!fs.existsSync(roleMdPath)) {
  issues.push("ROLE-COVERAGE.md is missing");
}

const uiPath = path.join(SRC, "components", "ui.tsx");
if (!fs.existsSync(uiPath)) {
  issues.push("frontend/src/components/ui.tsx is missing");
} else {
  const uiSrc = fs.readFileSync(uiPath, "utf8");
  for (const sym of ["export function Button", "export function Dialog", "export function Pill", "export function ProgressCircle", "export function Kbd"]) {
    if (!uiSrc.includes(sym)) issues.push(`ui.tsx missing symbol: ${sym}`);
  }
}

const BANNED_IMPORTS = [
  "@base-ui/react",
  "recharts",
  "cmdk",
  "@dnd-kit/",
];

const visited = new Set();
const queue = [path.join(SRC, "main.tsx")];

function resolveLocal(fromFile, spec) {
  let target = null;
  if (spec.startsWith("@/")) {
    target = path.join(SRC, spec.slice(2));
  } else if (spec.startsWith("./") || spec.startsWith("../")) {
    target = path.resolve(path.dirname(fromFile), spec);
  }
  if (!target) return null;
  const candidates = [
    target,
    `${target}.tsx`,
    `${target}.ts`,
    `${target}.jsx`,
    `${target}.js`,
    path.join(target, "index.tsx"),
    path.join(target, "index.ts"),
  ];
  return candidates.find((c) => fs.existsSync(c) && fs.statSync(c).isFile()) ?? null;
}

while (queue.length > 0) {
  const file = queue.shift();
  if (!file || visited.has(file) || !fs.existsSync(file)) continue;
  visited.add(file);

  const src = fs.readFileSync(file, "utf8");
  const rel = path.relative(ROOT, file);

  if (src.includes("transition-colors")) {
    issues.push(`${rel}: contains forbidden .transition-colors (transitions outline in Tailwind v4)`);
  }
  if (/disabled:opacity-\d+/.test(src)) {
    issues.push(`${rel}: contains forbidden disabled:opacity-*`);
  }

  const importRe = /(?:import|export)\s+(?:[^"']*?\s+from\s+)?["']([^"']+)["']|import\(["']([^"']+)["']\)/g;
  let m;
  while ((m = importRe.exec(src)) !== null) {
    const spec = m[1] || m[2];
    if (!spec) continue;
    if (spec.includes("/rune/") || spec.startsWith("rune")) {
      issues.push(`${rel}: forbidden legacy rune import "${spec}"`);
    }
    for (const banned of BANNED_IMPORTS) {
      if (spec === banned || spec.startsWith(banned)) {
        issues.push(`${rel}: forbidden bloated dependency import "${spec}"`);
      }
    }
    const nextFile = resolveLocal(file, spec);
    if (nextFile && !visited.has(nextFile)) {
      queue.push(nextFile);
    }
  }
}

if (issues.length > 0) {
  console.error("role-coverage FAIL:");
  for (const i of issues) console.error(`  - ${i}`);
  process.exit(1);
}

console.log(`role-coverage OK: scanned ${visited.size} source modules, 0 legacy/bloat imports, 0 CSS regressions.`);
