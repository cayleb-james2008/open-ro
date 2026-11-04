/**
 * Curated local media catalog (2026-only) and hardware-tier recommendations.
 * Pure module: no Node builtins, so the renderer can type against it. Hardware
 * detection lives in the main process; ComfyUI-reported memory arrives with the
 * bridge and replaces an unknown GPU profile.
 */

export type MediaModality = 'image' | 'audio' | 'voice' | 'video' | '3d';
export type MediaLane = 'newest' | 'efficient';
export type MediaLicenseClass = 'permissive' | 'research' | 'community';
export type MediaBackend = 'cuda' | 'rocm' | 'cpu';
export type VramConfidence = 'documented' | 'estimated' | 'unknown';
export type MediaTier = 'cpu' | '8gb' | '12gb' | '16gb' | '24gb' | 'multi-gpu';
export type MediaStatus = 'ready' | 'cpu-only' | 'needs-gpu' | 'no-fit';

export interface MediaLicense {
  name: string;
  class: MediaLicenseClass;
  commercial: boolean;
}

export interface MediaModel {
  id: string;
  name: string;
  modality: MediaModality;
  lane: MediaLane;
  /** First public release, not a repack or quantization upload date. */
  releasedAt: string;
  license: MediaLicense;
  paramsB: number | null;
  steps: number | null;
  minVramGB: number | null;
  vramConfidence: VramConfidence;
  minRamGB: number;
  minGpus: number;
  cpuCapable: boolean;
  backends: MediaBackend[];
  workflow: string;
  source: string;
  notes: string;
}

export interface MediaHardwareProfile {
  totalRamGB: number;
  freeRamGB: number;
  cpuModel: string;
  cores: number;
  platform: string;
  arch: string;
  gpuName: string | null;
  /** Largest device memory reported, used for display. */
  vramGB: number | null;
  /** Per-device memory, used to match multi-GPU floors without trusting the max alone. */
  gpuVramGB: number[];
  gpuCount: number;
  vramSource: 'nvidia-smi' | 'comfyui' | 'unknown';
  freeDiskGB: number | null;
}

export interface MediaRecommendation {
  modality: MediaModality;
  status: MediaStatus;
  model: MediaModel | null;
  lane: MediaLane | null;
  reason: string;
}

export interface MediaPolicy {
  year: number;
  newestSince: string;
  note: string;
}

export interface MediaReport {
  profile: MediaHardwareProfile;
  tier: MediaTier;
  recommendations: MediaRecommendation[];
  catalog: MediaModel[];
  policy: MediaPolicy;
}

export const MEDIA_POLICY: MediaPolicy = {
  year: 2026,
  newestSince: '2026-08-03',
  note: 'Only first releases from 2026 are catalogued. Newest means the last 90 days; efficiency holdovers are badged and never replaced by a 2025 fallback.',
};

