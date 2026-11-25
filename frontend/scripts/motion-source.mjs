#!/usr/bin/env node
/**
 * Verifies Emil Kowalski v2.0 Motion Contract between frontend/src/tokens.css + index.css and frontend/src/motion.ts.
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const SRC = path.resolve(__dirname, "..", "src");

const tokensPath = path.join(SRC, "tokens.css");
const indexPath = path.join(SRC, "index.css");
const motionPath = path.join(SRC, "motion.ts");

const issues = [];

if (!fs.existsSync(tokensPath)) {
  issues.push("Missing frontend/src/tokens.css");
} else {
  const tokens = fs.readFileSync(tokensPath, "utf8");
  for (const req of [
    "--dur-press: 150ms",
    "--dur-reveal: 180ms",
    "--dur-swap: 200ms",
    "--dur-enter: 220ms",
    "--dur-sheet: 220ms",
    "--ease-house: cubic-bezier(0.23, 1, 0.32, 1)",
  ]) {
    if (!tokens.includes(req)) {
      issues.push(`tokens.css missing required v2.0 motion rule: ${req}`);
    }
  }
}

if (!fs.existsSync(indexPath) || !fs.readFileSync(indexPath, "utf8").includes("prefers-reduced-motion")) {
  issues.push("index.css missing prefers-reduced-motion guard");
}

if (!fs.existsSync(motionPath)) {
  issues.push("Missing frontend/src/motion.ts");
} else {
  const motion = fs.readFileSync(motionPath, "utf8");
  for (const req of ["stiffness: 450", "damping: 35", "mass: 0.7", "press: 0.15", "reveal: 0.18", "swap: 0.2", "enter: 0.22", "sheet: 0.22", "prefers-reduced-motion", "useReducedMotion"]) {
    if (!motion.includes(req)) {
      issues.push(`motion.ts missing required v2.0 preset value: ${req}`);
    }
  }
}

if (issues.length > 0) {
  console.error("motion-source FAIL:");
  for (const i of issues) console.error(`  - ${i}`);
  process.exit(1);
}

console.log("motion-source OK: v2.0 Emil Kowalski contract (tokens.css, index.css, motion.ts) verified.");
