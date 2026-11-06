#!/usr/bin/env node
/**
 * Read a rendered PNG back as pixels, so a screenshot can be judged without an
 * image-capable model.
 *
 * Adapted from /home/cayleb/.dsh-fleet-state/missions/m-0035/work/inspect-render.mjs
 * (its zlib inflate and the five PNG scanline filters). Dependency-free: node's
 * `zlib` is the only decoder used.
 *
 * Prints:
 *   1. the header: size, background colour, distinct colours, non-background %;
 *   2. an ASCII luminance downsample, so composition is readable as text;
 *   3. a max-luminance downsample, so thin ink is visible.
 *
 * Usage:
 *   node scripts/inspect-png.mjs <file.png> [--cols N] [--rows N] [--json]
 *
 * Exits 0 on success, 1 when the PNG cannot be decoded.
 */
import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { inflateSync } from 'node:zlib';

const PNG_SIGNATURE = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
const RAMP = ' .:-=+*#%@';

/** Decode an 8-bit, non-interlaced PNG into { width, height, pixels } (RGB triples). */
export function decodePng(buffer) {
  if (!Buffer.isBuffer(buffer)) buffer = Buffer.from(buffer);
  if (buffer.length < 8 || !buffer.subarray(0, 8).equals(PNG_SIGNATURE)) {
    throw new Error('not a PNG (bad signature)');
  }
  let offset = 8;
  let header = null;
  const idat = [];
  while (offset + 8 <= buffer.length) {
    const length = buffer.readUInt32BE(offset);
    const type = buffer.toString('ascii', offset + 4, offset + 8);
    const body = buffer.subarray(offset + 8, offset + 8 + length);
    if (type === 'IHDR') {
      header = {
        width: body.readUInt32BE(0),
        height: body.readUInt32BE(4),
        bitDepth: body[8],
        colorType: body[9],
        interlace: body[12],
      };
    } else if (type === 'IDAT') {
      idat.push(body);
    } else if (type === 'IEND') {
      break;
    }
    offset += 12 + length;
  }
  if (header === null) throw new Error('no IHDR chunk');
  if (header.bitDepth !== 8) throw new Error(`unsupported bit depth ${header.bitDepth}`);
  if (header.interlace !== 0) throw new Error('interlaced PNG not supported');
  const channels = header.colorType === 2 ? 3 : header.colorType === 6 ? 4 : header.colorType === 0 ? 1 : header.colorType === 4 ? 2 : 0;
  if (channels === 0) throw new Error(`unsupported colour type ${header.colorType}`);

  const raw = inflateSync(Buffer.concat(idat));
  const { width, height } = header;
  const stride = width * channels;
  const out = Buffer.alloc(stride * height);

  const paeth = (a, b, c) => {
    const p = a + b - c;
    const pa = Math.abs(p - a);
    const pb = Math.abs(p - b);
    const pc = Math.abs(p - c);
    if (pa <= pb && pa <= pc) return a;
    return pb <= pc ? b : c;
  };

  let pos = 0;
  for (let y = 0; y < height; y++) {
    if (pos + 1 + stride > raw.length) throw new Error(`truncated scanline ${y}`);
    const filter = raw[pos++];
    const line = raw.subarray(pos, pos + stride);
    pos += stride;
    const target = out.subarray(y * stride, (y + 1) * stride);
    const prior = y > 0 ? out.subarray((y - 1) * stride, y * stride) : null;
    for (let x = 0; x < stride; x++) {
      const rawByte = line[x];
      const a = x >= channels ? target[x - channels] : 0;
      const b = prior === null ? 0 : prior[x];
      const c = prior === null || x < channels ? 0 : prior[x - channels];
      let value;
      switch (filter) {
        case 0: value = rawByte; break;
        case 1: value = rawByte + a; break;
        case 2: value = rawByte + b; break;
        case 3: value = rawByte + ((a + b) >> 1); break;
        case 4: value = rawByte + paeth(a, b, c); break;
        default: throw new Error(`unknown filter ${filter} on row ${y}`);
      }
      target[x] = value & 0xff;
    }
  }

  const pixels = Buffer.alloc(width * height * 3);
  for (let i = 0; i < width * height; i++) {
    const src = i * channels;
    const dst = i * 3;
    if (channels === 1) {
      pixels[dst] = out[src];
      pixels[dst + 1] = out[src];
      pixels[dst + 2] = out[src];
    } else if (channels === 2) {
      pixels[dst] = out[src];
      pixels[dst + 1] = out[src];
      pixels[dst + 2] = out[src];
    } else {
      pixels[dst] = out[src];
      pixels[dst + 1] = out[src + 1];
      pixels[dst + 2] = out[src + 2];
    }
  }
  return { width, height, pixels };
}

function hexOf(pixels, i) {
  return `#${pixels[i * 3].toString(16).padStart(2, '0')}${pixels[i * 3 + 1].toString(16).padStart(2, '0')}${pixels[i * 3 + 2].toString(16).padStart(2, '0')}`;
}

/**
 * Decode and measure one PNG.
 * Returns { width, height, distinctColours, background, nonBackgroundPct }.
 * `background` is the most common exact RGB triple; `nonBackgroundPct` is the
 * share of pixels that differ from it.
 */
