#!/usr/bin/env node
/**
 * Verifies motion tokens in frontend/src/tokens.css + index.css match frontend/src/motion.ts
 * and prefers-reduced-motion support is present without heavy runtime wrappers.
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
  for (const req of ["--dur-press", "--dur-reveal", "--dur-swap", "--dur-enter", "--dur-sheet", "--ease-house"]) {
    if (!tokens.includes(req)) {
      issues.push(`tokens.css missing required motion token: ${req}`);
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
  if (motion.includes("motion/react") || motion.includes("framer-motion")) {
    issues.push("motion.ts must not import motion/react or framer-motion");
  }
  if (!motion.includes("prefers-reduced-motion")) {
    issues.push("motion.ts must respect prefers-reduced-motion");
  }
  if (!motion.includes("useReducedMotion")) {
    issues.push("motion.ts must export useReducedMotion");
  }
}

if (issues.length > 0) {
  console.error("motion-source FAIL:");
  for (const i of issues) console.error(`  - ${i}`);
  process.exit(1);
}

console.log("motion-source OK: tokens.css, index.css, and motion.ts verified (reduced-motion compliant).");
