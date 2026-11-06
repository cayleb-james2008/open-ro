#!/usr/bin/env node
/**
 * Capture the four Open-Ro Studio views (plus the chat approval tray) from the
 * dev-only fixture bridge, through the real App component.
 *
 * No npm dependencies: chromium's DevTools protocol is spoken with node's
 * builtin fetch + WebSocket, decoding copied in spirit from
 * scripts/verify-ui-craft.mjs. That gate owns port 9333, so this script picks a
 * free port of its own and never touches 9333.
 *
 * Usage:
 *   node scripts/capture-views.mjs
 *
 * Output:
 *   artifacts/screenshots/{chat,models,workspace,integrations,chat-approvals}.png
 *   artifacts/screenshots/manifest.json
 *
 * Exit 0 only when every PNG is >= 20 KB, 1440x900, has >= 8 distinct colours,
 * is >= 1% non-background, and the page did not render an error / empty root.
 */
import { spawn } from 'node:child_process';
import { createHash } from 'node:crypto';
import { existsSync, rmSync } from 'node:fs';
import { mkdir, writeFile } from 'node:fs/promises';
import { createServer } from 'node:net';
import { dirname, join, relative, resolve } from 'node:path';
import { setTimeout as delay } from 'node:timers/promises';
import { fileURLToPath } from 'node:url';
import { inspectPng } from './inspect-png.mjs';

const REPO_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const BASE_URL = (process.env.OPENRO_BASE_URL || 'http://127.0.0.1:5173').replace(/\/+$/, '');
const SHOT_DIR = process.env.OPENRO_SHOT_DIR
  ? resolve(REPO_ROOT, process.env.OPENRO_SHOT_DIR)
  : join(REPO_ROOT, 'docs', 'screenshots');
const CHROME_PATH = process.env.CHROME_PATH || '/usr/bin/chromium';
const CAPTURED_AT = '2026-11-06T05:00:00.000Z';
const WIDTH = 1440;
const HEIGHT = 900;
const MIN_BYTES = 20 * 1024;
const MIN_COLOURS = 8;
const MIN_NON_BACKGROUND_PCT = 1;
const VERIFY_UI_PORT = 9333;

const VIEWS = [
  { view: 'chat', route: '/comps/chat', key: 1, expected: 'Chat' },
  { view: 'models', route: '/comps/models', key: 2, expected: 'Models' },
  { view: 'workspace', route: '/comps/workspace', key: 3, expected: 'Workspace' },
  { view: 'integrations', route: '/comps/integrations', key: 4, expected: 'Integrations' },
  { view: 'chat-approvals', route: '/comps/chat-approvals', key: null, expected: 'Chat' },
];

const failures = [];
const manifestViews = [];
let chrome = null;
let chromeProfileDir = null;
let vite = null;
let viteStartedHere = false;
let ws = null;
let chromeStderr = '';
let nextId = 0;

/** A loopback port nobody holds, excluding the repo's verify:ui gate port. */
function freePort() {
  return new Promise((resolvePort, reject) => {
    const server = createServer();
    server.unref();
    server.on('error', reject);
    server.listen(0, '127.0.0.1', () => {
      const address = server.address();
      server.close(() => {
        const port = typeof address === 'object' && address ? address.port : 0;
        if (!port || port === VERIFY_UI_PORT) {
          freePort().then(resolvePort, reject);
          return;
        }
        resolvePort(port);
      });
    });
  });
}

async function serverUp() {
  try {
    const response = await fetch(`${BASE_URL}/`, { signal: AbortSignal.timeout(2000) });
    return response.ok;
  } catch {
    return false;
  }
}

async function ensureVite() {
  if (await serverUp()) {
    console.log(`[vite] using the dev server already listening on ${BASE_URL}`);
    return;
  }
  const viteBin = join(REPO_ROOT, 'node_modules', 'vite', 'bin', 'vite.js');
  if (!existsSync(viteBin)) {
    throw new Error(`vite dev server is not up and ${viteBin} does not exist; start npm run dev:renderer manually.`);
  }
  console.log(`[vite] starting ${viteBin}`);
  vite = spawn(process.execPath, [viteBin], {
    cwd: REPO_ROOT,
    detached: true,
    stdio: 'ignore',
  });
  viteStartedHere = true;
  for (let attempt = 0; attempt < 480; attempt += 1) {
    if (await serverUp()) {
      console.log(`[vite] ready on ${BASE_URL}`);
      return;
    }
    await delay(250);
  }
  throw new Error(`vite did not answer on ${BASE_URL} within 120s.`);
}