export function inspectPng(bufferOrPath) {
  const buffer = typeof bufferOrPath === 'string' || bufferOrPath instanceof URL
    ? readFileSync(bufferOrPath)
    : Buffer.from(bufferOrPath);
  const { width, height, pixels } = decodePng(buffer);
  const total = width * height;
  const counts = new Map();
  for (let i = 0; i < total; i++) {
    const hex = hexOf(pixels, i);
    counts.set(hex, (counts.get(hex) ?? 0) + 1);
  }
  let background = '#000000';
  let backgroundCount = 0;
  for (const [hex, count] of counts) {
    if (count > backgroundCount) {
      background = hex;
      backgroundCount = count;
    }
  }
  return {
    width,
    height,
    distinctColours: counts.size,
    background,
    nonBackgroundPct: total === 0 ? 0 : ((total - backgroundCount) / total) * 100,
  };
}

/** Luminance buffer and its range, used by the ASCII downsample. */
function luminance(pixels) {
  const lum = new Float64Array(pixels.length / 3);
  let min = 255;
  let max = 0;
  for (let i = 0; i < lum.length; i++) {
    const l = 0.2126 * pixels[i * 3] + 0.7152 * pixels[i * 3 + 1] + 0.0722 * pixels[i * 3 + 2];
    lum[i] = l;
    if (l < min) min = l;
    if (l > max) max = l;
  }
  return { lum, min, max };
}

function asciiPanel(lum, width, height, cols, rows, mode, min, max) {
  const cellW = width / cols;
  const cellH = height / rows;
  const lines = [];
  for (let r = 0; r < rows; r++) {
    let line = '';
    for (let c = 0; c < cols; c++) {
      const x0 = Math.floor(c * cellW);
      const x1 = Math.max(x0 + 1, Math.floor((c + 1) * cellW));
      const y0 = Math.floor(r * cellH);
      const y1 = Math.max(y0 + 1, Math.floor((r + 1) * cellH));
      let sum = 0;
      let count = 0;
      let peak = 0;
      for (let y = y0; y < y1; y++) {
        for (let x = x0; x < x1; x++) {
          const l = lum[y * width + x];
          sum += l;
          count++;
          if (l > peak) peak = l;
        }
      }
      const value = mode === 'max' ? peak : sum / Math.max(1, count);
      const t = max === min ? 0 : (value - min) / (max - min);
      line += RAMP[Math.min(RAMP.length - 1, Math.round(t * (RAMP.length - 1)))];
    }
    lines.push(line);
  }
  return lines.join('\n');
}

function parseArgs(argv) {
  const options = { file: null, cols: 100, rows: null, json: false };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === '--json') options.json = true;
    else if (arg === '--cols') options.cols = Math.max(8, Number(argv[++i]) || 100);
    else if (arg === '--rows') options.rows = Math.max(4, Number(argv[++i]) || 40);
    else if (arg === '--help' || arg === '-h') options.help = true;
    else if (!arg.startsWith('-') && options.file === null) options.file = arg;
    else throw new Error(`unknown argument: ${arg}`);
  }
  return options;
}

function main() {
  let options;
  try {
    options = parseArgs(process.argv.slice(2));
  } catch (error) {
    console.error(`inspect-png: ${error.message}`);
    process.exit(1);
  }
  if (options.help || !options.file) {
    console.log('usage: node scripts/inspect-png.mjs <file.png> [--cols N] [--rows N] [--json]');
    process.exit(options.help ? 0 : 1);
  }
  const file = resolve(options.file);
  let image;
  let stats;
  try {
    const buffer = readFileSync(file);
    image = decodePng(buffer);
    stats = inspectPng(buffer);
  } catch (error) {
    console.error(`inspect-png: cannot decode ${file}: ${error.message}`);
    process.exit(1);
  }

  if (options.json) {
    console.log(JSON.stringify({ file, ...stats }, null, 2));
    return;
  }

  const { width, height, pixels } = image;
  console.log(`image: ${file}`);
  console.log(`size: ${width} x ${height} (${(width * height * 3 / 1024 / 1024).toFixed(2)} MB raw RGB)`);
  console.log(`background: ${stats.background} (most common exact colour)`);
  console.log(`distinct colours: ${stats.distinctColours}`);
  console.log(`non-background: ${stats.nonBackgroundPct.toFixed(2)}%`);
  console.log(`blank: ${stats.distinctColours <= 2 || stats.nonBackgroundPct < 0.1 ? 'YES (SUSPECT)' : 'no — the canvas has painted content'}`);

  const { lum, min, max } = luminance(pixels);
  const cols = options.cols;
  const rows = options.rows ?? Math.max(4, Math.round(cols * (height / width) * 0.5));
  console.log(`\n--- ASCII luminance (avg), ${cols} x ${rows}, ${min.toFixed(0)}..${max.toFixed(0)} ---`);
  console.log(asciiPanel(lum, width, height, cols, rows, 'avg', min, max));
  console.log(`\n--- ASCII luminance (max ink), ${cols} x ${rows} ---`);
  console.log(asciiPanel(lum, width, height, cols, rows, 'max', min, max));
}

const invokedDirectly = process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href;
if (invokedDirectly) {
  main();
}
