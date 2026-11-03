/**
 * Adapted from OMNI-LOOP v4 Appendix C.1.
 * Uses system Chrome and the DevTools protocol already in Node 22.
 * Does not add a browser dependency.
 */
import { spawn } from 'node:child_process';
import { existsSync } from 'node:fs';
import { mkdir, writeFile } from 'node:fs/promises';
import { setTimeout as delay } from 'node:timers/promises';

const target = process.env.TARGET_URL || 'http://127.0.0.1:5173';
const chromeCandidates = ['/usr/local/bin/google-chrome', '/usr/bin/google-chrome', '/usr/bin/google-chrome-stable', '/usr/bin/chromium', '/usr/bin/chromium-browser'];
const chromePath = process.env.CHROME_PATH || chromeCandidates.find((candidate) => existsSync(candidate));
if (!chromePath) {
  console.error(`No Chrome or Chromium found. Set CHROME_PATH. Checked: ${chromeCandidates.join(', ')}`);
  process.exit(1);
}
const shotDir = process.env.SCREENSHOT_DIR || '.omni-loop/screenshots';
const port = 9333;
const viewports = [
  { name: 'desktop', width: 1440, height: 900 },
  { name: 'tablet', width: 768, height: 1024 },
  { name: 'mobile', width: 375, height: 812 },
];

const chrome = spawn(chromePath, [
  '--headless=new',
  '--no-sandbox',
  '--disable-gpu',
  '--user-data-dir=/tmp/openro-ui-craft',
  `--remote-debugging-port=${port}`,
  'about:blank',
], { stdio: 'ignore' });

let nextId = 0;
let ws;

async function connect() {
  for (let attempt = 0; attempt < 40; attempt += 1) {
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json/new?${encodeURIComponent(target)}`, { method: 'PUT' });
      const page = await response.json();
      ws = new WebSocket(page.webSocketDebuggerUrl);
      await new Promise((resolve, reject) => {
        ws.addEventListener('open', resolve);
        ws.addEventListener('error', reject);
      });
      return;
    } catch {
      await delay(150);
    }
  }
  throw new Error('Chrome DevTools endpoint did not open.');
}

function send(method, params = {}) {
  const id = ++nextId;
  return new Promise((resolve, reject) => {
    const onMessage = (event) => {
      const payload = JSON.parse(event.data);
      if (payload.id !== id) return;
      ws.removeEventListener('message', onMessage);
      if (payload.error) reject(new Error(payload.error.message));
      else resolve(payload.result);
    };
    ws.addEventListener('message', onMessage);
    ws.send(JSON.stringify({ id, method, params }));
  });
}

async function evaluate(expression) {
  const result = await send('Runtime.evaluate', { expression, returnByValue: true, awaitPromise: true });
  if (result.exceptionDetails) {
    throw new Error(result.exceptionDetails.text || 'Page evaluation failed.');
  }
  return result.result.value;
}

try {
  await connect();
  await send('Page.enable');
  await send('Runtime.enable');
  await mkdir(shotDir, { recursive: true });
  const failures = [];
  for (const viewport of viewports) {
    await send('Emulation.setDeviceMetricsOverride', {
      width: viewport.width,
      height: viewport.height,
      deviceScaleFactor: 1,
      mobile: viewport.name === 'mobile',
    });
    await send('Page.navigate', { url: target });
    await delay(800);
    const report = await evaluate(`(() => {
      const errors = [];
      const overflow = document.documentElement.scrollWidth > document.documentElement.clientWidth + 1;
      const tiny = Array.from(document.querySelectorAll('body *')).filter((element) => {
        const text = (element.textContent || '').trim();
        if (!text || element.children.length > 0) return false;
        const size = parseFloat(getComputedStyle(element).fontSize);
        return size > 0 && size < 11;
      }).map((element) => element.className || element.tagName);
      const transitionAll = Array.from(document.querySelectorAll('*')).filter((element) => {
        const style = getComputedStyle(element);
        const duration = parseFloat(style.transitionDuration);
        return style.transitionProperty === 'all' && duration > 0;
      }).length;
      return { overflow, tiny, transitionAll, title: document.querySelector('h1')?.textContent || '' };
    })()`);
    const shot = await send('Page.captureScreenshot', { format: 'png' });
    await writeFile(`${shotDir}/route_${viewport.name}.png`, Buffer.from(shot.data, 'base64'));
    if (report.overflow) failures.push(`${viewport.name}: horizontal overflow`);
    if (viewport.name === 'mobile' && report.tiny.length > 0) failures.push(`mobile text under 11px: ${report.tiny.slice(0, 8).join(', ')}`);
    if (report.transitionAll > 0) failures.push(`${viewport.name}: ${report.transitionAll} elements use transition-property all`);
    if (!report.title) failures.push(`${viewport.name}: missing heading`);
    console.log(`[${viewport.name}] overflow=${report.overflow} tiny=${report.tiny.length} transitionAll=${report.transitionAll} title=${JSON.stringify(report.title)}`);
  }
  if (failures.length > 0) {
    console.error(failures.join('\n'));
    process.exitCode = 1;
  } else {
    console.log('[PASS] UI craft gate: 3 viewports, no overflow, no sub-11px mobile text, no transition-property all.');
  }
} finally {
  chrome.kill();
  ws?.close();
}
