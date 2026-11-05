import { describe, expect, it } from 'vitest';
import * as os from 'node:os';
import * as path from 'node:path';
import * as fs from 'node:fs/promises';
import { BlenderRunner } from '../electron/blenderRunner';

describe('Blender Binary Discovery & Version Parsing (Unit)', () => {
  it('parses standard Blender version output banner formats', () => {
    const runner = new BlenderRunner();
    const v4 = runner.parseVersionString('Blender 4.2.0 (hash 0a9e70183b06 built 2024-07-16 06:17:10)\n');
    expect(v4).toEqual({
      version: '4.2.0',
      major: 4,
      minor: 2,
      patch: 0,
    });

    const v3 = runner.parseVersionString('Blender 3.6.14 (sub 0)\n');
    expect(v3).toEqual({
      version: '3.6.14',
      major: 3,
      minor: 6,
      patch: 14,
    });

    const v5 = runner.parseVersionString('Blender 5.0\n');
    expect(v5).toEqual({
      version: '5.0.0',
      major: 5,
      minor: 0,
      patch: 0,
    });
  });

  it('returns null on malformed or non-blender version strings', () => {
    const runner = new BlenderRunner();
    expect(runner.parseVersionString('Python 3.13.1\n')).toBeNull();
    expect(runner.parseVersionString('')).toBeNull();
    expect(runner.parseVersionString('Random Tool v1.0.0')).toBeNull();
  });

  it('evaluates whether detected version meets minimum recommended requirements (>= 3.3)', async () => {
    const runner = new BlenderRunner();
    const status = await runner.getStatus();
    // In environments without Blender, installed is false and supported is false
    if (!status.installed) {
      expect(status.installed).toBe(false);
      expect(status.binaryPath).toBeNull();
      expect(status.version).toBeNull();
      expect(status.supported).toBe(false);
      expect(status.installGuideUrl).toBe('https://www.blender.org/download/');
    } else {
      expect(status.majorVersion).toBeGreaterThanOrEqual(3);
    }
  });

  it('handles missing binary gracefully without unhandled exceptions', async () => {
    const runner = new BlenderRunner();
    const res = await runner.discover({ customPath: 'C:\\nonexistent\\blender.exe', forceRefresh: true });
    expect(res).toBeNull();
  });

  it('prefers OPENRO_BLENDER_BIN or customPath override when provided', async () => {
    const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), 'openro-bin-test-'));
    const dummyExe = path.join(tempDir, process.platform === 'win32' ? 'fake_blender.bat' : 'fake_blender.sh');
    const scriptContent = process.platform === 'win32'
      ? '@echo off\r\necho Blender 4.2.1\r\n'
      : '#!/bin/sh\necho "Blender 4.2.1"\n';

    await fs.writeFile(dummyExe, scriptContent, { mode: 0o755 });

    try {
      const runner = new BlenderRunner();
      const info = await runner.discover({ customPath: dummyExe, forceRefresh: true });
      expect(info).not.toBeNull();
      expect(info?.version).toBe('4.2.1');
      expect(info?.major).toBe(4);
      expect(info?.minor).toBe(2);
      expect(info?.patch).toBe(1);
    } finally {
      await fs.rm(tempDir, { recursive: true, force: true });
    }
  });

  it('detect(customPath) refreshes cache and returns updated BlenderStatusView', async () => {
    const runner = new BlenderRunner();
    const status = await runner.detect('C:\\definitely\\not\\installed\\blender.exe');
    expect(status.installed).toBe(false);
    expect(status.installGuideUrl).toBe('https://www.blender.org/download/');
  });

  it('handles executable paths containing spaces safely', async () => {
    const tempDir = await fs.mkdtemp(path.join(os.tmpdir(), 'openro bin test with spaces-'));
    const dummyExe = path.join(tempDir, process.platform === 'win32' ? 'blender app.bat' : 'blender app.sh');
    const scriptContent = process.platform === 'win32'
      ? '@echo off\r\necho Blender 3.6.5\r\n'
      : '#!/bin/sh\necho "Blender 3.6.5"\n';

    await fs.writeFile(dummyExe, scriptContent, { mode: 0o755 });

    try {
      const runner = new BlenderRunner();
      const info = await runner.discover({ customPath: dummyExe, forceRefresh: true });
      expect(info).not.toBeNull();
      expect(info?.version).toBe('3.6.5');
      expect(info?.executablePath).toContain('with spaces');
    } finally {
      await fs.rm(tempDir, { recursive: true, force: true });
    }
  });

  it('getStatus matches the BlenderStatusView data contract', async () => {
    const runner = new BlenderRunner();
    const status = await runner.getStatus();
    expect(typeof status.installed).toBe('boolean');
    expect(typeof status.supported).toBe('boolean');
    expect(typeof status.installGuideUrl).toBe('string');
    expect(status.installGuideUrl).toContain('blender.org');
  });
});
