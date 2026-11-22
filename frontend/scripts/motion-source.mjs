// M1 motion-source check for OpenRO (House Frontend Step 7):
// Verifies screen files under frontend/src/ use only presets from rune/used/motion.ts and CSS tokens --dur-* / --ease-house,
// never literal durations, springs, delays, or transition-all, and verifies motion.ts matches tokens.css.
import fs from "node:fs";
import path from "node:path";

const feRoot = path.resolve(import.meta.dirname, "..");
const repoRoot = path.resolve(feRoot, "..");
const src = path.join(feRoot, "src");
const used = path.join(repoRoot, "rune", "used");
const fails = [];
const fail = (where, msg) => { fails.push(`${where}: ${msg}`); console.error(`FAIL [motion-source] ${where}: ${msg}`); };
const rel = (p) => path.relative(repoRoot, p);
const walk = (d) => fs.readdirSync(d, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(path.join(d, e.name)) : [path.join(d, e.name)]));

const strip = (s) => {
  let o = "", q = null;
  for (let i = 0; i < s.length; i++) {
    const c = s[i], d = s[i + 1];
    if (q) { o += c; if (c === "\\") o += s[++i] ?? ""; else if (c === q || (c === "\n" && q !== "`")) q = null; continue; }
    if (c === '"' || c === "'" || c === "`") { q = c; o += c; continue; }
    if (c === "/" && d === "/" && s[i - 1] !== ":") { for (; i < s.length && s[i] !== "\n"; i++) o += " "; i--; continue; }
    if (c === "/" && d === "*") { const e = s.indexOf("*/", i + 2), end = e < 0 ? s.length : e + 2; for (; i < end; i++) o += s[i] === "\n" ? "\n" : " "; i--; continue; }
    o += c;
  }
  return o;
};
const lineOf = (t, i) => t.slice(0, i).split("\n").length;
const NUM = String.raw`-?(?:\d+\.?\d*|\.\d+)(?:e-?\d+)?`;

for (const f of walk(src).filter((x) => /\.tsx?$/.test(x) && !x.endsWith(".d.ts"))) {
  const t = strip(fs.readFileSync(f, "utf8"));
  const variant = String.raw`(?:[\w\[\]=&>*().:-]+:)*`;
  const checks = [
    [new RegExp(String.raw`(?<![\w-])${variant}(?:duration|delay)-(?:\d+|\[[^\]\s]+\])(?![\w-])`, "g"), (m) => `Tailwind literal duration/delay "${m[0]}"`],
    [new RegExp(String.raw`(?<![\w-])${variant}ease-(?:linear|in|out|in-out|\[[^\]\s]+\])(?![\w-])`, "g"), (m) => `Tailwind literal easing "${m[0]}"`],
    [new RegExp(String.raw`(?<![\w-])${variant}transition-all(?![\w-])`, "g"), () => `transition-all is forbidden`],
    [/\b(?:stiffness|damping|bounce|visualDuration)\s*:\s*\d/g, (m) => `literal spring parameter "${m[0]}"`],
    [/\btransition\s*=\s*\{\s*\{/g, () => `inline transition={{...}} literal: pass a preset from @/motion`],
  ];
  for (const [re, msg] of checks) {
    for (let m; (m = re.exec(t)); ) fail(`${rel(f)}:${lineOf(t, m.index)}`, msg(m));
  }
}

for (const f of walk(src).filter((x) => x.endsWith(".css") && path.basename(x) !== "tokens.css" && path.basename(x) !== "theme.css")) {
  const c = fs.readFileSync(f, "utf8").replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, " "));
  for (let m, re = /\btransition(?:-property)?\s*:\s*all\b/g; (m = re.exec(c)); ) {
    fail(`${rel(f)}:${lineOf(c, m.index)}`, 'transition on "all"');
  }
  for (let m, re = /(?<![\w-])(transition|animation)(-duration|-delay|-timing-function)?\s*:\s*([^;}]+)/g; (m = re.exec(c)); ) {
    const v = m[3].replace(/!important/, "").replace(/calc\((?:[^()]|\([^()]*\))*\)/g, (x) => (/var\(--dur-/.test(x) ? "T" : x)).replace(/var\(--[\w-]+\)/g, "V");
    const lit = [...v.matchAll(/(?<![\w.-])(\d*\.?\d+)(ms|s)\b/g)].find((x) => Number(x[1]) * (x[2] === "s" ? 1000 : 1) > 1);
    if (lit) fail(`${rel(f)}:${lineOf(c, m.index)}`, `literal time ${lit[0]} in ${m[0].trim()}`);
  }
}

const mf = path.join(used, "motion.ts");
const tk = fs.readFileSync(path.join(src, "tokens.css"), "utf8");
const raw = fs.readFileSync(mf, "utf8");
const cssEase = tk.match(/--ease-house\s*:\s*cubic-bezier\(([^)]*)\)/)?.[1].split(",").map(Number);
const jsEase = raw.match(/export const ease = \[([^\]]*)\]/)?.[1].split(",").map(Number);
if (!cssEase || !jsEase || jsEase.join() !== cssEase.join()) fail("rune/used/motion.ts", "ease disagrees with --ease-house");
const jsDur = raw.match(/export const dur = \{([^}]*)\}/)?.[1];
for (const k of ["press", "reveal", "swap", "enter", "sheet"]) {
  const css = tk.match(new RegExp(String.raw`--dur-${k}\s*:\s*(\d+)ms`))?.[1];
  const js = jsDur?.match(new RegExp(String.raw`\b${k}\s*:\s*(${NUM})`))?.[1];
  if (!css || js === undefined || Math.round(Number(js) * 1000) !== Number(css)) {
    fail("rune/used/motion.ts", `dur.${k} disagrees with --dur-${k}`);
  }
}

console.log(`motion-source: ${fails.length ? `FAILED (${fails.length})` : "ok, all screen files use @/motion and --dur-* tokens"}`);
process.exit(fails.length ? 1 : 0);
