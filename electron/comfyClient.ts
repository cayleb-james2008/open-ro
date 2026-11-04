import * as crypto from 'node:crypto';
import { promises as fs } from 'node:fs';
import type { FileHandle } from 'node:fs/promises';
import * as path from 'node:path';

/**
 * ComfyUI bridge. The app never downloads models; it checks a user-installed
 * local server, freezes a 2026 workflow plan, submits it after review, and
 * copies the resulting files into the managed media sink.
 */

export interface ComfyDeviceStats {
  name: string;
  type: string | null;
  vramTotalGB: number | null;
  vramFreeGB: number | null;
}

export interface ComfySystemStats {
  comfyVersion: string | null;
  devices: ComfyDeviceStats[];
  ramTotalGB: number | null;
  ramFreeGB: number | null;
}

export interface ComfyPreset {
  id: string;
  modelId: string;
  name: string;
  modality: 'audio' | 'image';
  family: 'ace' | 'flux';
  requiredNodes: string[];
  unet: string;
  clip1: string;
  clip2: string;
  vae: string;
  steps: number;
  defaultSeconds: number;
  width?: number;
  height?: number;
}

export interface ComfyPresetReadiness {
  presetId: string;
  modelId: string;
  ready: boolean;
  missingNodes: string[];
  reason: string;
}

export interface ComfyHealth {
  ok: boolean;
  baseUrl: string;
  system: ComfySystemStats | null;
  nodeCount: number;
  presets: ComfyPresetReadiness[];
  error?: string;
}

export interface ComfyGenerationInput {
  presetId: string;
  prompt: string;
  lyrics?: string;
  durationSeconds?: number;
  seed?: number;
}

export interface ComfyGenerationPlan {
  planId: string;
  presetId: string;
  modelId: string;
  prompt: string;
  lyrics: string;
  durationSeconds: number;
  seed: number;
  workflow: Record<string, ComfyWorkflowNode>;
  summary: string;
  createdAt: string;
}

export type ComfyPlanSummary = Omit<ComfyGenerationPlan, 'workflow'>;

export interface ComfyWorkflowNode {
  class_type: string;
  inputs: Record<string, unknown>;
  _meta?: { title?: string };
}

export interface ComfyOutputFile {
  filename: string;
  subfolder: string;
  type: string;
}

export interface ComfySavedFile {
  path: string;
  bytes: number;
  sha256: string;
}

export interface ComfyGenerationResult {
  ok: boolean;
  promptId: string;
  summary: string;
  files: ComfySavedFile[];
}

export interface ComfyRunOptions {
  timeoutMs?: number;
  pollMs?: number;
  requestTimeoutMs?: number;
}

const MAX_PROMPT = 2_000;
const MAX_LYRICS = 8_000;
const MAX_OUTPUT_BYTES = 100 * 1024 * 1024;
const ALLOWED_EXTENSIONS = new Set(['.mp3', '.wav', '.flac', '.ogg', '.png', '.jpg', '.jpeg', '.webp', '.gif', '.mp4', '.webm', '.ply']);

const REQUIRED_ACE_NODES = [
  'UNETLoader',
  'DualCLIPLoader',
  'VAELoader',
  'TextEncodeAceStepAudio1.5',
  'EmptyAceStep1.5LatentAudio',
  'KSampler',
  'ModelSamplingAuraFlow',
  'ConditioningZeroOut',
  'VAEDecodeAudio',
  'SaveAudioAdvanced',
  'PrimitiveInt',
];

const REQUIRED_FLUX_NODES = [
  'UNETLoader',
  'CLIPLoader',
  'VAELoader',
  'CLIPTextEncode',
  'ConditioningZeroOut',
  'Flux2Scheduler',
  'KSamplerSelect',
  'CFGGuider',
  'RandomNoise',
  'SamplerCustomAdvanced',
  'EmptyFlux2LatentImage',
  'VAEDecode',
  'SaveImage',
];

