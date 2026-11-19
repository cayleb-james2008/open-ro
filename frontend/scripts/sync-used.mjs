// sync-used.mjs: copies exactly the library parts this app imports from the house-frontend starter into ../rune/used,
// follows the imports of every copied part, and writes rune/used/MANIFEST.md (part, library, licence, who imports it).
// Run after adding or removing a part import:  npm run sync-used
import fs from "node:fs";
import path from "node:path";
const root = path.resolve(import.meta.dirname, "..");
const used = path.resolve(root, "../rune/used");
const HOUSE = process.env.HOUSE_SRC || "/home/cayleb/.claude/skills/house-frontend/starter/src";
const exts = ["", ".tsx", ".ts", "/index.tsx", "/index.ts"];
const LIB = [
  [/components\/ui\/(texture-button|expandable)/, "Cult UI (cult-ui.com)", "MIT"],
  [/components\/kibo-ui\//, "Kibo UI (kibo-ui.com)", "MIT"],
  [/components\/(SparkChart|Tracker|ProgressCircle|BarList|CategoryBar|Tooltip)\//, "Tremor Raw (tremor.so)", "Apache-2.0"],
  [/components\/ui\/(morphing-dialog|animated-background|animated-group|sliding-number|text-morph|text-shimmer|border-trail|spotlight|magnetic|in-view|scroll-progress|transition-panel|dock|dynamic-island)/, "Motion Primitives (motion-primitives.com)", "MIT"],
  [/components\/ui\/(button|spinner|input-group|input|textarea|kbd|tabs|segmented-control|badge|avatar|table|tooltip)/, "COSS UI (coss.com/ui)", "MIT"],
  [/components\/ui\//, "shadcn/ui on Radix UI primitives", "MIT"],
  [/(lib|motion|hooks)/, "House presets (house-frontend starter)", "owner's"],
];
const lib = (rel) => LIB.find(([re]) => re.test(rel)) || [null, "House frontend starter", "owner's"];
const find = (base, id) => { for (const e of exts) { const p = path.join(base, id + e); if (fs.existsSync(p) && fs.statSync(p).isFile()) return p; } return null; };
// Parts edited in rune/used (see rune/used/LOCAL-EDITS.md): the copy must not overwrite them.
const LOCAL = new Set(fs.existsSync(path.join(used, "LOCAL-EDITS.md")) ? [...fs.readFileSync(path.join(used, "LOCAL-EDITS.md"), "utf8").matchAll(/`(components\/[^`]+|[^`]+\.tsx?)`/g)].map((m) => m[1]) : []);
const seen = new Map(); // rel path in rune/used -> importers
const queue = [];
const walkSrc = (d) => fs.readdirSync(d, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walkSrc(path.join(d, e.name)) : /\.(tsx?|css)$/.test(e.name) ? [path.join(d, e.name)] : []));
const importsOf = (file) => [...fs.readFileSync(file, "utf8").matchAll(/(?:from|import)\s+["']@\/([^"']+)["']/g)].map((m) => m[1]);
for (const f of walkSrc(path.join(root, "src"))) for (const id of importsOf(f)) if (!find(path.join(root, "src"), id)) queue.push([id, path.relative(root, f)]);
// Copied parts may also import their own neighbours by relative path (e.g. ../../utils/cx); those are followed too.
const relImportsOf = (file) => [...fs.readFileSync(file, "utf8").matchAll(/(?:from|import)\s+["'](\.{1,2}\/[^"']+)["']/g)].map((m) => m[1]);
while (queue.length) {
  const [id, by] = queue.shift();
  const src = id.startsWith("/") ? find(path.dirname(id), path.basename(id)) : find(HOUSE, id);
  if (!src) { console.error(`NOT FOUND in the house starter: ${id.startsWith("/") ? id : "@/" + id} (imported by ${by})`); process.exitCode = 1; continue; }
  const rel = path.relative(HOUSE, src);
  if (seen.has(rel)) { seen.get(rel).add(by); continue; }
  seen.set(rel, new Set([by]));
  const dest = path.join(used, rel);
  fs.mkdirSync(path.dirname(dest), { recursive: true });
  if (!LOCAL.has(rel)) fs.copyFileSync(src, dest);
  else console.log("kept local edit", rel);
  for (const sub of importsOf(src)) queue.push([sub, "rune/used/" + rel]);
  for (const sub of relImportsOf(src)) queue.push([path.resolve(path.dirname(src), sub), "rune/used/" + rel]);
}
// drop parts that are no longer imported
const have = (d) => (fs.existsSync(d) ? walkSrc(d) : []);
for (const f of have(used)) { const rel = path.relative(used, f); if (!seen.has(rel)) { fs.rmSync(f); console.log("removed unused part", rel); } }
const rows = [...seen.keys()].sort().map((rel) => { const [, l, lic] = lib(rel); return `| \`${rel}\` | ${l} | ${lic} | ${[...seen.get(rel)].sort().join(", ")} |`; });
fs.mkdirSync(used, { recursive: true });
fs.writeFileSync(path.join(used, "MANIFEST.md"), `# rune/used: the library parts this app uses

Copied unchanged (apart from the house edits recorded in the starter's STACK.lock.md) from the owner's house-frontend starter by \`frontend/scripts/sync-used.mjs\`. Only parts the screens import are here; \`npm run sync-used\` refreshes this folder and this table. npm packages the parts need (react, motion, radix, vaul, sonner, recharts, lucide, ...) are in \`frontend/package.json\`.

| Part | Library | Licence | Imported by |
| --- | --- | --- | --- |
${rows.join("\n")}
`);
console.log(`rune/used: ${seen.size} parts`);
