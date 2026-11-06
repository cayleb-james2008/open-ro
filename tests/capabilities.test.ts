import { describe, expect, it } from 'vitest';
import {
  capabilityChecks,
  detectCapabilities,
  studioCapability,
  type CommandRunner,
} from '../electron/capabilities';

describe('Optional capability detection (electron/capabilities.ts)', () => {
  it('includes built-in Roblox Studio MCP capability as experimental', async () => {
    const capabilities = await detectCapabilities();
    expect(capabilities[0]).toEqual(studioCapability);
    expect(capabilities[0].id).toBe('roblox-studio');
    expect(capabilities[0].state).toBe('experimental');
  });

  it('runs on the active host and detects installed Git cleanly', async () => {
    const capabilities = await detectCapabilities();
    const git = capabilities.find((c) => c.id === 'git');
    expect(git).toBeDefined();
    expect(git?.state).toBe('ready');
    expect(git?.detail).toMatch(/^git version/i);
    expect(git?.category).toBe('vcs');
  });

  it('marks present tools ready and parses their versions accurately', async () => {
    const fakeRunner: CommandRunner = async (cmd) => {
      switch (cmd) {
        case 'git':
          return { stdout: 'git version 2.45.2.windows.1\n' };
        case 'rojo':
          return { stdout: 'Rojo 7.4.1\n' };
        case 'blender':
          return { stdout: 'Blender 4.2.0 LTS\nBuild: 2026-07-15\n' };
        case 'codex':
          return { stdout: 'codex 0.8.2\n' };
        case 'claude':
          return { stdout: 'claude-code 1.0.5\n' };
        case 'opencode':
          return { stdout: 'opencode 0.3.1\n' };
        case 'vinegar':
          return { stdout: 'vinegar 1.7.0\n' };
        default:
          throw new Error('command not found');
      }
    };

    const capabilities = await detectCapabilities({ runner: fakeRunner, platform: 'linux' });
    expect(capabilities.length).toBe(capabilityChecks.length + 1);

    const blender = capabilities.find((c) => c.id === 'blender');
    expect(blender?.state).toBe('experimental');
    expect(blender?.detail).toBe('Blender 4.2.0 LTS');

    const rojo = capabilities.find((c) => c.id === 'rojo');
    expect(rojo?.state).toBe('experimental');
    expect(rojo?.detail).toBe('Rojo 7.4.1');

    const vinegar = capabilities.find((c) => c.id === 'vinegar');
    expect(vinegar?.state).toBe('experimental');
    expect(vinegar?.detail).toBe('vinegar 1.7.0');

    const codex = capabilities.find((c) => c.id === 'codex-cli');
    expect(codex?.state).toBe('ready');
    expect(codex?.detail).toBe('codex 0.8.2');
  });

  it('fails safely to state missing when executables are not found', async () => {
    const missingRunner: CommandRunner = async () => {
      const err = new Error('spawn ENOENT');
      Object.assign(err, { code: 'ENOENT' });
      throw err;
    };

    const capabilities = await detectCapabilities({ runner: missingRunner, platform: 'linux' });
    const dynamicCapabilities = capabilities.slice(1);

    for (const cap of dynamicCapabilities) {
      expect(cap.state).toBe('missing');
      expect(cap.detail).toBe('Not detected on this machine.');
      expect(cap.installUrl).toBeDefined();
    }
  });

  it('handles timeouts gracefully and reports missing', async () => {
    const timeoutRunner: CommandRunner = async () => {
      const err = new Error('timed out after 4000ms');
      Object.assign(err, { killed: true });
      throw err;
    };

    const capabilities = await detectCapabilities({ runner: timeoutRunner, platform: 'win32' });
    const dynamicCapabilities = capabilities.slice(1);

    for (const cap of dynamicCapabilities) {
      expect(cap.state).toBe('missing');
    }
  });

  it('filters platform-specific checks (e.g. vinegar is checked only on linux)', async () => {
    const runner: CommandRunner = async () => ({ stdout: '1.0.0' });

    const windowsCaps = await detectCapabilities({ runner, platform: 'win32' });
    expect(windowsCaps.some((c) => c.id === 'vinegar')).toBe(false);

    const linuxCaps = await detectCapabilities({ runner, platform: 'linux' });
    expect(linuxCaps.some((c) => c.id === 'vinegar')).toBe(true);
  });
});