export const COMFY_PRESETS: ComfyPreset[] = [
  {
    id: 'audio-ace-step-1.5-turbo',
    modelId: 'ace-step-1.5-turbo',
    name: 'ACE-Step 1.5 Turbo',
    modality: 'audio',
    family: 'ace',
    requiredNodes: REQUIRED_ACE_NODES,
    unet: 'acestep_v1.5_turbo.safetensors',
    clip1: 'qwen_0.6b_ace15.safetensors',
    clip2: 'qwen_1.7b_ace15.safetensors',
    vae: 'ace_1.5_vae.safetensors',
    steps: 8,
    defaultSeconds: 10,
  },
  {
    id: 'audio-ace-step-1.5-xl',
    modelId: 'ace-step-1.5-xl',
    name: 'ACE-Step 1.5 XL Turbo',
    modality: 'audio',
    family: 'ace',
    requiredNodes: REQUIRED_ACE_NODES,
    unet: 'acestep_v1.5_xl_turbo_bf16.safetensors',
    clip1: 'qwen_0.6b_ace15.safetensors',
    clip2: 'qwen_4b_ace15.safetensors',
    vae: 'ace_1.5_vae.safetensors',
    steps: 8,
    defaultSeconds: 10,
  },
  {
    id: 'image-flux-2-klein-4b',
    modelId: 'flux-2-klein-4b',
    name: 'FLUX.2 klein 4B',
    modality: 'image',
    family: 'flux',
    requiredNodes: REQUIRED_FLUX_NODES,
    unet: 'flux-2-klein-4b.safetensors',
    clip1: 'qwen_3_4b_fp4_flux2.safetensors',
    clip2: '',
    vae: 'flux2-vae.safetensors',
    steps: 4,
    defaultSeconds: 0,
    width: 1024,
    height: 1024,
  },
];

const ACE_WORKFLOW: Record<string, ComfyWorkflowNode> = {
  '3': {
    class_type: 'KSampler',
    inputs: {
      seed: ['109', 0],
      steps: 8,
      cfg: 1,
      sampler_name: 'euler',
      scheduler: 'simple',
      denoise: 1,
      model: ['78', 0],
      positive: ['94', 0],
      negative: ['47', 0],
      latent_image: ['98', 0],
    },
    _meta: { title: 'KSampler' },
  },
  '18': { class_type: 'VAEDecodeAudio', inputs: { samples: ['3', 0], vae: ['106', 0] }, _meta: { title: 'VAE Decode Audio' } },
  '47': { class_type: 'ConditioningZeroOut', inputs: { conditioning: ['94', 0] }, _meta: { title: 'Conditioning Zero Out' } },
  '78': { class_type: 'ModelSamplingAuraFlow', inputs: { shift: 3, sampling: 'flow', model: ['104', 0] }, _meta: { title: 'ModelSamplingAuraFlow' } },
  '94': {
    class_type: 'TextEncodeAceStepAudio1.5',
    inputs: {
      tags: '',
      lyrics: '[Instrumental]',
      seed: ['109', 0],
      bpm: 95,
      duration: 10,
      timesignature: '4',
      language: 'en',
      keyscale: 'E minor',
      generate_audio_codes: true,
      cfg_scale: 2,
      temperature: 0.85,
      top_p: 0.9,
      top_k: 0,
      min_p: 0,
      clip: ['105', 0],
    },
    _meta: { title: 'TextEncodeAceStepAudio1.5' },
  },
  '98': { class_type: 'EmptyAceStep1.5LatentAudio', inputs: { seconds: 10, batch_size: 1 }, _meta: { title: 'Empty Ace Step 1.5 Latent Audio' } },
  '104': { class_type: 'UNETLoader', inputs: { unet_name: '', weight_dtype: 'default' }, _meta: { title: 'Load Diffusion Model' } },
  '105': { class_type: 'DualCLIPLoader', inputs: { clip_name1: '', clip_name2: '', type: 'ace', device: 'default' }, _meta: { title: 'Load CLIP (Dual)' } },
  '106': { class_type: 'VAELoader', inputs: { vae_name: '' }, _meta: { title: 'Load VAE' } },
  '109': { class_type: 'PrimitiveInt', inputs: { value: 0 }, _meta: { title: 'Int (Seed)' } },
  '111': { class_type: 'SaveAudioAdvanced', inputs: { filename_prefix: 'audio/openro', format: 'mp3', 'format.quality': 'V0', audio: ['18', 0] }, _meta: { title: 'Save Audio (Advanced)' } },
};

