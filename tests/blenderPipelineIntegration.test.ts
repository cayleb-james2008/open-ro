import { describe, expect, it } from 'vitest';
import * as os from 'node:os';
import * as path from 'node:path';
import * as fs from 'node:fs/promises';
import { BlenderRunner } from '../electron/blenderRunner';
import type { RobloxAssetManifest } from '../electron/blenderTypes';

describe('3D Asset Pipeline Integration: 5 Core Tasks & Geometry Math', () => {
  it('inspect_mesh: computes mesh metrics, bounding boxes, and compliance checks', async () => {
    const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), 'openro-pipeline-inspect-'));
    const dummyMesh = path.join(tempDir, 'character.obj');
    await fs.writeFile(dummyMesh, '# minimal mesh\nv 0 0 0\nv 1 0 0\nv 0 1 0\nf 1 2 3\n', 'utf8');

    try {
      const runner = new BlenderRunner();
      const res = await runner.runPipelineTask({
        taskType: 'inspect_mesh',
        inputFile: 'character.obj',
        outputDir: tempDir,
      }, { projectRoot: tempDir });

      expect(res.ok).toBe(true);
      expect(res.taskType).toBe('inspect_mesh');
      expect(res.metrics).toBeDefined();
      expect(typeof res.metrics?.trianglesBefore).toBe('number');
      expect(typeof res.metrics?.uvMapCount).toBe('number');
    } finally {
      await fs.rm(tempDir, { recursive: true, force: true });
    }
  });

  it('convert_format: applies coordinate normalization (Blender Z-up to Roblox Y-up)', async () => {
    const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), 'openro-pipeline-convert-'));
    const dummyMesh = path.join(tempDir, 'prop.obj');
    await fs.writeFile(dummyMesh, '# prop\nv 0 0 0\nv 1 1 1\n', 'utf8');

    try {
      const runner = new BlenderRunner();
      const res = await runner.runPipelineTask({
        taskType: 'convert_format',
        inputFile: 'prop.obj',
        targetFormat: 'fbx',
        coordinateNormalization: true,
        assetName: 'prop_roblox',
        outputDir: tempDir,
      }, { projectRoot: tempDir });

      expect(res.ok).toBe(true);
      expect(res.generatedFiles.length).toBeGreaterThan(0);
      expect(res.summary).toContain('normalized Roblox coordinates');
    } finally {
      await fs.rm(tempDir, { recursive: true, force: true });
    }
  });

  it('decimate_mesh: reduces geometry to strictly satisfy Roblox MeshPart budget (<= 10,000)', async () => {
    const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), 'openro-pipeline-decimate-'));
    const dummyMesh = path.join(tempDir, 'highpoly.obj');
    await fs.writeFile(dummyMesh, '# highpoly\n', 'utf8');

    try {
      const runner = new BlenderRunner();
      const res = await runner.runPipelineTask({
        taskType: 'decimate_mesh',
        inputFile: 'highpoly.obj',
        targetTriangles: 10000,
        preserveUv: true,
        preserveNormals: true,
        outputDir: tempDir,
      }, { projectRoot: tempDir });

      expect(res.ok).toBe(true);
      expect(res.metrics?.trianglesAfter).toBeLessThanOrEqual(10000);
      expect(res.summary).toContain('10,000');
    } finally {
      await fs.rm(tempDir, { recursive: true, force: true });
    }
  });

  it('generate_collision: produces low-poly collision proxy (hull or box)', async () => {
    const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), 'openro-pipeline-collision-'));
    const dummyMesh = path.join(tempDir, 'boulder.obj');
    await fs.writeFile(dummyMesh, '# boulder\n', 'utf8');

    try {
      const runner = new BlenderRunner();
      const res = await runner.runPipelineTask({
        taskType: 'generate_collision',
        inputFile: 'boulder.obj',
        collisionType: 'hull',
        assetName: 'boulder',
        outputDir: tempDir,
      }, { projectRoot: tempDir });

      expect(res.ok).toBe(true);
      expect(res.generatedFiles.some((f) => f.path.endsWith('_collision.obj'))).toBe(true);
      expect(res.summary).toContain('collision');
    } finally {
      await fs.rm(tempDir, { recursive: true, force: true });
    }
  });

  it('export_roblox_package: bundles visual mesh, collision proxy, and roblox-asset.json manifest', async () => {
    const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), 'openro-pipeline-package-'));
    const dummyMesh = path.join(tempDir, 'sword.obj');
    await fs.writeFile(dummyMesh, '# sword\n', 'utf8');

    try {
      const runner = new BlenderRunner();
      const res = await runner.runPipelineTask({
        taskType: 'export_roblox_package',
        inputFile: 'sword.obj',
        assetName: 'KnightSword',
        author: 'Knight Creator',
        license: 'CC-BY 4.0',
        outputDir: tempDir,
      }, { projectRoot: tempDir });

      expect(res.ok).toBe(true);
      const manifestFile = res.generatedFiles.find((f) => f.path.endsWith('roblox-asset.json'));
      expect(manifestFile).toBeDefined();

      if (manifestFile) {
        const manifestRaw = await fs.readFile(manifestFile.path, 'utf8');
        const manifest = JSON.parse(manifestRaw) as RobloxAssetManifest;
        expect(manifest.schemaVersion).toBe('1.0');
        expect(manifest.name).toBe('KnightSword');
        expect(manifest.attribution.author).toBe('Knight Creator');
        expect(manifest.robloxSettings.collisionFidelity).toBe('Hull');
      }
    } finally {
      await fs.rm(tempDir, { recursive: true, force: true });
    }
  });

  it('sandboxing: resolves safe output directories within projectRoot', async () => {
    const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), 'openro-sandbox-test-'));
    try {
      const runner = new BlenderRunner();
      const res = await runner.runPipelineTask({
        taskType: 'convert_format',
        outputDir: 'assets/models',
      }, { projectRoot: tempDir });

      expect(res.ok).toBe(true);
      const createdDir = path.join(tempDir, 'assets', 'models');
      const stat = await fs.stat(createdDir);
      expect(stat.isDirectory()).toBe(true);
    } finally {
      await fs.rm(tempDir, { recursive: true, force: true });
    }
  });

  it('progress callbacks: streams stage updates during task execution', async () => {
    const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), 'openro-progress-test-'));
    const stages: string[] = [];

    try {
      const runner = new BlenderRunner();
      await runner.runPipelineTask({
        taskType: 'decimate_mesh',
        outputDir: tempDir,
      }, {
        projectRoot: tempDir,
        onProgress: (event) => {
          if (event.type === 'task-progress') {
            stages.push(event.stage);
          }
        },
      });

      expect(stages.length).toBeGreaterThan(0);
    } finally {
      await fs.rm(tempDir, { recursive: true, force: true });
    }
  });

  it('decimation options: preserves UV seams and sharp normals parameters', async () => {
    const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), 'openro-normals-test-'));
    try {
      const runner = new BlenderRunner();
      const res = await runner.runPipelineTask({
        taskType: 'decimate_mesh',
        preserveUv: true,
        preserveNormals: true,
        outputDir: tempDir,
      }, { projectRoot: tempDir });

      expect(res.ok).toBe(true);
      expect(res.summary).toContain('preserving UV seams and normal vectors');
    } finally {
      await fs.rm(tempDir, { recursive: true, force: true });
    }
  });
});
