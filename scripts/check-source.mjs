import { readdir, readFile } from 'node:fs/promises';
import * as path from 'node:path';

const roots = ['src', 'electron', 'tests'];
const forbidden = [
  { pattern: /@ts-ignore\b|@ts-expect-error\b|@ts-nocheck\b/, reason: 'type suppression' },
  { pattern: /eslint-disable/, reason: 'lint suppression' },
  { pattern: /\b(?:it|test|describe)\.(?:only|skip)\b|\bxit\b|\bxdescribe\b/, reason: 'focused or skipped test' },
  { pattern: /\b(?:TODO|TBD)\b|Lorem ipsum/, reason: 'placeholder text' },
];

async function files(dir) {
  const entries = await readdir(dir, { withFileTypes: true });
  const found = [];
  for (const entry of entries) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      found.push(...(await files(full)));
    } else if (/\.(?:ts|tsx|mjs|css)$/.test(entry.name)) {
      found.push(full);
    }
  }
  return found;
}

const failures = [];
for (const root of roots) {
  for (const file of await files(root)) {
    const text = await readFile(file, 'utf8');
    const lines = text.split('\n');
    lines.forEach((line, index) => {
      for (const rule of forbidden) {
        if (rule.pattern.test(line)) {
          failures.push(`${file}:${index + 1} ${rule.reason}: ${line.trim()}`);
        }
      }
    });
  }
}

if (failures.length > 0) {
  console.error(failures.join('\n'));
  process.exit(1);
}

console.log(`Checked ${roots.join(', ')} for suppressions, skipped tests, and placeholders.`);
