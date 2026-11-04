import { readFileSync } from 'node:fs';
import { describe, expect, it } from 'vitest';
import {
  MEDIA_CATALOG,
  MEDIA_POLICY,
  catalogViolations,
  mediaTier,
  reportMediaHardware,
  type MediaHardwareProfile,
  type MediaModality,
  type MediaReport,
} from '../electron/mediaCatalog';

function profile(overrides: Partial<MediaHardwareProfile> = {}): MediaHardwareProfile {
  return {
    totalRamGB: 16,
    freeRamGB: 8,
    cpuModel: 'Test CPU',
    cores: 8,
    platform: 'win32',
    arch: 'x64',
    gpuName: null,
    vramGB: null,
    gpuVramGB: [],
    gpuCount: 0,
    vramSource: 'unknown',
    freeDiskGB: 200,
    ...overrides,
  };
}

function recommendation(report: MediaReport, modality: MediaModality) {
  const found = report.recommendations.find((entry) => entry.modality === modality);
  if (!found) {
    throw new Error(`Missing recommendation for ${modality}`);
  }
  return found;
}

describe('2026-only media catalog policy', () => {
  it('contains only valid 2026 releases and valid lane dates', () => {
    expect(catalogViolations()).toEqual([]);
    expect(MEDIA_CATALOG).toHaveLength(9);
    for (const model of MEDIA_CATALOG) {
      expect(model.releasedAt >= `${MEDIA_POLICY.year}-01-01`).toBe(true);
      if (model.lane === 'newest') {
        expect(model.releasedAt >= MEDIA_POLICY.newestSince).toBe(true);
      }
      expect(model.source.startsWith('https://')).toBe(true);
      expect(model.workflow.length).toBeGreaterThan(0);
    }
  });

  it('has no 2025 or legacy fallback entries', () => {
    const ids = MEDIA_CATALOG.map((model) => model.id);
    for (const legacy of ['sdxl', 'sd-1.5', 'musicgen', 'animatediff', 'svd', 'wan2.2-ti2v-5b', 'z-image-turbo', 'trellis-2', 'hunyuan3d-omni', 'triposr']) {
      expect(ids).not.toContain(legacy);
    }
    expect(MEDIA_CATALOG.some((model) => model.releasedAt.startsWith('2025'))).toBe(false);
  });

  it('rejects an injected 2025 model, a misplaced newest lane, and duplicate ids', () => {
    const legacy = { ...MEDIA_CATALOG[0], id: 'sdxl', releasedAt: '2023-07-26' };
    const legacyFlags = catalogViolations([...MEDIA_CATALOG, legacy]).join(' ');
    expect(legacyFlags).toContain('before 2026');
    expect(legacyFlags).toContain('forbidden legacy model id');
    const oldEfficient = { ...MEDIA_CATALOG[1], id: 'old-efficient', lane: 'efficient' as const, releasedAt: '2025-06-01' };
    expect(catalogViolations([...MEDIA_CATALOG, oldEfficient])).toContain('old-efficient: first release 2025-06-01 is before 2026');
    const misplaced = { ...MEDIA_CATALOG[1], id: 'old-newest', lane: 'newest' as const, releasedAt: '2026-03-01' };
    expect(catalogViolations([...MEDIA_CATALOG, misplaced]).join(' ')).toContain('before 2026-08-03');
    expect(catalogViolations([...MEDIA_CATALOG, { ...MEDIA_CATALOG[0] }]).join(' ')).toContain('duplicate id');
    const badFloor = { ...MEDIA_CATALOG[0], id: 'bad-floor', minVramGB: null, vramConfidence: 'documented' as const };
    expect(catalogViolations([...MEDIA_CATALOG, badFloor]).join(' ')).toContain('unknown VRAM floor must be marked unknown');
    const badConfidence = { ...MEDIA_CATALOG[0], id: 'bad-confidence', minVramGB: 8, vramConfidence: 'unknown' as const };
    expect(catalogViolations([...MEDIA_CATALOG, badConfidence]).join(' ')).toContain('a VRAM floor needs documented or estimated confidence');
    const fakeCommercial = { ...MEDIA_CATALOG[0], id: 'fake-commercial', license: { ...MEDIA_CATALOG[0].license, commercial: true } };
    expect(catalogViolations([...MEDIA_CATALOG, fakeCommercial]).join(' ')).toContain('non-permissive license cannot claim the commercial default');
  });

  it('keeps research and community licenses out of the commercial default', () => {
    const qwen = MEDIA_CATALOG.find((model) => model.id === 'qwen-image-2.1-lightning');
    expect(qwen?.license).toMatchObject({ class: 'research', commercial: false });
    expect(qwen?.notes).toContain('not the commercial default');
    const minimax = MEDIA_CATALOG.find((model) => model.id === 'minimax-music-3');
    expect(minimax?.license).toMatchObject({ class: 'community', commercial: false });
    const ace = MEDIA_CATALOG.find((model) => model.id === 'ace-step-1.5-turbo');
    expect(ace?.license).toMatchObject({ name: 'MIT', class: 'permissive', commercial: true });
  });
});

