import { createHash } from 'node:crypto';
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import { readFileSync } from 'node:fs';
import { mkdtemp, readFile, realpath, rm, symlink } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { afterEach, describe, expect, it } from 'vitest';
import {
  COMFY_PRESETS,
  buildComfyPlan,
  getComfyHealth,
  imagePresetDeviceReady,
  normalizeComfyBaseUrl,
  planReadyOnHealth,
  prepareComfyGeneration,
  publicPlan,
  runComfyPlan,
} from '../electron/comfyClient';
import { approvalCopy } from '../src/labels';

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
  const all = new Set(COMFY_PRESETS.flatMap((preset) => preset.requiredNodes));
  return Object.fromEntries([...all].filter((node) => !omit.includes(node)).map((node) => [node, {}]));
}

const NODE_COUNT = new Set(COMFY_PRESETS.flatMap((preset) => preset.requiredNodes)).size;

const systemStats = {
  system: { comfyui_version: '0.44.0', ram_total: 16 * 1024 ** 3, ram_free: 8 * 1024 ** 3 },
  devices: [{ name: 'CPU', type: 'cpu', vram_total: 16 * 1024 ** 3, vram_free: 8 * 1024 ** 3 }],
};

const cudaStats = {
  system: { comfyui_version: '0.44.0', ram_total: 32 * 1024 ** 3, ram_free: 24 * 1024 ** 3 },
  devices: [{ name: 'cuda:0 RTX 4080', type: 'cuda', vram_total: 16 * 1024 ** 3, vram_free: 15 * 1024 ** 3 }],
};

const smallGpuStats = {
  system: { comfyui_version: '0.44.0', ram_total: 32 * 1024 ** 3, ram_free: 24 * 1024 ** 3 },
  devices: [{ name: 'cuda:0 RTX 3060', type: 'cuda', vram_total: 12 * 1024 ** 3, vram_free: 11 * 1024 ** 3 }],
};

const edgeGpuStats = {
  system: { comfyui_version: '0.44.0', ram_total: 32 * 1024 ** 3, ram_free: 24 * 1024 ** 3 },
  devices: [{ name: 'cuda:0 13GB', type: 'cuda', vram_total: 13 * 1024 ** 3, vram_free: 12 * 1024 ** 3 }],
};

const mpsStats = {
  system: { comfyui_version: '0.44.0', ram_total: 32 * 1024 ** 3, ram_free: 24 * 1024 ** 3 },
  devices: [{ name: 'mps', type: 'mps', vram_total: 16 * 1024 ** 3, vram_free: 15 * 1024 ** 3 }],
};

