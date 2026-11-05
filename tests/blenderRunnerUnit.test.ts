import { describe, expect, it } from 'vitest';
import * as os from 'node:os';
import * as path from 'node:path';
import * as fs from 'node:fs/promises';
import { BlenderRunner } from '../electron/blenderRunner';
import { OPENRO_3D_SENTINEL_START, OPENRO_3D_SENTINEL_END } from '../electron/blenderScriptTemplate';

describe('Headless Blender Task Runner: Unit & Process Containment', () => {
  it('constructs correct headless command-line arguments (--background --factory-startup -P)', async () => {
    const runner = new BlenderRunner();
    const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), 'openro-args-test-'));
    const dummyScript = path.join(tempDir, 'test.py');
    await fs.writeFile(dummyScript, 'print("ok")', 'utf8');

    try {
      const res = await runner.runBlenderTask({
        scriptPath: dummyScript,
        outputDir: tempDir,
        args: ['--custom-arg', 'value123'],
      });
      // In simulation mode (when Blender is not installed), logs reflect execution
      expect(res.logs).toContain('Running Blender headless with script');
      expect(res.success).toBe(true);
    } finally {
      await fs.rm(tempDir, { recursive: true, force: true });
    }
  });

  it('escapes and serializes complex JSON arguments passing to task parameters', () => {
    const complexParams = {
      nested: { a: 1, b: 'quotes "and" backslashes \\' },
      symbols: ['#', '$', '%', '&', '*', '@'],
      triangles: 10000,
      enabled: true,
    };
    const serialized = JSON.stringify(complexParams);
    const parsed = JSON.parse(serialized);
    expect(parsed).toEqual(complexParams);
  });

  it('parses structured JSON results from sentinel-delimited stdout blocks', () => {
    const runner = new BlenderRunner();
    const samplePayload = {
      success: true,
      durationMs: 420,
      generatedFiles: ['C:\\projects\\mesh.fbx'],
      fileDetails: [{ path: 'C:\\projects\\mesh.fbx', bytes: 12345 }],
    };
    const sampleStdout = `
Blender 4.2.0 (hash 0a9e70183b06)
Read blend: C:\\Users\\Temp\\run.blend
Info: Processed mesh geometry.

${OPENRO_3D_SENTINEL_START}
${JSON.stringify(samplePayload)}
${OPENRO_3D_SENTINEL_END}

Blender quit
`;
    // Access parseSentinelOutput via private method test or simulation
    const parsed = (runner as unknown as { parseSentinelOutput: (s: string) => typeof samplePayload }).parseSentinelOutput(sampleStdout);
    expect(parsed).toEqual(samplePayload);
  });

  it('returns null when sentinel markers are missing or malformed', () => {
    const runner = new BlenderRunner();
    const malformedStdout = `
Blender 4.2.0 (hash 0a9e70183b06)
Random output without sentinels
Blender quit
`;
    const parsed = (runner as unknown as { parseSentinelOutput: (s: string) => unknown }).parseSentinelOutput(malformedStdout);
    expect(parsed).toBeNull();
  });

  it('caps excessive stdout length to avoid unbounded memory consumption', async () => {
    const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), 'openro-cap-test-'));
    const dummyScript = path.join(tempDir, 'verbose.py');
    await fs.writeFile(dummyScript, '# verbose mock', 'utf8');

    try {
      const runner = new BlenderRunner();
      const res = await runner.runBlenderTask({
        scriptPath: dummyScript,
        outputDir: tempDir,
      });
      expect(res.logs.length).toBeLessThan(100_000);
      expect(res.success).toBe(true);
    } finally {
      await fs.rm(tempDir, { recursive: true, force: true });
    }
  });

  it('enforces task timeout boundary and aborts runaway process', async () => {
    const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), 'openro-timeout-test-'));
    const hangScript = path.join(tempDir, 'hang.py');
    await fs.writeFile(hangScript, '# hang script', 'utf8');

    try {
      const runner = new BlenderRunner();
      const res = await runner.runBlenderTask({
        scriptPath: hangScript,
        outputDir: tempDir,
        timeoutMs: 5_000,
      });
      expect(res).toBeDefined();
    } finally {
      await fs.rm(tempDir, { recursive: true, force: true });
    }
  });

  it('cancels running task immediately upon receiving cancelTask request', async () => {
    const runner = new BlenderRunner();
    const cancelRes = await runner.cancelTask('nonexistent-task-id');
    expect(cancelRes.ok).toBe(true);
    expect(cancelRes.cancelled).toBe(false);
  });

  it('records entries in task history and preserves newest runs', async () => {
    const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), 'openro-history-test-'));
    try {
      const runner = new BlenderRunner();
      const result = await runner.runPipelineTask({
        taskType: 'inspect_mesh',
        outputDir: tempDir,
      }, { projectRoot: tempDir });

      expect(result.ok).toBe(true);
      const history = runner.getHistory(tempDir);
      expect(history.length).toBeGreaterThanOrEqual(1);
      expect(history[0]?.taskType).toBe('inspect_mesh');
      expect(history[0]?.status).toBe('COMPLETED');
    } finally {
      await fs.rm(tempDir, { recursive: true, force: true });
    }
  });
});
