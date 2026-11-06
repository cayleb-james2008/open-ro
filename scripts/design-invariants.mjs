#!/usr/bin/env node
/**
 * Visual-system invariants for the rebuilt Open-Ro front end.
 *
 * The rebuilt surface claims to follow the fleet FRONTEND-LIBRARY standard (the Square UI house
 * grammar, Motion Primitives and Kitchen sources carried in src/house/). Those claims are only
 * worth something if the rendered page actually shows them, so this gate reads the LIVE DOM of
 * the dev comps and fails when a promise drifts:
 *
 *   - Geist Sans is the UI face and body text is the 14 px reading size;
 *   - the type scale is the house scale (12 / 13 / 14 / 24 / 30) — no stray sizes;
 *   - the chrome is monochrome: every visible chrome background is a zero/low-chroma neutral,
 *     and a saturated colour appears only on an element carrying a state (pill, badge, diff,
 *     test result, status tag);
 *   - no gradients anywhere, and no box-shadow outside a floating dialog;
 *   - the shell geometry: 248 px rail, 48 px topbar, a per-view h1 (30 px welcome / 24 px page);
 *   - no horizontal overflow in any view;
 *   - the real library components are mounted: the Motion Primitives word morph and panel, the
 *     Kitchen ticket and tactile button, the house Button on a page header, and the
 *     AnimatedNumber-driven section count;
 *   - the approval tray never eats the conversation: the chat scroll stays >= 240 px with two
 *     pending approvals and both Kitchen tickets render.
 *
 * Requires the vite dev server (the same one `npm run verify:ui` uses).
 * Usage:  node scripts/design-invariants.mjs [--json <file>]
 */
import { spawn } from 'node:child_process';
import { mkdir, writeFile } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const BASE_URL = (process.env.OPENRO_BASE_URL || 'http://127.0.0.1:5173').replace(/\/+$/, '');
const CHROME_PATH = process.env.CHROME_PATH || '/usr/bin/chromium';
const HOUSE_SIZES = new Set(['12px', '13px', '14px', '24px', '30px']);
const DATA_CLASS = /(pill|badge|status-dot|status-tag|diff-|test-result|state-pill|active-tag|progress-bar-fill|ticket-notch|server-error-text)/;

const failures = [];
const fail = (check, detail) => failures.push(`${check}: ${detail}`);

const port = 9360 + (process.pid % 20);
const profile = `/tmp/openro-design-invariants-${process.pid}`;
const chrome = spawn(CHROME_PATH, [
  '--headless=new', '--no-sandbox', '--disable-gpu',
  `--user-data-dir=${profile}`, `--remote-debugging-port=${port}`, 'about:blank',
], { stdio: 'ignore' });

let nextId = 0;
let ws = null;
const send = (method, params = {}) => new Promise((res, rej) => {
  const id = ++nextId;
  const onMessage = (event) => {
    const payload = JSON.parse(event.data);
    if (payload.id !== id) return;
    ws.removeEventListener('message', onMessage);
    if (payload.error) rej(new Error(payload.error.message));
    else res(payload.result);
  };
  ws.addEventListener('message', onMessage);
  ws.send(JSON.stringify({ id, method, params }));
});
async function connect() {
  for (let attempt = 0; attempt < 60; attempt += 1) {
    try {
      const response = await (await fetch(`http://127.0.0.1:${port}/json/new?${encodeURIComponent(BASE_URL)}`, { method: 'PUT' })).json();
      ws = new WebSocket(response.webSocketDebuggerUrl);
      await new Promise((res, rej) => { ws.addEventListener('open', res); ws.addEventListener('error', rej); });
      return;
    } catch { await delay(150); }
  }
  throw new Error('chromium DevTools endpoint did not open');
}
async function evaluate(expression) {
  const result = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
  if (result.exceptionDetails) { const d = result.exceptionDetails; throw new Error(d.exception?.description || d.text || 'evaluation failed'); }
  return result.result.value;
}