const rocmStats = {
  system: { comfyui_version: '0.44.0', ram_total: 32 * 1024 ** 3, ram_free: 24 * 1024 ** 3 },
  devices: [{ name: 'rocm:0', type: 'rocm', vram_total: 16 * 1024 ** 3, vram_free: 15 * 1024 ** 3 }],
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
        json(res, cudaStats);
      } else if (req.url === '/object_info') {
        json(res, requiredNodes());
      } else {
        res.writeHead(404);
        res.end();
      }
    });
    const health = await getComfyHealth(baseUrl);
    expect(health).toMatchObject({ ok: true, baseUrl, nodeCount: NODE_COUNT });
    expect(health.system?.comfyVersion).toBe('0.44.0');
    expect(health.system?.ramTotalGB).toBe(32);
    expect(health.system?.devices[0]).toMatchObject({ name: 'cuda:0 RTX 4080', type: 'cuda', vramTotalGB: 16 });
    expect(health.presets.every((preset) => preset.ready)).toBe(true);
    expect(health.presets.map((preset) => preset.presetId)).toEqual(COMFY_PRESETS.map((preset) => preset.id));
  });

  it('reports missing nodes and refuses to prepare that preset', async () => {
    const baseUrl = await startServer((req, res) => {
      if (req.url === '/system_stats') {
        json(res, cudaStats);
      } else {
        json(res, requiredNodes(['SaveAudioAdvanced']));
      }
    });
    const health = await getComfyHealth(baseUrl);
    expect(health.ok).toBe(true);
    expect(health.presets[0]).toMatchObject({ ready: false, missingNodes: ['SaveAudioAdvanced'] });
    expect(health.presets.find((preset) => preset.presetId === 'image-flux-2-klein-4b')?.ready).toBe(true);
    await expect(prepareComfyGeneration(baseUrl, { presetId: 'audio-ace-step-1.5-turbo', prompt: 'test' })).rejects.toThrow(/Missing ComfyUI nodes/);
  });

  it('gates FLUX.2 klein on a 13GB+ CUDA/ROCm device', async () => {
    const cpu = await getComfyHealth(await startServer((req, res) => (req.url === '/system_stats' ? json(res, systemStats) : json(res, requiredNodes()))));
    expect(imagePresetDeviceReady(cpu)).toMatchObject({ ready: false });
    expect(cpu.presets.find((preset) => preset.presetId === 'image-flux-2-klein-4b')?.ready).toBe(false);
    const cuda = await getComfyHealth(await startServer((req, res) => (req.url === '/system_stats' ? json(res, cudaStats) : json(res, requiredNodes()))));
    expect(imagePresetDeviceReady(cuda)).toMatchObject({ ready: true });
    const small = await getComfyHealth(await startServer((req, res) => (req.url === '/system_stats' ? json(res, smallGpuStats) : json(res, requiredNodes()))));
    expect(imagePresetDeviceReady(small)).toMatchObject({ ready: false });
    const edge = await getComfyHealth(await startServer((req, res) => (req.url === '/system_stats' ? json(res, edgeGpuStats) : json(res, requiredNodes()))));
    expect(imagePresetDeviceReady(edge)).toMatchObject({ ready: true });
    const mps = await getComfyHealth(await startServer((req, res) => (req.url === '/system_stats' ? json(res, mpsStats) : json(res, requiredNodes()))));
    expect(imagePresetDeviceReady(mps)).toMatchObject({ ready: false });
    const rocm = await getComfyHealth(await startServer((req, res) => (req.url === '/system_stats' ? json(res, rocmStats) : json(res, requiredNodes()))));
    expect(imagePresetDeviceReady(rocm)).toMatchObject({ ready: true });
    expect(planReadyOnHealth(cpu, 'image-flux-2-klein-4b')).toMatchObject({ ready: false });
    expect(planReadyOnHealth(cuda, 'image-flux-2-klein-4b')).toMatchObject({ ready: true });
    const offline = await getComfyHealth('http://127.0.0.1:9', 500);
    expect(planReadyOnHealth(offline, 'image-flux-2-klein-4b')).toMatchObject({ ready: false });
    expect(planReadyOnHealth(cuda, 'missing')).toMatchObject({ ready: false });
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

  it('builds the distilled FLUX.2 klein workflow with image semantics', () => {
    const imagePreset = COMFY_PRESETS.find((preset) => preset.id === 'image-flux-2-klein-4b');
    expect(imagePreset).toMatchObject({
      modelId: 'flux-2-klein-4b',
      modality: 'image',
      family: 'flux',
      steps: 4,
      width: 1024,
      height: 1024,
      unet: 'flux-2-klein-4b.safetensors',
      clip1: 'qwen_3_4b_fp4_flux2.safetensors',
      vae: 'flux2-vae.safetensors',
    });
    expect(imagePreset?.requiredNodes).toEqual([
      'UNETLoader', 'CLIPLoader', 'VAELoader', 'CLIPTextEncode', 'ConditioningZeroOut',
      'Flux2Scheduler', 'KSamplerSelect', 'CFGGuider', 'RandomNoise', 'SamplerCustomAdvanced',
      'EmptyFlux2LatentImage', 'VAEDecode', 'SaveImage',
    ]);
    expect(COMFY_PRESETS.find((preset) => preset.id === 'audio-ace-step-1.5-turbo')?.requiredNodes).toEqual([
      'UNETLoader', 'DualCLIPLoader', 'VAELoader', 'TextEncodeAceStepAudio1.5',
      'EmptyAceStep1.5LatentAudio', 'KSampler', 'ModelSamplingAuraFlow',
      'ConditioningZeroOut', 'VAEDecodeAudio', 'SaveAudioAdvanced', 'PrimitiveInt',
    ]);
    const plan = buildComfyPlan({ presetId: 'image-flux-2-klein-4b', prompt: 'A red fox in a library', seed: 5, durationSeconds: 30 });
    expect(plan.durationSeconds).toBe(0);
    expect(plan.workflow['1'].inputs.unet_name).toBe('flux-2-klein-4b.safetensors');
    expect(plan.workflow['2'].inputs).toMatchObject({ clip_name: 'qwen_3_4b_fp4_flux2.safetensors', type: 'flux2' });
    expect(plan.workflow['3'].inputs.vae_name).toBe('flux2-vae.safetensors');
    expect(plan.workflow['4'].inputs).toMatchObject({ text: 'A red fox in a library', clip: ['2', 0] });
    expect(plan.workflow['5'].inputs.conditioning).toEqual(['4', 0]);
    expect(plan.workflow['6'].inputs).toMatchObject({ steps: 4, width: 1024, height: 1024 });
    expect(plan.workflow['7'].inputs.sampler_name).toBe('euler');
    expect(plan.workflow['8'].inputs).toMatchObject({ model: ['1', 0], positive: ['4', 0], negative: ['5', 0], cfg: 1 });
    expect(plan.workflow['9'].inputs.noise_seed).toBe(5);
    expect(plan.workflow['10'].inputs).toMatchObject({ noise: ['9', 0], guider: ['8', 0], sampler: ['7', 0], sigmas: ['6', 0], latent_image: ['11', 0] });
    expect(plan.workflow['11'].inputs).toMatchObject({ width: 1024, height: 1024, batch_size: 1 });
    expect(plan.workflow['12'].inputs).toMatchObject({ samples: ['10', 0], vae: ['3', 0] });
    expect(plan.workflow['13'].inputs).toMatchObject({ images: ['12', 0], filename_prefix: 'image/openro' });
    expect(Object.values(plan.workflow).map((node) => node.class_type)).toEqual([
      'UNETLoader', 'CLIPLoader', 'VAELoader', 'CLIPTextEncode', 'ConditioningZeroOut',
      'Flux2Scheduler', 'KSamplerSelect', 'CFGGuider', 'RandomNoise', 'SamplerCustomAdvanced',
      'EmptyFlux2LatentImage', 'VAEDecode', 'SaveImage',
    ]);
    expect(plan.workflow['94']).toBeUndefined();
    expect(plan.summary).toContain('1024x1024');
    expect(plan.summary).not.toContain('0s');
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
    const relative = path.relative(await realpath(sink), await realpath(saved.path));
    expect(relative.startsWith('..')).toBe(false);
    expect(path.isAbsolute(relative)).toBe(false);
    expect(path.basename(saved.path)).toContain('evil.mp3');
    expect(path.basename(saved.path)).not.toContain('..');
    expect(saved.bytes).toBe(Buffer.from('ID3-open-ro-live-audio').length);
    expect(saved.sha256).toMatch(/^[0-9a-f]{64}$/);
    expect(await readFile(saved.path)).toEqual(Buffer.from('ID3-open-ro-live-audio'));
    expect(submitted).toContain('"client_id":"open-ro"');
    expect(submitted).toContain('Late night trap');
    expect(submitted).toContain('acestep_v1.5_turbo.safetensors');
  });

  it('submits a FLUX image plan and saves a PNG inside the sink', async () => {
    let submitted = '';
    const png = Buffer.from('89504e470d0a1a0a0000000d4948445200000001000000010806000000', 'hex');
    const baseUrl = await startServer((req, res, body) => {
      if (req.method === 'POST' && req.url === '/prompt') {
        submitted = body;
        json(res, { prompt_id: 'img1' });
      } else if (req.url === '/history/img1') {
        json(res, { img1: { status: { status_str: 'success' }, outputs: { 13: { images: [{ filename: 'flux.png', subfolder: 'image', type: 'output' }] } } } });
      } else if (req.url?.startsWith('/view')) {
        res.writeHead(200, { 'content-type': 'image/png' });
        res.end(png);
      } else {
        res.writeHead(404);
        res.end();
      }
    });
    const plan = buildComfyPlan({ presetId: 'image-flux-2-klein-4b', prompt: 'A red fox', seed: 9 });
    const sink = await mkdtemp(path.join(tmpdir(), 'openro-media-'));
    cleanups.push(() => rm(sink, { recursive: true, force: true }));
    const result = await runComfyPlan(baseUrl, plan, sink, { timeoutMs: 5_000, pollMs: 10 });
    expect(result.files).toHaveLength(1);
    expect(result.files[0].path.endsWith('.png')).toBe(true);
    expect(result.files[0].bytes).toBe(png.length);
    expect(result.files[0].sha256).toBe(createHash('sha256').update(png).digest('hex'));
    expect(submitted).toContain('flux-2-klein-4b.safetensors');
    expect(submitted).toContain('"class_type":"SaveImage"');
    expect(submitted).toContain('A red fox');
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

  it('exposes the image preset in the card, tool schema and GPU-gated UI', () => {
    const copy = approvalCopy({
      id: 'i',
      kind: 'media',
      createdAt: '',
      mediaPreset: 'image-flux-2-klein-4b',
      mediaPrompt: 'A red fox',
      mediaDuration: 0,
      mediaSeed: 5,
    });
    expect(copy.title).toBe('Generate local image: image-flux-2-klein-4b');
    expect(copy.detail).toContain('1024x1024');
    expect(copy.approveLabel).toBe('Generate image');
    const main = readFileSync('electron/main.ts', 'utf8');
    expect(main).toContain("enum: ['audio-ace-step-1.5-turbo', 'audio-ace-step-1.5-xl', 'image-flux-2-klein-4b']");
    expect(main).toContain("if (presetId === 'image-flux-2-klein-4b')");
    expect(main).toContain('const health = await getComfyHealth(comfyBaseUrl());');
    expect(main).toContain("planReadyOnHealth(health, 'image-flux-2-klein-4b')");
    expect(main).toContain('planReadyOnHealth(health, plan.presetId)');
    expect(main).toContain('planReadyOnHealth(health, stored.plan.presetId)');
    expect(main).not.toContain('const hardware = await detectMediaReport();');
    expect(main).toContain('approvals.stageMedia({ projectRoot: item.projectRoot, plan });');
    const app = readFileSync('src/App.tsx', 'utf8').replace(/\r\n/g, '\n');
    expect(app).toContain('image-flux-2-klein-4b');
    expect(app).toContain('imagePreset?.ready');
    expect(app).toContain('disabled={!imageGpuReady}');
    expect(app).toContain('image unavailable');
    expect(app).toContain('FLUX.2 klein 4B (13GB GPU + nodes)');
    expect(app).toContain("planIsImage ? 'Generate image' : 'Generate audio'");
    expect(app).toContain('CUDA/ROCm device with at least 13GB VRAM');
    expect(app).toContain('if (wantsImage && !imageGpuReady)');
    expect(app).toContain('setMediaPresetId(event.target.value);\n                  setMediaPlan(null);');
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