async function connect() {
  const port = await freePort();
  chromeProfileDir = `/tmp/openro-capture-${process.pid}`;
  rmSync(chromeProfileDir, { recursive: true, force: true });
  if (!existsSync(CHROME_PATH)) {
    throw new Error(`chromium not found at ${CHROME_PATH}; set CHROME_PATH.`);
  }
  chrome = spawn(CHROME_PATH, [
    '--headless=new',
    '--no-sandbox',
    '--disable-gpu',
    `--user-data-dir=${chromeProfileDir}`,
    `--remote-debugging-port=${port}`,
    '--hide-scrollbars',
    '--disable-dev-shm-usage',
    '--no-first-run',
    '--force-device-scale-factor=1',
    `--window-size=${WIDTH},${HEIGHT}`,
    'about:blank',
  ], { detached: true, stdio: ['ignore', 'ignore', 'pipe'] });
  chrome.stderr?.on('data', (chunk) => {
    chromeStderr = `${chromeStderr}${chunk}`.slice(-8000);
  });
  chrome.on('exit', (code) => {
    if (code !== null && code !== 0) {
      console.error(`[chromium] exited with code ${code}`);
    }
  });

  let page = null;
  for (let attempt = 0; attempt < 160; attempt += 1) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json/new?${encodeURIComponent('about:blank')}`, {
        method: 'PUT',
        signal: AbortSignal.timeout(2000),
      });
      if (response.ok) {
        page = await response.json();
        break;
      }
    } catch {
      // DevTools endpoint not up yet.
    }
    await delay(150);
  }
  if (page === null) {
    throw new Error(`chromium DevTools endpoint did not open on port ${port}.\n${chromeStderr.slice(-2000)}`);
  }
  ws = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((open, reject) => {
    ws.addEventListener('open', open, { once: true });
    ws.addEventListener('error', () => reject(new Error('DevTools WebSocket failed to open.')), { once: true });
  });
  console.log(`[chromium] pid ${chrome.pid}, CDP port ${port}`);
}

function send(method, params = {}) {
  const id = ++nextId;
  return new Promise((resolveSend, reject) => {
    const timer = setTimeout(() => {
      ws.removeEventListener('message', onMessage);
      reject(new Error(`CDP ${method} timed out after 20s.`));
    }, 20_000);
    const onMessage = (event) => {
      let payload;
      try {
        payload = JSON.parse(event.data);
      } catch {
        return;
      }
      if (payload.id !== id) return;
      clearTimeout(timer);
      ws.removeEventListener('message', onMessage);
      if (payload.error) reject(new Error(`${method}: ${payload.error.message}`));
      else resolveSend(payload.result);
    };
    ws.addEventListener('message', onMessage);
    ws.send(JSON.stringify({ id, method, params }));
  });
}

async function evaluate(expression) {
  const result = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
  if (result.exceptionDetails) {
    throw new Error(result.exceptionDetails.text || result.exceptionDetails.exception?.description || 'Page evaluation failed.');
  }
  return result.result.value;
}

async function waitFor(expression, label, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  let lastError = null;
  while (Date.now() < deadline) {
    try {
      if (await evaluate(expression)) return;
    } catch (error) {
      lastError = error;
    }
    await delay(100);
  }
  throw new Error(`timed out waiting for ${label}${lastError ? ` (${lastError.message})` : ''}`);
}

async function dispatchKey(key) {
  const params = {
    modifiers: 2, // Ctrl
    key: String(key),
    code: `Digit${key}`,
    windowsVirtualKeyCode: 48 + key,
    nativeVirtualKeyCode: 48 + key,
  };
  await send('Input.dispatchKeyEvent', { type: 'keyDown', ...params });
  await send('Input.dispatchKeyEvent', { type: 'keyUp', ...params });
}

async function navState() {
  return evaluate(`(() => {
    const current = document.querySelector('.rail-item[aria-current="page"]') || document.querySelector('.rail-item.is-current');
    const heading = document.querySelector('.content-page h1');
    return {
      nav: (current?.textContent || '').trim(),
      heading: (heading?.textContent || '').trim(),
      navCount: document.querySelectorAll('.rail-item').length,
      rootChildren: document.getElementById('root')?.children.length ?? 0,
      hasRail: Boolean(document.querySelector('.rail') || document.querySelector('.app')),
      errorText: /Something went wrong|The studio hit a renderer error/i.test(document.body?.innerText || ''),
      bodyTextLength: (document.body?.innerText || '').trim().length,
    };
  })()`);
}

async function switchView(view) {
  if (view.key === null) {
    // /comps/chat opens on Chat already; this capture is for the approval tray.
    return { method: 'default', state: await navState() };
  }
  await evaluate(`(() => { document.activeElement?.blur?.(); return true; })()`);
  await dispatchKey(view.key);
  await delay(250);
  let state = await navState();
  let method = 'keyboard';
  const matched = (value) => value.nav === view.expected || value.heading.toLowerCase().includes(view.expected.toLowerCase());
  if (!matched(state)) {
    // A rebuild may swallow the shortcut; fall back to the rail button by position.
    console.warn(`[${view.view}] Ctrl+${view.key} did not switch views (nav="${state.nav}", heading="${state.heading}"); clicking rail item ${view.key}.`);
    await evaluate(`(() => {
      const items = document.querySelectorAll('.rail-item');
      const item = items[${view.key} - 1];
      if (item) { item.click(); return Boolean(item); }
      return false;
    })()`);
    await delay(300);
    state = await navState();
    method = 'click';
  }
  if (!matched(state)) {
    failures.push(`${view.view}: view did not switch (nav="${state.nav}", heading="${state.heading}")`);
  }
  return { method, state };
}

async function triggerWorkspaceRefresh(view) {
  if (view.view !== 'workspace') return false;
  const clicked = await evaluate(`(() => {
    const button = Array.from(document.querySelectorAll('.heading-actions button, button'))
      .find((node) => node.textContent?.trim() === 'Refresh');
    if (!button) return false;
    button.click();
    return true;
  })()`);
  if (clicked) {
    await delay(500);
  } else {
    console.warn('[workspace] no Refresh button found; file tree may stay empty for this fixture.');
  }
  return clicked;
}

async function capture(view) {
  const url = `${BASE_URL}${view.route}`;
  await send('Emulation.setDeviceMetricsOverride', {
    width: WIDTH,
    height: HEIGHT,
    deviceScaleFactor: 1,
    mobile: false,
  });
  console.log(`[${view.view}] navigate ${url}`);
  await send('Page.navigate', { url });
  await waitFor(
    `document.readyState === 'complete' && (document.getElementById('root')?.children.length ?? 0) > 0 && Boolean(document.querySelector('.rail') || document.querySelector('.app'))`,
    `${view.view}: #root children and .rail/.app`,
  );
  await evaluate('document.fonts && document.fonts.ready ? document.fonts.ready.then(() => true) : true');

  const switched = await switchView(view);
  await delay(800);
  const refreshed = await triggerWorkspaceRefresh(view);

  // The approvals panel opens over 220ms while the chat auto-scrolls; settle both
  // so two captures of the same fixture do not depend on that race.
  await evaluate(`(() => {
    const chat = document.querySelector('.chat-scroll');
    if (chat) chat.scrollTop = chat.scrollHeight;
    const tray = document.querySelector('.approval-tray');
    if (tray) tray.scrollTop = 0;
    const page = document.querySelector('.content-page');
    if (page) page.scrollTop = 0;
    return true;
  })()`);
  await evaluate(`(() => { document.getAnimations?.().forEach((animation) => { try { animation.finish(); } catch { /* infinite */ } }); return true; })()`);
  await delay(100);

  const state = await navState();
  const dom = await evaluate(`(() => ({
    heading: (document.querySelector('.content-page h1')?.textContent || '').trim(),
    messageRows: document.querySelectorAll('.message-row').length,
    approvalCards: document.querySelectorAll('.approval-card').length,
    providerRows: document.querySelectorAll('.provider-row').length,
    providerChoices: document.querySelectorAll('.provider-choice').length,
    fileTreeRows: document.querySelectorAll('.file-tree .tree-row').length,
    mcpServerCards: document.querySelectorAll('.mcp-server-card').length,
    integrationCards: document.querySelectorAll('.integration-card').length,
    stepLines: document.querySelectorAll('.step-line').length,
  }))()`);
  if (!state.hasRail) failures.push(`${view.view}: .rail/.app missing from the DOM`);
  if (state.rootChildren === 0) failures.push(`${view.view}: #root is empty`);
  if (state.errorText) failures.push(`${view.view}: DOM contains a renderer error`);
  if (state.bodyTextLength === 0) failures.push(`${view.view}: page body has no text`);

  const shot = await send('Page.captureScreenshot', {
    format: 'png',
    fromSurface: true,
    captureBeyondViewport: false,
  });
  const buffer = Buffer.from(shot.data, 'base64');
  const file = join(SHOT_DIR, `${view.view}.png`);
  await writeFile(file, buffer);

  const stats = inspectPng(buffer);
  const entry = {
    view: view.view,
    file: relative(REPO_ROOT, file),
    route: view.route,
    sha256: createHash('sha256').update(buffer).digest('hex'),
    width: stats.width,
    height: stats.height,
    bytes: buffer.length,
    distinctColours: stats.distinctColours,
    background: stats.background,
    nonBackgroundPct: Number(stats.nonBackgroundPct.toFixed(2)),
    capturedAt: CAPTURED_AT,
    switchMethod: switched.method,
    activeNav: state.nav,
    workspaceRefreshClicked: refreshed,
    dom,
  };
  manifestViews.push(entry);

  if (buffer.length < MIN_BYTES) failures.push(`${view.view}: PNG is ${buffer.length} bytes (< ${MIN_BYTES})`);
  if (stats.width !== WIDTH || stats.height !== HEIGHT) failures.push(`${view.view}: ${stats.width}x${stats.height}, expected ${WIDTH}x${HEIGHT}`);
  if (stats.distinctColours < MIN_COLOURS) failures.push(`${view.view}: ${stats.distinctColours} distinct colours (< ${MIN_COLOURS})`);
  if (stats.nonBackgroundPct < MIN_NON_BACKGROUND_PCT) failures.push(`${view.view}: ${stats.nonBackgroundPct.toFixed(2)}% non-background (< ${MIN_NON_BACKGROUND_PCT}%)`);

  console.log(
    `[${view.view}] ${stats.width}x${stats.height} ${(buffer.length / 1024).toFixed(1)} KB · ` +
    `${stats.distinctColours} colours · ${stats.nonBackgroundPct.toFixed(2)}% non-bg · sha256 ${entry.sha256.slice(0, 12)}`,
  );
  return entry;
}