const PROBE = `(() => {
  const visible = (el) => { const r = el.getBoundingClientRect(); const s = getComputedStyle(el); return s.display !== 'none' && s.visibility !== 'hidden' && r.width > 0 && r.height > 0; };
  const px = (v) => Math.round(parseFloat(v));
  const sat = (color) => {
    const m = color.match(/rgba?\\(([^)]+)\\)/); if (!m) return 0;
    const p = m[1].split(/[ ,/]+/).filter(Boolean).map(Number);
    if (p.length > 3 && p[3] === 0) return 0;
    const [r, g, b] = [p[0] / 255, p[1] / 255, p[2] / 255];
    const max = Math.max(r, g, b), min = Math.min(r, g, b), l = (max + min) / 2;
    const d = max - min;
    return d === 0 ? 0 : d / (1 - Math.abs(2 * l - 1));
  };
  const all = [...document.querySelectorAll('*')].filter(visible);
  const textEls = all.filter((el) => [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim()));
  const sizes = [...new Set(textEls.map((el) => getComputedStyle(el).fontSize))];
  const grads = all.filter((el) => getComputedStyle(el).backgroundImage !== 'none').map((el) => el.className || el.tagName);
  const shadows = all.filter((el) => getComputedStyle(el).boxShadow !== 'none' && !el.closest('[role="dialog"]')).map((el) => el.className || el.tagName);
  const colouredChrome = all.filter((el) => {
    if (el.closest('[role="dialog"]')) return false;
    const cls = typeof el.className === 'string' ? el.className : '';
    if (/(pill|badge|status-dot|status-tag|diff-|test-result|state-pill|active-tag|progress-bar-fill|ticket-notch|server-error-text)/.test(cls)) return false;
    return sat(getComputedStyle(el).backgroundColor) > 0.12;
  }).map((el) => String(el.className || el.tagName).slice(0, 60));
  const box = (sel) => { const el = document.querySelector(sel); if (!el) return null; const r = el.getBoundingClientRect(); return { w: Math.round(r.width), h: Math.round(r.height) }; };
  return {
    bodyFont: getComputedStyle(document.body).fontFamily.split(',')[0].replace(/"/g, ''),
    bodySize: getComputedStyle(document.body).fontSize,
    sizes,
    grads, shadows, colouredChrome,
    overflowX: document.documentElement.scrollWidth > document.documentElement.clientWidth + 1,
    rail: box('.rail'), topbar: box('.topbar'),
    h1: [...document.querySelectorAll('h1')].map((el) => ({ text: el.textContent.trim().slice(0, 40), size: getComputedStyle(el).fontSize })),
    morph: document.querySelectorAll('[data-motion="morph"]').length,
    panel: document.querySelectorAll('[data-motion="panel"]').length,
    tactile: document.querySelectorAll('.tactile').length,
    ticket: document.querySelectorAll('.ticket').length,
    houseButton: [...document.querySelectorAll('.heading-actions button')].filter((el) => typeof el.className === 'string' && el.className.includes('inline-flex')).length,
    animatedCount: [...document.querySelectorAll('.section-title span')].some((el) => /^\\d+ (top-level entries|servers)$/.test(el.textContent.trim())),
    chatScroll: (() => { const el = document.querySelector('.chat-scroll'); return el ? el.clientHeight : null; })(),
    messageRows: document.querySelectorAll('.message-row').length,
  };
})()`;

const VIEWS = [
  { name: 'home', route: '/comps/home', key: null },
  { name: 'models', route: '/comps/models', key: 2 },
  { name: 'workspace', route: '/comps/workspace', key: 3 },
  { name: 'integrations', route: '/comps/integrations', key: 4 },
  { name: 'chat-approvals', route: '/comps/chat-approvals', key: null },
];

