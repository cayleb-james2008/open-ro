// Generates src/tokens.css from design/tokens.json. `--check` fails if the generated file drifted
// from tokens.json or if any other renderer source uses a raw color or a gradient.
import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { join, relative } from 'node:path';

const root = new URL('..', import.meta.url).pathname;
const tokensPath = join(root, 'design/tokens.json');
const cssPath = join(root, 'src/tokens.css');

export function buildCss(tokens) {
  const lines = ['/* Generated from design/tokens.json by scripts/build-tokens.mjs. Do not edit. */', ':root {'];
  const hex = (name) => {
    const value = tokens.palette[name];
    if (!/^#[0-9a-f]{6}$/.test(value ?? '')) throw new Error(`Unknown or invalid palette color: ${name}`);
    return value;
  };
  for (const name of Object.keys(tokens.palette)) lines.push(`  --ctp-${name}: ${hex(name)};`);
  for (const [role, name] of Object.entries(tokens.color)) { hex(name); lines.push(`  --color-${role}: var(--ctp-${name});`); }
  for (const [role, [name, alpha]] of Object.entries(tokens.alpha)) {
    const [r, g, b] = [1, 3, 5].map((i) => parseInt(hex(name).slice(i, i + 2), 16));
    lines.push(`  --color-${role}: rgb(${r} ${g} ${b} / ${alpha});`);
  }
  for (const [group, values] of [['font', tokens.font], ['weight', tokens.weight], ['space', tokens.space], ['motion', tokens.motion]]) {
    for (const [key, value] of Object.entries(values)) lines.push(`  --${group}-${key}: ${value};`);
  }
  lines.push('}', '', '@media (prefers-reduced-motion: reduce) {', '  :root {');
  for (const key of Object.keys(tokens.motion)) if (key !== 'curve') lines.push(`    --motion-${key}: 0ms;`);
  lines.push('  }', '}', '');
  return lines.join('\n');
}

const RAW_COLOR = /#[0-9a-fA-F]{3,8}\b|\b(?:rgba?|hsla?)\(|gradient\(/;
export function rawColorFindings(dir = join(root, 'src')) {
  const findings = [];
  for (const entry of readdirSync(dir, { withFileTypes: true, recursive: true })) {
    if (!entry.isFile() || !/\.(css|tsx?|html)$/.test(entry.name)) continue;
    const file = join(entry.parentPath, entry.name);
    if (file === cssPath) continue;
    readFileSync(file, 'utf8').split('\n').forEach((line, i) => {
      if (RAW_COLOR.test(line)) findings.push(`${relative(root, file)}:${i + 1}: raw color or gradient outside src/tokens.css`);
    });
  }
  return findings;
}

if (process.argv[1] === new URL(import.meta.url).pathname) {
  const css = buildCss(JSON.parse(readFileSync(tokensPath, 'utf8')));
  if (process.argv.includes('--check')) {
    let current = '';
    try { current = readFileSync(cssPath, 'utf8'); } catch { /* missing file counts as drift */ }
    const problems = [...(current === css ? [] : ['src/tokens.css drifted from design/tokens.json; run npm run tokens:build']), ...rawColorFindings()];
    for (const problem of problems) console.error(problem);
    if (problems.length) process.exit(1);
    console.log('tokens ok: src/tokens.css matches design/tokens.json; no raw colors in src');
  } else {
    writeFileSync(cssPath, css);
    console.log('wrote src/tokens.css');
  }
}
