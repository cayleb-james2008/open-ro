// Role coverage check for OpenRO (House Frontend Step 2):
// Walks screen files from frontend/src/main.tsx, credits library parts in ../rune/used/components and packages
// (sonner, vaul, motion/react), checks ../ROLE-COVERAGE.md, and forbids hand-written buttons/disclosures/dialogs/tabs.
import fs from "node:fs";
import path from "node:path";

const feRoot = path.resolve(import.meta.dirname, "..");
const repoRoot = path.resolve(feRoot, "..");
const src = path.join(feRoot, "src");
const used = path.join(repoRoot, "rune", "used");

export const LIBS = ["Motion Primitives", "COSS UI", "Cult UI", "Kibo UI", "Tremor", "Vaul", "Sonner", "Motion"];
const ALIAS = {
  "motion primitives": "Motion Primitives",
  "coss ui": "COSS UI",
  coss: "COSS UI",
  "cult ui": "Cult UI",
  cult: "Cult UI",
  "kibo ui": "Kibo UI",
  kibo: "Kibo UI",
  tremor: "Tremor",
  vaul: "Vaul",
  sonner: "Sonner",
  motion: "Motion",
  "motion.dev": "Motion",
};
const MAX_NO_NEED = 3;

const FILES = {
  "ui/animated-background": ["Motion Primitives", "Animated Background", true],
  "ui/sliding-number": ["Motion Primitives", "Sliding Number", true],
  "ui/text-morph": ["Motion Primitives", "Text Morph", true],
  "ui/morphing-dialog": ["Motion Primitives", "Morphing Dialog", true],
  "ui/dock": ["Motion Primitives", "Dock", true],
  "ui/in-view": ["Motion Primitives", "In View", true],
  "ui/animated-group": ["Motion Primitives", "Animated Group", true],
  "ui/spotlight": ["Motion Primitives", "Spotlight", true],
  "ui/border-trail": ["Motion Primitives", "Border Trail", true],
  "ui/text-shimmer": ["Motion Primitives", "Text Shimmer", true],
  "ui/transition-panel": ["Motion Primitives", "Transition Panel", true],
  "ui/scroll-progress": ["Motion Primitives", "Scroll Progress", true],
  "ui/magnetic": ["Motion Primitives", "Magnetic", true],
  "ui/button": ["COSS UI", "Button", true],
  "ui/spinner": ["COSS UI", "Spinner", true],
  "ui/input-group": ["COSS UI", "Input Group", true],
  "ui/input": ["COSS UI", "Input", true],
  "ui/textarea": ["COSS UI", "Textarea", true],
  "ui/kbd": ["COSS UI", "Kbd", true],
  "ui/tabs": ["COSS UI", "Tabs", true],
  "ui/segmented-control": ["COSS UI", "Segmented Control", true],
  "ui/badge": ["COSS UI", "Badge", true],
  "ui/avatar": ["COSS UI", "Avatar", true],
  "ui/table": ["COSS UI", "Table", true],
  "ui/tooltip": ["COSS UI", "Tooltip", true],
  "ui/texture-button": ["Cult UI", "Texture Button", false],
  "ui/dynamic-island": ["Cult UI", "Dynamic Island", true],
  "ui/expandable": ["Cult UI", "Expandable", true],
  "kibo-ui/kanban": ["Kibo UI", "Kanban", true],
  "kibo-ui/gantt": ["Kibo UI", "Gantt", true],
  "kibo-ui/calendar": ["Kibo UI", "Calendar", true],
  "kibo-ui/pill": ["Kibo UI", "Pill", true],
  "kibo-ui/avatar-stack": ["Kibo UI", "Avatar Stack", true],
  "SparkChart/SparkChart": ["Tremor", "Spark chart", true],
  "ProgressCircle/ProgressCircle": ["Tremor", "Progress Circle", true],
  "Tracker/Tracker": ["Tremor", "Tracker", true],
  "BarList/BarList": ["Tremor", "Bar List", true],
  "CategoryBar/CategoryBar": ["Tremor", "Category Bar", true],
  "Tooltip/Tooltip": ["Tremor", "Tremor Tooltip", true],
  "ui/card": null,
  "ui/command": null,
  "ui/context-menu": null,
  "ui/dialog": null,
  "ui/popover": null,
  "ui/scroll-area": null,
};

const PKGS = {
  sonner: { lib: "Sonner", parts: { toast: "toast (a stacked notice)", Toaster: "Toaster" } },
  vaul: { lib: "Vaul", all: "Drawer" },
  "motion/react": {
    lib: "Motion",
    parts: {
      motion: "motion element",
      m: "motion element",
      useSpring: "spring",
      useMotionValue: "motion value",
      useTransform: "motion value",
      animate: "animate()",
      useAnimate: "animate()",
      AnimatePresence: "AnimatePresence",
      LayoutGroup: "layout",
      Reorder: "layout",
      useScroll: "scroll-linked value",
      useInView: "in-view",
      useDragControls: "drag",
    },
  },
};
PKGS.motion = PKGS["motion/react"];
PKGS["framer-motion"] = PKGS["motion/react"];