describe('hardware-tier recommendations', () => {
  it('this PC: audio CPU-only, voice ready, no false GPU readiness', () => {
    const report = reportMediaHardware(
      profile({ totalRamGB: 15.4, freeRamGB: 7.2, cpuModel: 'AMD Ryzen 7 5700U', cores: 16, vramGB: null, gpuCount: 0 }),
    );
    expect(report.tier).toBe('cpu');
    expect(recommendation(report, 'audio')).toMatchObject({ status: 'cpu-only', model: { id: 'ace-step-1.5-turbo' } });
    expect(recommendation(report, 'voice')).toMatchObject({ status: 'ready', model: { id: 'qwen3-tts-0.6b' } });
    const image = recommendation(report, 'image');
    expect(image).toMatchObject({ status: 'needs-gpu', model: { id: 'flux-2-klein-4b' } });
    expect(image.reason).toContain('13GB');
    const video = recommendation(report, 'video');
    expect(video.status).toBe('no-fit');
    expect(video.reason).toContain('No 2026 model fits');
    const threeD = recommendation(report, '3d');
    expect(threeD).toMatchObject({ status: 'needs-gpu', model: { id: 'idol' } });
    expect(threeD.reason).toContain('7.2GB');
    expect(threeD.reason).toContain('a CUDA device');
    expect(threeD.reason).not.toContain('CUDA/ROCm');
  });

  it('16GB VRAM picks the efficient 2026 image model, MiniMax audio, and the newest fitting 3D model', () => {
    const report = reportMediaHardware(
      profile({ totalRamGB: 32, vramGB: 16, gpuVramGB: [16], gpuCount: 1, gpuName: 'RTX 4080', vramSource: 'nvidia-smi' }),
    );
    expect(report.tier).toBe('16gb');
    expect(recommendation(report, 'image')).toMatchObject({ status: 'ready', lane: 'efficient', model: { id: 'flux-2-klein-4b' } });
    expect(recommendation(report, 'audio')).toMatchObject({ status: 'ready', lane: 'newest', model: { id: 'minimax-music-3' } });
    const video = recommendation(report, 'video');
    expect(video).toMatchObject({ status: 'needs-gpu', model: { id: 'sana-video-2.0-5b-4step' } });
    expect(video.reason).toContain('a CUDA device');
    expect(video.reason).not.toContain('CUDA/ROCm');
    expect(recommendation(report, '3d')).toMatchObject({ status: 'ready', model: { id: 'idol' } });
  });

  it('24GB VRAM promotes the newest image, audio, and video models', () => {
    const report = reportMediaHardware(
      profile({ totalRamGB: 64, vramGB: 24, gpuVramGB: [24], gpuCount: 1, gpuName: 'RTX 4090', vramSource: 'nvidia-smi' }),
    );
    expect(report.tier).toBe('24gb');
    expect(recommendation(report, 'image')).toMatchObject({ status: 'ready', lane: 'newest', model: { id: 'qwen-image-2.1-lightning' } });
    expect(recommendation(report, 'image').reason).toContain('not the commercial default');
    expect(recommendation(report, 'video')).toMatchObject({ status: 'ready', model: { id: 'sana-video-2.0-5b-4step' } });
    expect(recommendation(report, 'audio')).toMatchObject({ status: 'ready', lane: 'newest', model: { id: 'minimax-music-3' } });
  });

  it('dual 24GB GPUs keep MiniMax Music 3 and match per-GPU floors', () => {
    const report = reportMediaHardware(
      profile({ totalRamGB: 64, vramGB: 24, gpuVramGB: [24, 24], gpuCount: 2, gpuName: 'RTX 4090 + RTX 4090', vramSource: 'nvidia-smi' }),
    );
    expect(report.tier).toBe('multi-gpu');
    expect(recommendation(report, 'audio')).toMatchObject({ status: 'ready', lane: 'newest', model: { id: 'minimax-music-3' } });
    expect(recommendation(report, 'audio').reason).toContain('not the commercial default');

    const dualModel = {
      ...MEDIA_CATALOG.find((model) => model.id === 'minimax-music-3')!,
      id: 'dual-audio-test',
      minGpus: 2,
      minVramGB: 16,
      minRamGB: 32,
      vramConfidence: 'documented' as const,
    };
    const mixed = reportMediaHardware(
      profile({ totalRamGB: 64, vramGB: 24, gpuVramGB: [8, 24], gpuCount: 2 }),
      [dualModel],
    );
    expect(recommendation(mixed, 'audio')).toMatchObject({ status: 'needs-gpu', model: { id: 'dual-audio-test' } });
    const matched = reportMediaHardware(
      profile({ totalRamGB: 64, vramGB: 24, gpuVramGB: [24, 24], gpuCount: 2 }),
      [dualModel],
    );
    expect(recommendation(matched, 'audio')).toMatchObject({ status: 'ready', model: { id: 'dual-audio-test' } });
    const unmeasured = reportMediaHardware(
      profile({ totalRamGB: 64, vramGB: 24, gpuVramGB: [], gpuCount: 2 }),
      [dualModel],
    );
    expect(recommendation(unmeasured, 'audio')).toMatchObject({ status: 'needs-gpu', model: { id: 'dual-audio-test' } });
  });

  it('8GB VRAM still runs Idol and ACE-Step Turbo, but not FLUX.2 klein or video', () => {
    const report = reportMediaHardware(
      profile({ totalRamGB: 16, vramGB: 8, gpuVramGB: [8], gpuCount: 1, gpuName: 'RTX 3060', vramSource: 'nvidia-smi' }),
    );
    expect(report.tier).toBe('8gb');
    expect(recommendation(report, 'image')).toMatchObject({ status: 'needs-gpu', model: { id: 'flux-2-klein-4b' } });
    expect(recommendation(report, 'audio')).toMatchObject({ status: 'ready', model: { id: 'ace-step-1.5-turbo' } });
    expect(recommendation(report, '3d')).toMatchObject({ status: 'ready', model: { id: 'idol' } });
    expect(recommendation(report, 'video').status).toBe('no-fit');
  });

  it('unknown device memory never yields GPU readiness', () => {
    const report = reportMediaHardware(profile({ totalRamGB: 32, vramGB: null, gpuCount: 1, gpuName: 'AMD Radeon', vramSource: 'unknown' }));
    expect(recommendation(report, 'image').status).toBe('needs-gpu');
    expect(recommendation(report, 'audio')).toMatchObject({ status: 'cpu-only', model: { id: 'ace-step-1.5-turbo' } });
    expect(recommendation(report, '3d').status).toBe('needs-gpu');
  });

  it('never recommends an unknown-VRAM 3D floor as ready', () => {
    const pixal = MEDIA_CATALOG.find((model) => model.id === 'pixal3d');
    expect(pixal).toBeDefined();
    const report = reportMediaHardware(
      profile({ totalRamGB: 64, vramGB: 24, gpuVramGB: [24], gpuCount: 1 }),
      [pixal!],
    );
    const threeD = recommendation(report, '3d');
    expect(threeD).toMatchObject({ status: 'needs-gpu', model: { id: 'pixal3d' } });
    expect(threeD.reason).toContain('does not publish a VRAM floor');
  });

  it('low RAM returns no-fit instead of substituting an older model', () => {
    const report = reportMediaHardware(profile({ totalRamGB: 3, freeRamGB: 1, vramGB: null, gpuCount: 0 }));
    for (const modality of ['image', 'audio', 'voice', 'video', '3d'] as const) {
      const entry = recommendation(report, modality);
      expect(entry.status).toBe('no-fit');
      expect(entry.model).toBeNull();
      expect(entry.reason).toContain('No 2026 model');
    }
  });

  it('treats 15.4GB as a CPU tier, not as a fabricated GPU tier', () => {
    expect(mediaTier(profile({ totalRamGB: 15.4, vramGB: null, gpuCount: 0 }))).toBe('cpu');
    expect(mediaTier(profile({ totalRamGB: 32, vramGB: 7.9, gpuCount: 1 }))).toBe('cpu');
    expect(mediaTier(profile({ totalRamGB: 32, vramGB: 8, gpuCount: 1 }))).toBe('8gb');
    expect(mediaTier(profile({ totalRamGB: 32, vramGB: 12, gpuCount: 1 }))).toBe('12gb');
    expect(mediaTier(profile({ totalRamGB: 32, vramGB: 16, gpuCount: 1 }))).toBe('16gb');
    expect(mediaTier(profile({ totalRamGB: 64, vramGB: 24, gpuCount: 1 }))).toBe('24gb');
    expect(mediaTier(profile({ totalRamGB: 64, vramGB: 24, gpuCount: 2 }))).toBe('multi-gpu');
  });
});

describe('media hardware wiring', () => {
  it('is exposed through IPC, the preload allowlist, and the api', () => {
    const main = readFileSync('electron/main.ts', 'utf8');
    expect(main).toContain("ipcMain.handle('media:hardware'");
    expect(main).toContain('detectNvidiaGpu()');
    expect(main).toContain("fs.statfs(app.getPath('userData'))");
    expect(main).toContain('reportMediaHardware(profile)');
    expect(readFileSync('electron/preload.ts', 'utf8')).toContain("'media:hardware'");
    expect(readFileSync('src/api.ts', 'utf8')).toContain('mediaHardware');
  });
});
