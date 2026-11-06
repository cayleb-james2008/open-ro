#!/usr/bin/env node
/**
 * compare-png.mjs — are two renders of the same view the same artefact?
 *
 * The committed screenshots are meant to be reproducible: a cold viewer re-running
 * `node scripts/capture-views.mjs` should get the same picture. Chromium's rasteriser is not
 * guaranteed byte-identical across runs (sub-pixel antialiasing can move a channel by one), so the
 * comparison is a pixel tolerance rather than a byte comparison:
 *
 *   - the two images must have the same dimensions;
 *   - at most `--limit` percent of pixels may differ by more than `--delta` channel values.
 *
 * Defaults: --limit 0.1 (percent), --delta 2 (of 255). Anything larger is a different picture.
 *
 * Usage:  node scripts/compare-png.mjs <committed.png> <fresh.png> [--limit 0.1] [--delta 2] [--json]
 * Exit:   0 when the images match within tolerance, 1 when they do not.
 */
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { decodePng } from './inspect-png.mjs';

function parseArgs(argv) {
  const options = { files: [], limit: 0.1, delta: 2, json: false };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    if (arg === '--limit') options.limit = Number(argv[++i]);
    else if (arg === '--delta') options.delta = Number(argv[++i]);
    else if (arg === '--json') options.json = true;
    else options.files.push(arg);
  }
  if (options.files.length !== 2 || !Number.isFinite(options.limit) || !Number.isFinite(options.delta)) {
    throw new Error('usage: node scripts/compare-png.mjs <committed.png> <fresh.png> [--limit 0.1] [--delta 2] [--json]');
  }
  return options;
}

let options;
try {
  options = parseArgs(process.argv.slice(2));
} catch (error) {
  console.error(`compare-png: ${error.message}`);
  process.exit(1);
}

let first;
let second;
try {
  first = decodePng(readFileSync(resolve(options.files[0])));
  second = decodePng(readFileSync(resolve(options.files[1])));
} catch (error) {
  console.error(`compare-png: cannot decode input: ${error.message}`);
  process.exit(1);
}

if (first.width !== second.width || first.height !== second.height) {
  console.error(`compare-png: dimensions differ (${first.width}x${first.height} vs ${second.width}x${second.height})`);
  process.exit(1);
}

const total = first.width * first.height;
let differing = 0;
let worst = 0;
for (let i = 0; i < total * 3; i += 1) {
  const delta = Math.abs(first.pixels[i] - second.pixels[i]);
  if (delta > options.delta) differing += 1;
  if (delta > worst) worst = delta;
}
const differingPixels = Math.ceil(differing / 3);
const pct = (100 * differingPixels) / total;
const result = {
  committed: resolve(options.files[0]),
  fresh: resolve(options.files[1]),
  width: first.width,
  height: first.height,
  differingPixels,
  totalPixels: total,
  differingPct: Number(pct.toFixed(4)),
  worstChannelDelta: worst,
  limitPct: options.limit,
};
if (options.json) console.log(JSON.stringify(result, null, 2));
else console.log(`compare: ${differingPixels}/${total} pixels differ (${result.differingPct}%), worst channel delta ${worst}, limit ${options.limit}%`);
process.exit(pct <= options.limit ? 0 : 1);
