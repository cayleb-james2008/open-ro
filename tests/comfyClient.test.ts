import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { readFileSync } from 'node:fs';
import { mkdtemp, readFile, rm, symlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import {
  COMFY_PRESETS,
  buildComfyPlan,
  getComfyHealth,
  normalizeComfyBaseUrl,
  prepareComfyGeneration,
  publicPlan,
  runComfyPlan,
} from '../electron/comfyClient';

type Handler = (req: IncomingMessage, res: ServerResponse, body: string) => void;

const cleanups: Array<() => Promise<void>> = [];

async function startServer(handler: Handler): Promise<string> {
  const server = createServer((req, res) => {
    let body = '';
    req.on('data', (chunk) => {
      body += chunk;
    });
    req.on('end', () => handler(req, res, body));
  });
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', () => resolve()));
  const address = server.address();
  if (!address || typeof address === 'string') {
    throw new Error('Mock server did not expose a port.');
  }
  cleanups.push(() => new Promise<void>((resolve, reject) => server.close((error) => (error ? reject(error) : resolve()))));
  return `http://127.0.0.1:${address.port}`;
}

function json(res: ServerResponse, value: unknown): void {
  res.writeHead(200, { 'content-type': 'application/json' });
  res.end(JSON.stringify(value));
}

function requiredNodes(omit: string[] = []): Record<string, unknown> {
  return Object.fromEntries(COMFY_PRESETS[0].requiredNodes.filter((node) => !omit.includes(node)).map((node) => [node, {}]));
}

const systemStats = {
  system: { comfyui_version: '0.44.0', ram_total: 16 * 1024 ** 3, ram_free: 8 * 1024 ** 3 },
  devices: [{ name: 'CPU', type: 'cpu', vram_total: 16 * 1024 ** 3, vram_free: 8 * 1024 ** 3 }],
};

afterEach(async () => {
  while (cleanups.length > 0) {
    await cleanups.pop()!();
  }
});

describe('ComfyUI health and presets', () => {
  it('reads system stats and object_info into preset readiness', async () => {
    const baseUrl = await startServer((req, res) => {
      if (req.url === '/system_stats') {
        json(res, systemStats);
      } else if (req.url === '/object_info') {
        json(res, requiredNodes());
      } else {
        res.writeHead(404);
        res.end();
      }
    });
    const health = await getComfyHealth(baseUrl);
    expect(health).toMatchObject({ ok: true, baseUrl, nodeCount: 11 });
    expect(health.system?.comfyVersion).toBe('0.44.0');
    expect(health.system?.ramTotalGB).toBe(16);
    expect(health.system?.devices[0]).toMatchObject({ name: 'CPU', type: 'cpu', vramTotalGB: null, vramFreeGB: null });
    expect(health.presets.every((preset) => preset.ready)).toBe(true);
    expect(health.presets.map((preset) => preset.presetId)).toEqual(COMFY_PRESETS.map((preset) => preset.id));
  });

  it('reports missing nodes and refuses to prepare that preset', async () => {
    const baseUrl = await startServer((req, res) => {
      if (req.url === '/system_stats') {
        json(res, systemStats);
      } else {
        json(res, requiredNodes(['SaveAudioAdvanced']));
      }
    });
    const health = await getComfyHealth(baseUrl);
    expect(health.ok).toBe(true);
    expect(health.presets[0]).toMatchObject({ ready: false, missingNodes: ['SaveAudioAdvanced'] });
    await expect(prepareComfyGeneration(baseUrl, { presetId: 'audio-ace-step-1.5-turbo', prompt: 'test' })).rejects.toThrow(/Missing ComfyUI nodes/);
  });

  it('returns an error instead of throwing when ComfyUI is down', async () => {
    const health = await getComfyHealth('http://127.0.0.1:9', 500);
    expect(health.ok).toBe(false);
    expect(health.error).toBeTruthy();
    expect(health.presets).toEqual([]);
  });

  it('does not follow a redirect away from loopback', async () => {
    const remote = await startServer((_req, res) => json(res, systemStats));
    const baseUrl = await startServer((req, res) => {
      if (req.url === '/system_stats') {
        res.writeHead(307, { location: `${remote}/system_stats` });
        res.end();
      } else {
        json(res, requiredNodes());
      }
    });
    const health = await getComfyHealth(baseUrl);
    expect(health.ok).toBe(false);
    expect(health.error).toBeTruthy();
  });

  it('accepts only loopback ComfyUI origins', () => {
    expect(normalizeComfyBaseUrl('http://127.0.0.1:8188/')).toBe('http://127.0.0.1:8188');
    expect(normalizeComfyBaseUrl('http://localhost:8188')).toBe('http://localhost:8188');
    expect(normalizeComfyBaseUrl('http://[::1]:8188')).toBe('http://[::1]:8188');
    expect(() => normalizeComfyBaseUrl('http://192.168.1.20:8188')).toThrow(/loopback/);
    expect(() => normalizeComfyBaseUrl('file:///tmp/comfy')).toThrow(/http/);
  });
});

describe('ComfyUI plans', () => {
  it('freezes the 2026 workflow with model, prompt, duration and seed', () => {
    const plan = buildComfyPlan({
      presetId: 'audio-ace-step-1.5-turbo',
      prompt: 'Late night trap, 808 bass',
      lyrics: '[Verse]\nhello',
      durationSeconds: 10,
      seed: 42,
    });
    expect(plan.workflow['104'].inputs.unet_name).toBe('acestep_v1.5_turbo.safetensors');
    expect(plan.workflow['105'].inputs.clip_name2).toBe('qwen_1.7b_ace15.safetensors');
    expect(plan.workflow['94'].inputs.tags).toBe('Late night trap, 808 bass');
    expect(plan.workflow['94'].inputs.lyrics).toBe('[Verse]\nhello');
    expect(plan.workflow['94'].inputs.duration).toBe(10);
    expect(plan.workflow['98'].inputs.seconds).toBe(10);
    expect(plan.workflow['109'].inputs.value).toBe(42);
    expect(plan.summary).toContain('ACE-Step 1.5 Turbo');
    expect(publicPlan(plan)).not.toHaveProperty('workflow');
  });

  it('validates prompts, presets, defaults and duration bounds', () => {
    expect(() => buildComfyPlan({ presetId: 'missing', prompt: 'x' })).toThrow(/Unknown ComfyUI preset/);
    expect(() => buildComfyPlan({ presetId: 'audio-ace-step-1.5-turbo', prompt: '   ' })).toThrow(/prompt is required/);
    const long = buildComfyPlan({ presetId: 'audio-ace-step-1.5-turbo', prompt: 'x', durationSeconds: 999 });
    expect(long.durationSeconds).toBe(60);
    const short = buildComfyPlan({ presetId: 'audio-ace-step-1.5-turbo', prompt: 'x', durationSeconds: 1 });
    expect(short.durationSeconds).toBe(5);
    expect(short.lyrics).toBe('[Instrumental]');
    expect(short.workflow['94'].inputs.duration).toBe(5);
  });

  it('builds distinct plans with distinct ids and an independent workflow copy', () => {
    const first = buildComfyPlan({ presetId: 'audio-ace-step-1.5-turbo', prompt: 'one' });
    const second = buildComfyPlan({ presetId: 'audio-ace-step-1.5-turbo', prompt: 'two' });
    expect(first.planId).not.toBe(second.planId);
    expect(first.modelId).toBe('ace-step-1.5-turbo');
    first.workflow['94'].inputs.tags = 'mutated';
    expect(second.workflow['94'].inputs.tags).toBe('two');
  });

  it('clamps lyrics and rejects out-of-range seeds', () => {
    const longLyrics = buildComfyPlan({ presetId: 'audio-ace-step-1.5-turbo', prompt: 'x', lyrics: 'y'.repeat(8_001) });
    expect(longLyrics.lyrics).toHaveLength(8_000);
    expect(() => buildComfyPlan({ presetId: 'audio-ace-step-1.5-turbo', prompt: 'x', seed: 1e20 })).toThrow(/Seed/);
    expect(() => buildComfyPlan({ presetId: 'audio-ace-step-1.5-turbo', prompt: 'x', seed: -1 })).toThrow(/Seed/);
    expect(buildComfyPlan({ presetId: 'audio-ace-step-1.5-turbo', prompt: 'x', seed: 0 }).workflow['109'].inputs.value).toBe(0);
  });
});

describe('ComfyUI generation and media sink', () => {
  it('submits a frozen plan, polls history and saves the output inside the sink', async () => {
    let submitted = '';
    const baseUrl = await startServer((req, res, body) => {
      if (req.url === '/system_stats') {
        json(res, systemStats);
      } else if (req.url === '/object_info') {
        json(res, requiredNodes());
      } else if (req.method === 'POST' && req.url === '/prompt') {
        submitted = body;
        json(res, { prompt_id: 'p1' });
      } else if (req.url === '/history/p1') {
        json(res, { p1: { status: { status_str: 'success' }, outputs: { 111: { audio: [{ filename: '../../evil.mp3', subfolder: 'audio', type: 'output' }] } } } });
      } else if (req.url?.startsWith('/view')) {
        res.writeHead(200, { 'content-type': 'audio/mpeg' });
        res.end(Buffer.from('ID3-open-ro-live-audio'));
      } else {
        res.writeHead(404);
        res.end();
      }
    });
    const plan = await prepareComfyGeneration(baseUrl, { presetId: 'audio-ace-step-1.5-turbo', prompt: 'Late night trap', durationSeconds: 10, seed: 7 });
    const sink = await mkdtemp(path.join(tmpdir(), 'openro-media-'));
    cleanups.push(() => rm(sink, { recursive: true, force: true }));
    const result = await runComfyPlan(baseUrl, plan, sink, { timeoutMs: 5_000, pollMs: 10 });
    expect(result.ok).toBe(true);
    expect(result.promptId).toBe('p1');
    expect(result.files).toHaveLength(1);
    const saved = result.files[0];
    expect(path.resolve(saved.path).startsWith(path.resolve(sink))).toBe(true);
    expect(path.basename(saved.path)).toContain('evil.mp3');
    expect(path.basename(saved.path)).not.toContain('..');
    expect(saved.bytes).toBe(Buffer.from('ID3-open-ro-live-audio').length);
    expect(saved.sha256).toMatch(/^[0-9a-f]{64}$/);
    expect(await readFile(saved.path)).toEqual(Buffer.from('ID3-open-ro-live-audio'));
    expect(submitted).toContain('"client_id":"open-ro"');
    expect(submitted).toContain('Late night trap');
    expect(submitted).toContain('acestep_v1.5_turbo.safetensors');
  });

  it('surfaces a ComfyUI error status and refuses unsupported output types', async () => {
    const baseUrl = await startServer((req, res) => {
      if (req.method === 'POST' && req.url === '/prompt') {
        json(res, { prompt_id: 'p1' });
      } else if (req.url === '/history/p1') {
        json(res, { p1: { status: { status_str: 'error' }, outputs: {} } });
      } else {
        res.writeHead(404);
        res.end();
      }
    });
    const plan = buildComfyPlan({ presetId: 'audio-ace-step-1.5-turbo', prompt: 'test' });
    const sink = await mkdtemp(path.join(tmpdir(), 'openro-media-'));
    cleanups.push(() => rm(sink, { recursive: true, force: true }));
    await expect(runComfyPlan(baseUrl, plan, sink, { timeoutMs: 1_000, pollMs: 10 })).rejects.toThrow(/generation error/);

    const badType = await startServer((req, res) => {
      if (req.method === 'POST' && req.url === '/prompt') {
        json(res, { prompt_id: 'p2' });
      } else if (req.url === '/history/p2') {
        json(res, { p2: { status: { status_str: 'success' }, outputs: { 1: { audio: [{ filename: 'evil.exe', subfolder: '', type: 'output' }] } } } });
      } else if (req.url?.startsWith('/view')) {
        res.writeHead(200, { 'content-type': 'application/octet-stream' });
        res.end(Buffer.from('not-allowed'));
      } else {
        res.writeHead(404);
        res.end();
      }
    });
    await expect(runComfyPlan(badType, plan, sink, { timeoutMs: 1_000, pollMs: 10 })).rejects.toThrow(/Unsupported ComfyUI output type/);
  });

  it('rejects an oversized declared output before buffering it', async () => {
    const baseUrl = await startServer((req, res) => {
      if (req.method === 'POST' && req.url === '/prompt') {
        json(res, { prompt_id: 'big' });
      } else if (req.url === '/history/big') {
        json(res, { big: { status: { status_str: 'success' }, outputs: { 1: { audio: [{ filename: 'big.mp3', subfolder: '', type: 'output' }] } } } });
      } else if (req.url?.startsWith('/view')) {
        res.writeHead(200, { 'content-length': String(100 * 1024 * 1024 + 1) });
        res.on('error', () => undefined);
        res.end(Buffer.from('x'));
      } else {
        res.writeHead(404);
        res.end();
      }
    });
    const plan = buildComfyPlan({ presetId: 'audio-ace-step-1.5-turbo', prompt: 'test' });
    const sink = await mkdtemp(path.join(tmpdir(), 'openro-media-'));
    cleanups.push(() => rm(sink, { recursive: true, force: true }));
    await expect(runComfyPlan(baseUrl, plan, sink, { timeoutMs: 1_000, pollMs: 10 })).rejects.toThrow(/larger than 100 MB/);
  });

  it('refuses a symlinked sink', async () => {
    const baseUrl = await startServer((req, res) => {
      if (req.method === 'POST' && req.url === '/prompt') {
        json(res, { prompt_id: 'link' });
      } else if (req.url === '/history/link') {
        json(res, { link: { status: { status_str: 'success' }, outputs: { 1: { audio: [{ filename: 'a.mp3', subfolder: '', type: 'output' }] } } } });
      } else if (req.url?.startsWith('/view')) {
        res.writeHead(200, { 'content-type': 'audio/mpeg' });
        res.end(Buffer.from('audio'));
      } else {
        res.writeHead(404);
        res.end();
      }
    });
    const real = await mkdtemp(path.join(tmpdir(), 'openro-media-real-'));
    const link = `${real}-link`;
    await symlink(real, link, process.platform === 'win32' ? 'junction' : 'dir');
    cleanups.push(() => rm(real, { recursive: true, force: true }));
    cleanups.push(() => rm(link, { recursive: true, force: true }));
    const plan = buildComfyPlan({ presetId: 'audio-ace-step-1.5-turbo', prompt: 'test' });
    await expect(runComfyPlan(baseUrl, plan, link, { timeoutMs: 1_000, pollMs: 10 })).rejects.toThrow(/must not be a symlink/);
  });

  it('times out without inventing an output', async () => {
    const baseUrl = await startServer((req, res) => {
      if (req.method === 'POST' && req.url === '/prompt') {
        json(res, { prompt_id: 'slow' });
      } else {
        json(res, {});
      }
    });
    const plan = buildComfyPlan({ presetId: 'audio-ace-step-1.5-turbo', prompt: 'test' });
    const sink = await mkdtemp(path.join(tmpdir(), 'openro-media-'));
    cleanups.push(() => rm(sink, { recursive: true, force: true }));
    await expect(runComfyPlan(baseUrl, plan, sink, { timeoutMs: 60, pollMs: 10 })).rejects.toThrow(/timed out/);
  });

  it('is wired through IPC, the preload allowlist and the api', () => {
    const main = readFileSync('electron/main.ts', 'utf8');
    expect(main).toContain("ipcMain.handle('media:comfy-status'");
    expect(main).toContain("ipcMain.handle('media:prepare'");
    expect(main).toContain("ipcMain.handle('media:generate'");
    expect(main).toContain("path.join(app.getPath('userData'), 'media')");
    expect(main.match(/mediaPlans\.delete\(planId\)/g)).toHaveLength(2);
    expect(main).toContain('normalizeComfyBaseUrl');
    const preload = readFileSync('electron/preload.ts', 'utf8');
    expect(preload).toContain("'media:comfy-status'");
    expect(preload).toContain("'media:prepare'");
    expect(preload).toContain("'media:generate'");
    const api = readFileSync('src/api.ts', 'utf8');
    expect(api).toContain('comfyStatus');
    expect(api).toContain('prepareMedia');
    expect(api).toContain('generateMedia');
  });
});