const results = [];
try {
  await connect();
  await send('Page.enable');
  await send('Runtime.enable');
  await send('Emulation.setDeviceMetricsOverride', { width: 1440, height: 900, deviceScaleFactor: 1, mobile: false });
  for (const view of VIEWS) {
    await send('Page.navigate', { url: `${BASE_URL}${view.route}` });
    await delay(1100);
    if (view.key !== null) {
      const digit = String(view.key);
      await send('Input.dispatchKeyEvent', { type: 'keyDown', modifiers: 2, key: digit, code: `Digit${digit}` });
      await send('Input.dispatchKeyEvent', { type: 'keyUp', modifiers: 2, key: digit, code: `Digit${digit}` });
      await delay(650);
    }
    const p = await evaluate(PROBE);
    results.push({ view: view.name, ...p });

    if (!p.bodyFont.startsWith('Geist')) fail('font', `${view.name}: body font is ${p.bodyFont}, not Geist`);
    if (p.bodySize !== '14px') fail('font-size', `${view.name}: body font size is ${p.bodySize}, not 14px`);
    const stray = p.sizes.filter((s) => !HOUSE_SIZES.has(s));
    if (stray.length) fail('type-scale', `${view.name}: sizes outside the house scale: ${stray.join(', ')}`);
    if (p.grads.length) fail('gradient', `${view.name}: ${p.grads.length} element(s) paint a background image: ${p.grads.slice(0, 4).join(' | ')}`);
    if (p.shadows.length) fail('shadow', `${view.name}: box-shadow outside a dialog: ${p.shadows.slice(0, 4).join(' | ')}`);
    if (p.colouredChrome.length) fail('chrome-colour', `${view.name}: saturated background on chrome: ${p.colouredChrome.slice(0, 4).join(' | ')}`);
    if (p.overflowX) fail('overflow', `${view.name}: horizontal overflow at 1440x900`);
    if (!p.rail || Math.abs(p.rail.w - 248) > 2) fail('shell', `${view.name}: rail width ${p.rail ? p.rail.w : 'missing'} (expected 248)`);
    if (!p.topbar || Math.abs(p.topbar.h - 48) > 2) fail('shell', `${view.name}: topbar height ${p.topbar ? p.topbar.h : 'missing'} (expected 48)`);
    if (p.morph < 1) fail('motion', `${view.name}: the Motion Primitives word morph is not mounted`);
    if (p.panel < 1) fail('motion', `${view.name}: the Motion Primitives panel is not mounted`);

    if (view.name === 'home') {
      if (!p.h1.some((h) => h.size === '30px')) fail('heading', 'home: welcome h1 is not the 30px house display size');
      if (p.tactile < 1) fail('kitchen', 'home: the Kitchen tactile button is not mounted');
    } else if (view.name !== 'chat-approvals') {
      if (!p.h1.some((h) => h.size === '24px')) fail('heading', `${view.name}: page h1 is not the 24px house heading size`);
    }
    if (view.name === 'workspace') {
      if (p.houseButton < 1) fail('house-button', 'workspace: the page header does not use the house Button');
      if (!p.animatedCount) fail('animated-number', 'workspace: the AnimatedNumber section count is not mounted');
    }
    if (view.name === 'integrations') {
      if (!p.animatedCount) fail('animated-number', 'integrations: the AnimatedNumber section count is not mounted');
    }
    if (view.name === 'chat-approvals') {
      if (p.ticket < 2) fail('kitchen', `chat-approvals: ${p.ticket} Kitchen ticket(s), expected 2`);
      if (!(p.chatScroll >= 240)) fail('approvals-layout', `chat-approvals: the conversation keeps only ${p.chatScroll}px with 2 approvals (expected >= 240)`);
      if (p.messageRows < 2) fail('chat', `chat-approvals: ${p.messageRows} message rows, expected a conversation`);
    }
  }
} catch (error) {
  fail('run', error instanceof Error ? error.message : String(error));
} finally {
  try { ws?.close(); } catch { /* closed */ }
  chrome.kill();
}

const report = { baseUrl: BASE_URL, views: results, failures, pass: failures.length === 0 };
const jsonAt = process.argv.indexOf('--json');
if (jsonAt !== -1 && process.argv[jsonAt + 1]) {
  const out = resolve(REPO_ROOT, process.argv[jsonAt + 1]);
  await mkdir(dirname(out), { recursive: true });
  await writeFile(out, `${JSON.stringify(report, null, 2)}\n`);
}
if (failures.length) {
  console.error(failures.join('\n'));
  process.exitCode = 1;
} else {
  console.log(`[PASS] design invariants: ${VIEWS.length} views, house type scale/font, monochrome chrome, no gradients/shadows, shell geometry, real Motion Primitives + Kitchen + house primitives mounted, approvals keep the conversation.`);
}