const HAND = [
  [/<button\b/, "a raw <button>: use the COSS UI Button (or the part that owns the control)"],
  [/role=["']?button\b/, 'role="button" on a hand-made element: use the COSS UI Button'],
  [/<(details|summary)\b/, "a hand-written disclosure: Cult UI Expandable or a Motion Primitives Morphing Dialog"],
  [/aria-expanded=/, "a hand-wired disclosure (aria-expanded): Cult UI Expandable, or the Morphing Dialog / Vaul Drawer"],
  [/role=["']?(tablist|tab|tabpanel)\b/, "hand-written tabs: COSS UI Tabs or Segmented Control, Motion Primitives Animated Background"],
  [/role=["']?(dialog|alertdialog)\b|<dialog\b/, "a hand-written dialog: Motion Primitives Morphing Dialog, or a Vaul Drawer on a phone"],
  [/role=["']?tooltip\b/, "a hand-written tooltip: COSS UI Tooltip"],
  [/role=["']?progressbar\b/, "a hand-written progress bar: Tremor Progress Circle or Category Bar"],
  [/from ["'](?:recharts|d3[^"']*)["']/, "a hand-written chart: Tremor"],
  [/from ["']@dnd-kit\//, "hand-wired drag and drop: Kibo UI Kanban"],
  [/from ["']@base-ui\/react/, "COSS UI's own primitive used directly: take the COSS UI part"],
];

const strip = (t) => t.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, " ")).replace(/(^|[^:"'`])\/\/[^\n]*/g, "$1");
const lineOf = (text, idx) => text.slice(0, idx).split("\n").length;
const rel = (p) => path.relative(repoRoot, p);

function resolve(from, spec) {
  const roots = spec.startsWith("@/")
    ? [path.join(src, spec.slice(2)), path.join(used, spec.slice(2))]
    : spec.startsWith(".")
      ? [path.resolve(path.dirname(from), spec)]
      : [];
  for (const base of roots) {
    for (const c of [base, base + ".tsx", base + ".ts", path.join(base, "index.tsx"), path.join(base, "index.ts")]) {
      if (fs.existsSync(c) && fs.statSync(c).isFile()) return c;
    }
  }
  return null;
}

function imports(file, text) {
  const out = [];
  const re = /import\s+(type\s+)?([^"';]*?)\s*from\s*["']([^"']+)["']/g;
  for (let m; (m = re.exec(text)); ) {
    if (m[1]) continue;
    const names = [];
    const clause = m[2].trim();
    const brace = clause.match(/\{([\s\S]*)\}/);
    const lead = clause.replace(/\{[\s\S]*\}/, "").replace(/,\s*$/, "").trim();
    if (lead.startsWith("* as ")) names.push({ local: lead.slice(5).trim(), imported: "*" });
    else if (lead) names.push({ local: lead, imported: "default" });
    if (brace) {
      for (const part of brace[1].split(",")) {
        const p = part.trim();
        if (!p || /^type\s/.test(p)) continue;
        const [imp, loc] = p.split(/\s+as\s+/).map((s) => s.trim());
        names.push({ local: loc ?? imp, imported: imp });
      }
    }
    out.push({ spec: m[3], names, line: lineOf(text, m.index), file: resolve(file, m[3]) });
  }
  return out;
}

const componentKey = (file) => {
  const r = path.relative(path.join(used, "components"), file).replace(/\\/g, "/").replace(/\.tsx?$/, "").replace(/\/index$/, "");
  return r.startsWith("..") ? null : r;
};
const inLib = (file) => file.startsWith(used + path.sep);
const refs = (text, name) => (text.match(new RegExp(`(?<![\\w$.])${name.replace(/\$/g, "\\$")}(?![\\w$])`, "g")) ?? []).length;
const rendered = (text, name) => new RegExp(`<${name}[\\s/>.]|(?<![\\w$.])${name}\\s*[.(]|[=:{(,]\\s*${name}\\s*[,})\\]]`).test(text);

const fails = [];
const fail = (tag, msg) => {
  fails.push(`[${tag}] ${msg}`);
  console.error(`FAIL [${tag}] ${msg}`);
};

const entry = path.join(src, "main.tsx");
const reached = new Set();
const queue = [entry];
const texts = new Map();
while (queue.length) {
  const f = queue.pop();
  if (reached.has(f)) continue;
  reached.add(f);
  if (inLib(f)) continue;
  const t = strip(fs.readFileSync(f, "utf8"));
  texts.set(f, t);
  for (const i of imports(f, t)) if (i.file) queue.push(i.file);
}

const every = (d) =>
  fs.readdirSync(d, { withFileTypes: true }).flatMap((e) =>
    e.isDirectory() ? every(path.join(d, e.name)) : /\.tsx?$/.test(e.name) && !e.name.endsWith(".d.ts") ? [path.join(d, e.name)] : [],
  );
for (const f of every(src)) {
  if (!inLib(f) && !reached.has(f)) fail("dead", `${rel(f)}: nothing imports it from src/main.tsx; delete it`);
}

const found = new Map();
for (const [f, t] of texts) {
  for (const imp of imports(f, t)) {
    const pkg = PKGS[imp.spec];
    const comp = imp.file ? componentKey(imp.file) : null;
    for (const n of imp.names) {
      if (!rendered(t, n.local) && !(n.imported === "*" && new RegExp(`\\b${n.local}\\.`).test(t))) continue;
      const add = (lib, part, strength) => {
        const k = `${lib}|${part}`;
        if (!found.has(k)) found.set(k, { lib, part, strength, where: new Set() });
        found.get(k).where.add(rel(f));
      };
      if (pkg) {
        if (pkg.all) add(pkg.lib, pkg.all, true);
        else if (pkg.parts[n.imported]) add(pkg.lib, pkg.parts[n.imported], true);
      } else if (comp !== null && imp.file) {
        const entry = FILES[comp];
        if (entry) add(entry[0], entry[1], entry[2]);
      }
    }
  }
  if (/from ["'](?:motion\/react|motion|framer-motion)["']/.test(t)) {
    const kinds = [
      [/<(?:motion|m)\.\w+/, "motion element"],
      [/\blayoutId\s*=/, "layoutId glide"],
      [/<(?:motion|m)\.\w+[^>]*\blayout(?:\s|=|>|\/)/, "layout (FLIP)"],
      [/\b(?:whileHover|whileTap|whileDrag|drag)\s*=/, "gesture"],
    ];
    for (const [re, part] of kinds) {
      if (re.test(t)) {
        const k = `Motion|${part}`;
        if (!found.has(k)) found.set(k, { lib: "Motion", part, strength: true, where: new Set() });
        found.get(k).where.add(rel(f));
      }
    }
  }
}

for (const [f, t] of texts) {
  for (const imp of imports(f, t)) {
    const lib = PKGS[imp.spec] || (imp.file && componentKey(imp.file) !== null);
    if (!lib) continue;
    for (const n of imp.names) {
      if (n.imported !== "*" && refs(t, n.local) <= 1) {
        fail("dead", `${rel(f)}:${imp.line}: ${n.local} is imported from ${imp.spec} but never rendered`);
      }
    }
  }
  for (const [re, msg] of HAND) {
    const g = new RegExp(re.source, "g");
    for (let m; (m = g.exec(t)); ) fail("hand-written", `${rel(f)}:${lineOf(t, m.index)}: ${msg}`);
  }
}

const usedByLib = Object.fromEntries(LIBS.map((l) => [l, [...found.values()].filter((p) => p.lib === l)]));
console.log("Parts the screens import and render:");
for (const l of LIBS) {
  const u = usedByLib[l];
  console.log(`  ${l.padEnd(18)} ${u.length ? u.map((p) => `${p.part} (${[...p.where].slice(0, 2).join("; ")})`).join(", ") : "none"}`);
}

const tableFile = [path.join(repoRoot, "ROLE-COVERAGE.md"), path.join(feRoot, "ROLE-COVERAGE.md")].find(fs.existsSync);
if (!tableFile) fail("table", "ROLE-COVERAGE.md is missing");
else {
  const rows = [];
  for (const line of fs.readFileSync(tableFile, "utf8").split("\n")) {
    if (!/^\s*\|/.test(line) || /^\s*\|[\s:|-]+\|?\s*$/.test(line)) continue;
    const cells = line.trim().replace(/^\||\|$/g, "").split("|").map((c) => c.trim());
    const lib = ALIAS[cells[0].toLowerCase().replace(/[*`_]/g, "").replace(/\s*\(.*\)/, "").trim()];
    if (lib) rows.push({ lib, cells: cells.slice(1) });
  }
  const norm = (s) => s.toLowerCase().replace(/[^a-z0-9]/g, "");
  let noNeed = 0;
  for (const l of LIBS) {
    const mine = rows.filter((r) => r.lib === l);
    const u = usedByLib[l];
    if (!mine.length) { fail("table", `${l}: no row in ROLE-COVERAGE.md`); continue; }
    const nn = mine.filter((r) => /no need here/i.test(r.cells.join(" ")));
    if (nn.length) noNeed++;
    if (!u.length && !nn.length) fail("coverage", `${l}: no part imported/rendered and no "no need here" row`);
    for (const r of mine.filter((x) => !/no need here/i.test(x.cells.join(" ")))) {
      const text = norm(r.cells.join(" "));
      if (u.length && !u.some((p) => text.includes(norm(p.part.replace(/^Tremor /, "").split(/[\s(]/)[0])))) {
        fail("table", `${l}: row names no rendered part (rendered: ${u.map((p) => p.part).join(", ")})`);
      }
    }
    if (u.length && !u.some((p) => p.strength)) fail("coverage", `${l}: used only outside its strength`);
  }
  if (noNeed > MAX_NO_NEED) fail("table", `${noNeed} libraries answered "no need here" (max ${MAX_NO_NEED})`);
}

console.log(`role-coverage: ${fails.length ? `FAILED (${fails.length})` : "ok"}`);
process.exit(fails.length ? 1 : 0);