export const MEDIA_CATALOG: MediaModel[] = [
  {
    id: 'qwen-image-2.1-lightning',
    name: 'Qwen-Image 2.1 + Lightning',
    modality: 'image',
    lane: 'newest',
    releasedAt: '2026-10-13',
    license: { name: 'Qwen Research License', class: 'research', commercial: false },
    paramsB: 7,
    steps: 4,
    minVramGB: 24,
    vramConfidence: 'estimated',
    minRamGB: 32,
    minGpus: 1,
    cpuCapable: false,
    backends: ['cuda', 'rocm'],
    workflow: 'image-qwen-2.1-lightning',
    source: 'https://huggingface.co/lightx2v/Qwen-Image-2.1-Lightning',
    notes: '40-step base distilled to a 4-step Lightning LoRA. Research license, not the commercial default.',
  },
  {
    id: 'flux-2-klein-4b',
    name: 'FLUX.2 klein 4B',
    modality: 'image',
    lane: 'efficient',
    releasedAt: '2026-01-14',
    license: { name: 'Apache 2.0', class: 'permissive', commercial: true },
    paramsB: 4,
    steps: 4,
    minVramGB: 13,
    vramConfidence: 'documented',
    minRamGB: 8,
    minGpus: 1,
    cpuCapable: false,
    backends: ['cuda', 'rocm'],
    workflow: 'image-flux-2-klein-4b',
    source: 'https://huggingface.co/black-forest-labs/FLUX.2-klein-4B',
    notes: 'Official card: ~13GB VRAM, 4 steps, ComfyUI and Diffusers.',
  },
  {
    id: 'minimax-music-3',
    name: 'MiniMax Music 3',
    modality: 'audio',
    lane: 'newest',
    releasedAt: '2026-08-07',
    license: { name: 'MiniMax Music 3 license', class: 'community', commercial: false },
    paramsB: 8.6,
    steps: null,
    minVramGB: 8,
    vramConfidence: 'documented',
    minRamGB: 32,
    minGpus: 1,
    cpuCapable: false,
    backends: ['cuda'],
    workflow: 'audio-minimax-music-3',
    source: 'https://huggingface.co/MiniMaxAI/MiniMax-Music3',
    notes: 'Current card: full precision under 24GB, ~22GB with auto CPU offload, and 8GB with language-model layer streaming; five-minute songs. Review the license before commercial use.',
  },
  {
    id: 'ace-step-1.5-xl',
    name: 'ACE-Step 1.5 XL Turbo',
    modality: 'audio',
    lane: 'efficient',
    releasedAt: '2026-04-02',
    license: { name: 'MIT', class: 'permissive', commercial: true },
    paramsB: 4,
    steps: 8,
    minVramGB: 12,
    vramConfidence: 'documented',
    minRamGB: 16,
    minGpus: 1,
    cpuCapable: false,
    backends: ['cuda', 'rocm'],
    workflow: 'audio-ace-step-1.5-xl',
    source: 'https://huggingface.co/ACE-Step/acestep-v15-xl-turbo',
    notes: 'XL 4B DiT; official card: at least 12GB VRAM with offload, 20GB recommended.',
  },
  {
    id: 'ace-step-1.5-turbo',
    name: 'ACE-Step 1.5 Turbo',
    modality: 'audio',
    lane: 'efficient',
    releasedAt: '2026-01-23',
    license: { name: 'MIT', class: 'permissive', commercial: true },
    paramsB: null,
    steps: 8,
    minVramGB: 4,
    vramConfidence: 'documented',
    minRamGB: 8,
    minGpus: 1,
    cpuCapable: true,
    backends: ['cuda', 'rocm', 'cpu'],
    workflow: 'audio-ace-step-1.5-turbo',
    source: 'https://huggingface.co/ACE-Step/Ace-Step1.5',
    notes: 'Official card: under 4GB VRAM, 8 steps, CPU/ROCm supported, commercial-ready MIT.',
  },
  {
    id: 'qwen3-tts-0.6b',
    name: 'Qwen3-TTS 0.6B',
    modality: 'voice',
    lane: 'efficient',
    releasedAt: '2026-01-21',
    license: { name: 'Apache 2.0', class: 'permissive', commercial: true },
    paramsB: 0.6,
    steps: null,
    minVramGB: 0,
    vramConfidence: 'documented',
    minRamGB: 4,
    minGpus: 0,
    cpuCapable: true,
    backends: ['cpu', 'cuda'],
    workflow: 'voice-qwen3-tts-0.6b',
    source: 'https://huggingface.co/Qwen/Qwen3-TTS-12Hz-0.6B-Base',
    notes: 'Streaming, 10 languages and 3-second voice cloning; CPU-capable 0.6B.',
  },
  {
    id: 'sana-video-2.0-5b-4step',
    name: 'SANA-Video 2.0 5B 4-Step',
    modality: 'video',
    lane: 'newest',
    releasedAt: '2026-08-27',
    license: { name: 'Apache 2.0', class: 'permissive', commercial: true },
    paramsB: 4.47,
    steps: 4,
    minVramGB: 24,
    vramConfidence: 'estimated',
    minRamGB: 32,
    minGpus: 1,
    cpuCapable: false,
    backends: ['cuda'],
    workflow: 'video-sana-2.0-5b-4step',
    source: 'https://huggingface.co/Efficient-Large-Model/SANA-Video_2.0_5B_720p_4step',
    notes: '720p 5s/8s, four-step distilled preview. Official VRAM floor not published; 24GB-class estimate.',
  },
  {
    id: 'idol',
    name: 'Idol',
    modality: '3d',
    lane: 'newest',
    releasedAt: '2026-10-28',
    license: { name: 'Apache 2.0', class: 'permissive', commercial: true },
    paramsB: null,
    steps: 50,
    minVramGB: 7.2,
    vramConfidence: 'documented',
    minRamGB: 8,
    minGpus: 1,
    cpuCapable: false,
    backends: ['cuda'],
    workflow: '3d-idol',
    source: 'https://huggingface.co/speridlabs/idol',
    notes: 'One image to up to one million Gaussians (.ply) in under two seconds; 7.2GB VRAM, CUDA.',
  },
  {
    id: 'pixal3d',
    name: 'Pixal3D',
    modality: '3d',
    lane: 'efficient',
    releasedAt: '2026-04-30',
    license: { name: 'MIT', class: 'permissive', commercial: true },
    paramsB: null,
    steps: null,
    minVramGB: null,
    vramConfidence: 'unknown',
    minRamGB: 16,
    minGpus: 1,
    cpuCapable: false,
    backends: ['cuda'],
    workflow: '3d-pixal3d',
    source: 'https://huggingface.co/TencentARC/Pixal3D',
    notes: 'SIGGRAPH 2026; low-VRAM mode loads models on demand. VRAM floor not published.',
  },
];