function killTree(child, signal) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return;
  try {
    if (child.pid) process.kill(-child.pid, signal);
  } catch {
    try {
      child.kill(signal);
    } catch {
      // Already gone.
    }
  }
}

async function cleanup() {
  try {
    ws?.close();
  } catch {
    // ignore
  }
  ws = null;
  if (chrome) {
    killTree(chrome, 'SIGTERM');
    await delay(400);
    killTree(chrome, 'SIGKILL');
    chrome = null;
  }
  if (viteStartedHere && vite) {
    killTree(vite, 'SIGTERM');
    await delay(300);
    killTree(vite, 'SIGKILL');
    vite = null;
  }
  if (chromeProfileDir && chromeProfileDir.startsWith('/tmp/openro-capture-')) {
    try {
      rmSync(chromeProfileDir, { recursive: true, force: true });
    } catch {
      // A leftover profile is harmless.
    }
  }
}

process.once('SIGINT', () => {
  void cleanup().finally(() => process.exit(130));
});
process.once('SIGTERM', () => {
  void cleanup().finally(() => process.exit(143));
});

try {
  await mkdir(SHOT_DIR, { recursive: true });
  await ensureVite();
  await connect();
  await send('Page.enable');
  await send('Runtime.enable');
  // The panel/ticket motion is a 220ms open; capture the settled end state.
  await send('Emulation.setEmulatedMedia', {
    media: '',
    features: [{ name: 'prefers-reduced-motion', value: 'reduce' }],
  });
  // Warm the Vite module graph and font cache once, so the first view of a cold
  // browser rasterises the same as the rest.
  await send('Page.navigate', { url: `${BASE_URL}/comps/home` });
  await waitFor(
    `document.readyState === 'complete' && (document.getElementById('root')?.children.length ?? 0) > 0 && Boolean(document.querySelector('.rail') || document.querySelector('.app'))`,
    'warm-up: #root children and .rail/.app',
  );
  await evaluate('document.fonts && document.fonts.ready ? document.fonts.ready.then(() => true) : true');
  await delay(400);
  for (const view of VIEWS) {
    try {
      await capture(view);
    } catch (error) {
      failures.push(`${view.view}: ${error instanceof Error ? error.message : String(error)}`);
      console.error(`[${view.view}] capture failed: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  const manifest = {
    capturedAt: CAPTURED_AT,
    baseUrl: BASE_URL,
    viewport: { width: WIDTH, height: HEIGHT, deviceScaleFactor: 1 },
    views: manifestViews,
    failures,
  };
  const manifestPath = join(SHOT_DIR, 'manifest.json');
  await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(`\n[manifest] ${relative(REPO_ROOT, manifestPath)} (${manifestViews.length} views)`);

  if (failures.length > 0) {
    console.error('\n[FAIL]');
    for (const failure of failures) console.error(`- ${failure}`);
    process.exitCode = 1;
  } else {
    console.log(`[PASS] ${manifestViews.length} views at ${WIDTH}x${HEIGHT}, all non-blank and error-free.`);
  }
} catch (error) {
  console.error(`capture-views: ${error instanceof Error ? error.message : String(error)}`);
  process.exitCode = 1;
} finally {
  await cleanup();
}