function roundGB(value: unknown): number | null {
  return typeof value === 'number' && Number.isFinite(value) && value > 0 ? Math.round((value / 1024 ** 3) * 10) / 10 : null;
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export function normalizeComfyBaseUrl(raw: string): string {
  const url = new URL(raw.trim());
  if (url.protocol !== 'http:' && url.protocol !== 'https:') {
    throw new Error('The ComfyUI URL must use http or https.');
  }
  const host = url.hostname.toLowerCase();
  if (!['127.0.0.1', 'localhost', '::1', '[::1]'].includes(host)) {
    throw new Error('Only a loopback ComfyUI URL is allowed.');
  }
  return url.origin;
}

async function requestJson(baseUrl: string, route: string, init: RequestInit = {}, timeoutMs = 10_000): Promise<unknown> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), timeoutMs);
  try {
    const url = new URL(route, `${baseUrl.replace(/\/+$/, '')}/`);
    const response = await fetch(url, {
      ...init,
      redirect: 'error',
      signal: controller.signal,
      headers: { accept: 'application/json', ...(init.headers ?? {}) },
    });
    if (!response.ok) {
      throw new Error(`ComfyUI ${route} returned ${response.status}.`);
    }
    return await response.json();
  } finally {
    clearTimeout(timer);
  }
}

export async function getComfyHealth(baseUrl: string, timeoutMs = 10_000): Promise<ComfyHealth> {
  try {
    const [stats, info] = await Promise.all([
      requestJson(baseUrl, '/system_stats', {}, timeoutMs),
      requestJson(baseUrl, '/object_info', {}, timeoutMs),
    ]);
    const statsRecord = (stats && typeof stats === 'object' ? stats : {}) as Record<string, unknown>;
    const systemRecord = (statsRecord.system && typeof statsRecord.system === 'object' ? statsRecord.system : {}) as Record<string, unknown>;
    const deviceList = Array.isArray(statsRecord.devices) ? statsRecord.devices : [];
    const devices: ComfyDeviceStats[] = deviceList
      .filter((device): device is Record<string, unknown> => Boolean(device) && typeof device === 'object')
      .map((device) => {
        const type = typeof device.type === 'string' ? device.type : null;
        const isCpu = type?.toLowerCase() === 'cpu';
        return {
          name: typeof device.name === 'string' ? device.name : 'device',
          type,
          vramTotalGB: isCpu ? null : roundGB(device.vram_total),
          vramFreeGB: isCpu ? null : roundGB(device.vram_free),
        };
      });
    const nodeNames = info && typeof info === 'object' ? Object.keys(info) : [];
    const gpu = imageDeviceReady(devices);
    const presets = COMFY_PRESETS.map((preset): ComfyPresetReadiness => {
      const missingNodes = preset.requiredNodes.filter((node) => !nodeNames.includes(node));
      const needsImageGpu = preset.family === 'flux';
      const ready = missingNodes.length === 0 && (!needsImageGpu || gpu.ready);
      const reason = missingNodes.length > 0
        ? `Missing ComfyUI nodes: ${missingNodes.join(', ')}.`
        : needsImageGpu && !gpu.ready
          ? gpu.reason
          : 'Required ComfyUI nodes are installed.';
      return { presetId: preset.id, modelId: preset.modelId, ready, missingNodes, reason };
    });
    return {
      ok: true,
      baseUrl,
      system: {
        comfyVersion: typeof systemRecord.comfyui_version === 'string' ? systemRecord.comfyui_version : null,
        devices,
        ramTotalGB: roundGB(systemRecord.ram_total),
        ramFreeGB: roundGB(systemRecord.ram_free),
      },
      nodeCount: nodeNames.length,
      presets,
    };
  } catch (error) {
    return { ok: false, baseUrl, system: null, nodeCount: 0, presets: [], error: messageOf(error) };
  }
}

function planAceWorkflow(preset: ComfyPreset, prompt: string, lyrics: string, durationSeconds: number, seed: number): Record<string, ComfyWorkflowNode> {
  const workflow = JSON.parse(JSON.stringify(ACE_WORKFLOW)) as Record<string, ComfyWorkflowNode>;
  workflow['3'].inputs.steps = preset.steps;
  workflow['94'].inputs.tags = prompt;
  workflow['94'].inputs.lyrics = lyrics;
  workflow['94'].inputs.duration = durationSeconds;
  workflow['98'].inputs.seconds = durationSeconds;
  workflow['104'].inputs.unet_name = preset.unet;
  workflow['105'].inputs.clip_name1 = preset.clip1;
  workflow['105'].inputs.clip_name2 = preset.clip2;
  workflow['106'].inputs.vae_name = preset.vae;
  workflow['109'].inputs.value = seed;
  workflow['111'].inputs.filename_prefix = 'audio/openro';
  return workflow;
}