const MODALITIES: MediaModality[] = ['image', 'audio', 'voice', 'video', '3d'];

const LEGACY_MODEL_IDS = new Set([
  'sdxl',
  'sd-1.5',
  'sd15',
  'musicgen',
  'animatediff',
  'svd',
  'wan2.2-ti2v-5b',
  'z-image-turbo',
  'trellis-2',
  'hunyuan3d-omni',
  'triposr',
]);

export function mediaTier(profile: MediaHardwareProfile): MediaTier {
  if (profile.gpuCount > 1) {
    return 'multi-gpu';
  }
  const vram = profile.vramGB;
  if (vram === null || vram < 8) {
    return 'cpu';
  }
  if (vram < 12) {
    return '8gb';
  }
  if (vram < 16) {
    return '12gb';
  }
  if (vram < 24) {
    return '16gb';
  }
  return '24gb';
}

function requirementText(model: MediaModel): string {
  const gpu =
    model.minGpus === 0
      ? ''
      : `, ${model.minGpus} GPU(s)` +
        (model.minVramGB === null ? ' (VRAM floor not published)' : ` with ${model.minVramGB}GB VRAM${model.vramConfidence === 'estimated' ? ' (estimated)' : ''}`);
  return `${model.minRamGB}GB RAM${gpu}`;
}

function licenseNote(model: MediaModel): string {
  return model.license.commercial ? '' : ` License: ${model.license.name}; not the commercial default.`;
}

function gpuFits(model: MediaModel, profile: MediaHardwareProfile): boolean {
  if (model.minGpus === 0) {
    return true;
  }
  if (profile.gpuCount < model.minGpus) {
    return false;
  }
  if (model.minVramGB === null) {
    return false;
  }
  const floor = model.minVramGB;
  if (profile.gpuVramGB.length >= model.minGpus) {
    return profile.gpuVramGB.filter((vram) => vram >= floor).length >= model.minGpus;
  }
  if (model.minGpus > 1) {
    return false;
  }
  return profile.vramGB !== null && profile.vramGB >= floor;
}

function fits(model: MediaModel, profile: MediaHardwareProfile): MediaStatus {
  if (profile.totalRamGB < model.minRamGB) {
    return 'no-fit';
  }
  if (model.minGpus === 0) {
    return 'ready';
  }
  if (gpuFits(model, profile)) {
    return 'ready';
  }
  if (model.cpuCapable) {
    return 'cpu-only';
  }
  return 'needs-gpu';
}

function readyReason(model: MediaModel, profile: MediaHardwareProfile): string {
  if (model.minGpus === 0) {
    return `CPU-capable ${model.lane === 'newest' ? 'newest' : 'efficiency'} 2026 pick; needs ${model.minRamGB}GB RAM.${licenseNote(model)}`;
  }
  const floor = model.minVramGB === null ? 'an unpublished VRAM floor' : `${model.minVramGB}GB VRAM (${model.vramConfidence})`;
  return `${model.lane === 'newest' ? 'Newest' : 'Efficiency'} 2026 pick; fits ${profile.vramGB}GB with ${floor}.${licenseNote(model)}`;
}

function needsGpuReason(model: MediaModel, profile: MediaHardwareProfile): string {
  if (model.minVramGB === null) {
    return `${model.name} does not publish a VRAM floor; connect ComfyUI to measure this device before running.${licenseNote(model)}`;
  }
  const backend = model.backends.includes('rocm') ? 'CUDA/ROCm' : 'CUDA';
  const gpu = model.minGpus > 1 ? `${model.minGpus} ${backend} GPUs` : `a ${backend} device`;
  const detected = profile.gpuCount === 0 ? 'no matching device was detected' : `${profile.gpuCount} GPU(s) and ${profile.vramGB ?? 'unknown'}GB VRAM were detected`;
  return `${model.name} requires ${gpu} with at least ${model.minVramGB}GB VRAM${model.vramConfidence === 'estimated' ? ' (estimated floor)' : ''}; ${detected}.${licenseNote(model)}`;
}