function planFluxWorkflow(preset: ComfyPreset, prompt: string, seed: number): Record<string, ComfyWorkflowNode> {
  const width = preset.width ?? 1024;
  const height = preset.height ?? 1024;
  return {
    '1': { class_type: 'UNETLoader', inputs: { unet_name: preset.unet, weight_dtype: 'default' } },
    '2': { class_type: 'CLIPLoader', inputs: { clip_name: preset.clip1, type: 'flux2', device: 'default' } },
    '3': { class_type: 'VAELoader', inputs: { vae_name: preset.vae } },
    '4': { class_type: 'CLIPTextEncode', inputs: { text: prompt, clip: ['2', 0] } },
    '5': { class_type: 'ConditioningZeroOut', inputs: { conditioning: ['4', 0] } },
    '6': { class_type: 'Flux2Scheduler', inputs: { steps: preset.steps, width, height } },
    '7': { class_type: 'KSamplerSelect', inputs: { sampler_name: 'euler' } },
    '8': { class_type: 'CFGGuider', inputs: { model: ['1', 0], positive: ['4', 0], negative: ['5', 0], cfg: 1 } },
    '9': { class_type: 'RandomNoise', inputs: { noise_seed: seed } },
    '10': { class_type: 'SamplerCustomAdvanced', inputs: { noise: ['9', 0], guider: ['8', 0], sampler: ['7', 0], sigmas: ['6', 0], latent_image: ['11', 0] } },
    '11': { class_type: 'EmptyFlux2LatentImage', inputs: { width, height, batch_size: 1 } },
    '12': { class_type: 'VAEDecode', inputs: { samples: ['10', 0], vae: ['3', 0] } },
    '13': { class_type: 'SaveImage', inputs: { images: ['12', 0], filename_prefix: 'image/openro' } },
  };
}

function planWorkflow(preset: ComfyPreset, prompt: string, lyrics: string, durationSeconds: number, seed: number): Record<string, ComfyWorkflowNode> {
  return preset.family === 'flux' ? planFluxWorkflow(preset, prompt, seed) : planAceWorkflow(preset, prompt, lyrics, durationSeconds, seed);
}

export function buildComfyPlan(input: ComfyGenerationInput): ComfyGenerationPlan {
  const preset = COMFY_PRESETS.find((entry) => entry.id === input.presetId);
  if (!preset) {
    throw new Error(`Unknown ComfyUI preset: ${input.presetId}.`);
  }
  const prompt = typeof input.prompt === 'string' ? input.prompt.trim() : '';
  if (!prompt) {
    throw new Error('A media prompt is required.');
  }
  if (prompt.length > MAX_PROMPT) {
    throw new Error(`The media prompt is longer than ${MAX_PROMPT} characters.`);
  }
  const lyrics = ((typeof input.lyrics === 'string' ? input.lyrics.trim() : '') || '[Instrumental]').slice(0, MAX_LYRICS);
  const requestedSeconds = input.durationSeconds ?? preset.defaultSeconds;
  const durationSeconds = preset.modality === 'image'
    ? 0
    : Math.min(60, Math.max(5, Number.isFinite(requestedSeconds) ? Math.round(requestedSeconds) : preset.defaultSeconds));
  let seed = crypto.randomInt(0, 2 ** 31 - 1);
  if (input.seed !== undefined) {
    if (!Number.isInteger(input.seed) || input.seed < 0 || input.seed > 2 ** 31 - 1) {
      throw new Error('Seed must be an integer between 0 and 2147483647.');
    }
    seed = input.seed;
  }
  return {
    planId: crypto.randomUUID(),
    presetId: preset.id,
    modelId: preset.modelId,
    prompt,
    lyrics,
    durationSeconds,
    seed,
    workflow: planWorkflow(preset, prompt, lyrics, durationSeconds, seed),
    summary: preset.modality === 'image'
      ? `${preset.name}: ${preset.width ?? 1024}x${preset.height ?? 1024}, ${preset.steps} steps, seed ${seed}. Review this plan, then generate to save under the app media folder.`
      : `${preset.name}: ${durationSeconds}s, ${preset.steps} steps, seed ${seed}. Review this plan, then generate to save under the app media folder.`,
    createdAt: new Date().toISOString(),
  };
}

function imageDeviceReady(devices: ComfyDeviceStats[]): { ready: boolean; reason: string } {
  const gpu = devices.find((device) => ['cuda', 'rocm'].includes((device.type ?? '').toLowerCase()) && (device.vramTotalGB ?? 0) >= 13);
  return gpu
    ? { ready: true, reason: `${gpu.name} reports ${gpu.vramTotalGB}GB VRAM.` }
    : { ready: false, reason: 'FLUX.2 klein 4B needs a CUDA/ROCm device with at least 13GB VRAM; ComfyUI reports none.' };
}

export function imagePresetDeviceReady(health: ComfyHealth): { ready: boolean; reason: string } {
  if (!health.ok) {
    return { ready: false, reason: 'ComfyUI is not reachable.' };
  }
  return imageDeviceReady(health.system?.devices ?? []);
}

export function planReadyOnHealth(health: ComfyHealth, presetId: string): { ready: boolean; reason: string } {
  if (!health.ok) {
    return { ready: false, reason: health.error ?? 'ComfyUI is not reachable.' };
  }
  const preset = health.presets.find((entry) => entry.presetId === presetId);
  return preset
    ? { ready: preset.ready, reason: preset.reason }
    : { ready: false, reason: `Unknown ComfyUI preset: ${presetId}.` };
}

export async function prepareComfyGeneration(baseUrl: string, input: ComfyGenerationInput): Promise<ComfyGenerationPlan> {
  const health = await getComfyHealth(baseUrl);
  if (!health.ok) {
    throw new Error(`ComfyUI is not reachable at ${baseUrl}. ${health.error ?? ''}`.trim());
  }
  const readiness = health.presets.find((entry) => entry.presetId === input.presetId);
  if (!readiness) {
    throw new Error(`Unknown ComfyUI preset: ${input.presetId}.`);
  }
  if (!readiness.ready) {
    throw new Error(readiness.reason);
  }
  return buildComfyPlan(input);
}

export function publicPlan(plan: ComfyGenerationPlan): ComfyPlanSummary {
  return {
    planId: plan.planId,
    presetId: plan.presetId,
    modelId: plan.modelId,
    prompt: plan.prompt,
    lyrics: plan.lyrics,
    durationSeconds: plan.durationSeconds,
    seed: plan.seed,
    summary: plan.summary,
    createdAt: plan.createdAt,
  };
}

function collectOutputs(outputs: unknown): ComfyOutputFile[] {
  const files: ComfyOutputFile[] = [];
  if (!outputs || typeof outputs !== 'object') {
    return files;
  }
  for (const value of Object.values(outputs as Record<string, unknown>)) {
    if (!value || typeof value !== 'object') {
      continue;
    }
    for (const key of ['audio', 'images', 'gifs', 'video', 'files']) {
      const list = (value as Record<string, unknown>)[key];
      if (!Array.isArray(list)) {
        continue;
      }
      for (const item of list) {
        if (!item || typeof item !== 'object') {
          continue;
        }
        const record = item as Record<string, unknown>;
        if (typeof record.filename !== 'string' || !record.filename) {
          continue;
        }
        files.push({
          filename: record.filename,
          subfolder: typeof record.subfolder === 'string' ? record.subfolder : '',
          type: typeof record.type === 'string' ? record.type : 'output',
        });
      }
    }
  }
  const seen = new Set<string>();
  return files.filter((file) => {
    const key = `${file.subfolder}/${file.filename}`;
    if (seen.has(key)) {
      return false;
    }
    seen.add(key);
    return true;
  });
}