function recommend(modality: MediaModality, profile: MediaHardwareProfile, catalog: MediaModel[]): MediaRecommendation {
  const candidates = catalog.filter((model) => model.modality === modality);
  if (candidates.length === 0) {
    return { modality, status: 'no-fit', model: null, lane: null, reason: 'No 2026 model is catalogued for this modality.' };
  }
  const newest = candidates.filter((model) => model.lane === 'newest').sort((left, right) => right.releasedAt.localeCompare(left.releasedAt));
  const efficient = candidates.filter((model) => model.lane === 'efficient').sort((left, right) => right.releasedAt.localeCompare(left.releasedAt));
  for (const model of [...newest, ...efficient]) {
    const status = fits(model, profile);
    if (status === 'ready' || status === 'cpu-only') {
      return {
        modality,
        status,
        model,
        lane: model.lane,
        reason:
          status === 'ready'
            ? readyReason(model, profile)
            : `CPU-only fallback for ${model.name}; expect slow generation on this machine. ${model.minRamGB}GB RAM required.${licenseNote(model)}`,
      };
    }
  }
  const ramOk = candidates.filter((model) => profile.totalRamGB >= model.minRamGB);
  if (ramOk.length > 0) {
    const candidate = [...ramOk].sort((left, right) => {
      const leftFloor = left.minVramGB ?? Number.POSITIVE_INFINITY;
      const rightFloor = right.minVramGB ?? Number.POSITIVE_INFINITY;
      return leftFloor - rightFloor || right.releasedAt.localeCompare(left.releasedAt);
    })[0];
    return { modality, status: 'needs-gpu', model: candidate, lane: candidate.lane, reason: needsGpuReason(candidate, profile) };
  }
  const closest = [...candidates].sort((left, right) => left.minRamGB - right.minRamGB)[0];
  return {
    modality,
    status: 'no-fit',
    model: null,
    lane: null,
    reason: `No 2026 model fits this hardware. The closest is ${closest.name}: ${requirementText(closest)}. Older models are not substituted.`,
  };
}

export function reportMediaHardware(profile: MediaHardwareProfile, catalog: MediaModel[] = MEDIA_CATALOG): MediaReport {
  return {
    profile,
    tier: mediaTier(profile),
    recommendations: MODALITIES.map((modality) => recommend(modality, profile, catalog)),
    catalog,
    policy: MEDIA_POLICY,
  };
}

/** Policy guard for catalog additions and tests. Empty means the catalog is valid. */
export function catalogViolations(catalog: MediaModel[] = MEDIA_CATALOG): string[] {
  const violations: string[] = [];
  const yearStart = `${MEDIA_POLICY.year}-01-01`;
  const seen = new Set<string>();
  for (const model of catalog) {
    if (seen.has(model.id)) {
      violations.push(`${model.id}: duplicate id`);
    }
    seen.add(model.id);
    if (!model.license.name) {
      violations.push(`${model.id}: missing license name`);
    }
    if (model.paramsB !== null && model.paramsB <= 0) {
      violations.push(`${model.id}: paramsB must be positive`);
    }
    if (model.steps !== null && (!Number.isInteger(model.steps) || model.steps <= 0)) {
      violations.push(`${model.id}: steps must be a positive integer`);
    }
    if (model.minVramGB !== null && model.minVramGB < 0) {
      violations.push(`${model.id}: minVramGB cannot be negative`);
    }
    if (model.releasedAt < yearStart) {
      violations.push(`${model.id}: first release ${model.releasedAt} is before ${MEDIA_POLICY.year}`);
    }
    if (model.lane === 'newest' && model.releasedAt < MEDIA_POLICY.newestSince) {
      violations.push(`${model.id}: newest-lane release ${model.releasedAt} is before ${MEDIA_POLICY.newestSince}`);
    }
    if (model.lane === 'efficient' && model.releasedAt >= MEDIA_POLICY.newestSince) {
      violations.push(`${model.id}: efficient-lane release ${model.releasedAt} belongs in the newest lane`);
    }
    if (!model.source.startsWith('https://')) {
      violations.push(`${model.id}: source is not an https URL`);
    }
    if (!model.workflow) {
      violations.push(`${model.id}: missing workflow preset`);
    }
    if (model.minVramGB === null && model.vramConfidence !== 'unknown') {
      violations.push(`${model.id}: unknown VRAM floor must be marked unknown`);
    }
    if (model.minVramGB !== null && model.vramConfidence === 'unknown') {
      violations.push(`${model.id}: a VRAM floor needs documented or estimated confidence`);
    }
    if (!Number.isInteger(model.minGpus) || model.minGpus < 0) {
      violations.push(`${model.id}: minGpus must be a non-negative integer`);
    }
    if (model.minRamGB <= 0) {
      violations.push(`${model.id}: minRamGB must be positive`);
    }
    if (model.license.class === 'permissive' && !model.license.commercial) {
      violations.push(`${model.id}: permissive license must be commercial-safe`);
    }
    if (model.license.class !== 'permissive' && model.license.commercial) {
      violations.push(`${model.id}: non-permissive license cannot claim the commercial default`);
    }
    if (LEGACY_MODEL_IDS.has(model.id)) {
      violations.push(`${model.id}: forbidden legacy model id`);
    }
  }
  return violations;
}