async function saveOutput(baseUrl: string, output: ComfyOutputFile, sinkDir: string, stamp: string, requestTimeoutMs: number): Promise<ComfySavedFile> {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), requestTimeoutMs);
  try {
    const url = new URL('/view', `${baseUrl.replace(/\/+$/, '')}/`);
    url.searchParams.set('filename', output.filename);
    url.searchParams.set('subfolder', output.subfolder);
    url.searchParams.set('type', output.type);
    const response = await fetch(url, { redirect: 'error', signal: controller.signal });
    if (!response.ok) {
      throw new Error(`ComfyUI /view returned ${response.status}.`);
    }
    const declared = Number(response.headers.get('content-length') ?? '');
    if (Number.isFinite(declared) && declared > MAX_OUTPUT_BYTES) {
      throw new Error('ComfyUI output is larger than 100 MB.');
    }
    const buffer = Buffer.from(await response.arrayBuffer());
    if (buffer.length > MAX_OUTPUT_BYTES) {
      throw new Error('ComfyUI output is larger than 100 MB.');
    }
    const extension = path.extname(output.filename).toLowerCase();
    if (!ALLOWED_EXTENSIONS.has(extension)) {
      throw new Error(`Unsupported ComfyUI output type: ${extension || '(none)'}.`);
    }
    const safeName = path.basename(output.filename).replace(/[^\w.-]+/g, '-').slice(0, 120) || `output${extension}`;
    const sha256 = crypto.createHash('sha256').update(buffer).digest('hex');
    const resolvedSink = path.resolve(sinkDir);
    await fs.mkdir(resolvedSink, { recursive: true });
    if ((await fs.lstat(resolvedSink)).isSymbolicLink()) {
      throw new Error('The media sink must not be a symlink.');
    }
    const realSink = await fs.realpath(resolvedSink);
    let handle: FileHandle | null = null;
    let target = '';
    for (let attempt = 0; attempt < 10; attempt += 1) {
      const suffix = attempt === 0 ? '' : `-${attempt}`;
      target = path.join(realSink, `${stamp}-${sha256.slice(0, 8)}${suffix}-${safeName}`);
      try {
        handle = await fs.open(target, 'wx', 0o600);
        break;
      } catch (error) {
        if ((error as NodeJS.ErrnoException).code !== 'EEXIST') {
          throw error;
        }
      }
    }
    if (!handle) {
      throw new Error('Could not choose a unique media output name.');
    }
    try {
      await handle.writeFile(buffer);
    } finally {
      await handle.close();
    }
    return { path: target, bytes: buffer.length, sha256 };
  } finally {
    clearTimeout(timer);
  }
}

export async function runComfyPlan(
  baseUrl: string,
  plan: ComfyGenerationPlan,
  sinkDir: string,
  options: ComfyRunOptions = {},
): Promise<ComfyGenerationResult> {
  const requestTimeoutMs = options.requestTimeoutMs ?? 15_000;
  const submitted = await requestJson(
    baseUrl,
    '/prompt',
    {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ prompt: plan.workflow, client_id: 'open-ro' }),
    },
    requestTimeoutMs,
  );
  const promptId = submitted && typeof submitted === 'object' && typeof (submitted as Record<string, unknown>).prompt_id === 'string'
    ? String((submitted as Record<string, unknown>).prompt_id)
    : '';
  if (!promptId) {
    throw new Error('ComfyUI accepted the plan but returned no prompt id.');
  }
  const deadline = Date.now() + (options.timeoutMs ?? 30 * 60_000);
  const pollMs = options.pollMs ?? 2_000;
  while (Date.now() < deadline) {
    const history = await requestJson(baseUrl, `/history/${encodeURIComponent(promptId)}`, {}, requestTimeoutMs);
    const record = history && typeof history === 'object' ? (history as Record<string, unknown>)[promptId] : undefined;
    if (record && typeof record === 'object') {
      const status = (record as Record<string, unknown>).status;
      const statusStr = status && typeof status === 'object' ? (status as Record<string, unknown>).status_str : undefined;
      if (statusStr === 'error') {
        throw new Error('ComfyUI reported a generation error.');
      }
      const outputs = collectOutputs((record as Record<string, unknown>).outputs);
      if (outputs.length === 0) {
        throw new Error('ComfyUI finished without output files.');
      }
      const stamp = new Date().toISOString().replace(/[:.]/g, '-');
      const files: ComfySavedFile[] = [];
      for (const output of outputs) {
        files.push(await saveOutput(baseUrl, output, sinkDir, stamp, requestTimeoutMs));
      }
      return {
        ok: true,
        promptId,
        summary: `${plan.modelId} generated ${files.length} file(s) via ComfyUI.`,
        files,
      };
    }
    await new Promise((resolve) => setTimeout(resolve, pollMs));
  }
  throw new Error('ComfyUI generation timed out.');
}
