import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import type { CapabilityInfo } from './types';

const defaultExec = promisify(execFile);

export type CommandRunner = (
  command: string,
  args: string[],
  options: { timeout: number }
) => Promise<{ stdout: string; stderr?: string }>;

export interface CapabilityCheckDefinition {
  id: string;
  name: string;
  category: CapabilityInfo['category'];
  state: CapabilityInfo['state'];
  installUrl?: string;
  command: string;
  args: string[];
  parse: (stdout: string) => string;
  platform?: NodeJS.Platform;
}

/**
 * Optional capability detection (docs/ARCHITECTURE.md §4). Capabilities are
 * reported as adapters with explicit state, never auto-installed.
 */
export const capabilityChecks: CapabilityCheckDefinition[] = [
  {
    id: 'git',
    name: 'Git',
    category: 'vcs',
    state: 'ready',
    installUrl: 'https://git-scm.com/downloads',
    command: 'git',
    args: ['--version'],
    parse: (stdout) => stdout.trim(),
  },
  {
    id: 'rojo',
    name: 'Rojo',
    category: 'asset',
    state: 'experimental',
    installUrl: 'https://rojo.space/docs/v7/installation/',
    command: 'rojo',
    args: ['--version'],
    parse: (stdout) => stdout.trim(),
  },
  {
    id: 'blender',
    name: 'Blender',
    category: 'asset',
    state: 'experimental',
    installUrl: 'https://www.blender.org/download/',
    command: 'blender',
    args: ['--version'],
    parse: (stdout) => stdout.split('\n')[0]?.trim() ?? '',
  },
  {
    id: 'codex-cli',
    name: 'Codex CLI',
    category: 'provider-cli',
    state: 'ready',
    installUrl: 'https://chatgpt.com/codex',
    command: 'codex',
    args: ['--version'],
    parse: (stdout) => stdout.trim(),
  },
  {
    id: 'claude-code-cli',
    name: 'Claude Code CLI',
    category: 'provider-cli',
    state: 'ready',
    installUrl: 'https://code.claude.com/docs/en/setup',
    command: 'claude',
    args: ['--version'],
    parse: (stdout) => stdout.trim(),
  },
  {
    id: 'opencode-cli',
    name: 'OpenCode CLI',
    category: 'provider-cli',
    state: 'ready',
    installUrl: 'https://opencode.ai/download',
    command: 'opencode',
    args: ['--version'],
    parse: (stdout) => stdout.trim(),
  },
  {
    id: 'vinegar',
    name: 'Vinegar (Linux Roblox Studio runner)',
    category: 'studio',
    state: 'experimental',
    installUrl: 'https://vinegarhq.org/',
    command: 'vinegar',
    args: ['--version'],
    parse: (stdout) => stdout.trim(),
    platform: 'linux',
  },
];

export const studioCapability: CapabilityInfo = {
  id: 'roblox-studio',
  name: 'Roblox Studio MCP',
  category: 'studio',
  state: 'experimental',
  detail: 'Built into Roblox Studio. Enable it under Assistant, then Manage MCP Servers. Open-Ro does not launch Studio or install it for you.',
  installUrl: 'https://create.roblox.com/docs/studio/mcp',
};

export async function detectCapabilities(options?: {
  runner?: CommandRunner;
  platform?: NodeJS.Platform;
}): Promise<CapabilityInfo[]> {
  const runner = options?.runner ?? defaultExec;
  const currentPlatform = options?.platform ?? process.platform;

  const relevantChecks = capabilityChecks.filter((check) => {
    if (check.platform && check.platform !== currentPlatform) {
      return false;
    }
    return true;
  });

  const detected = await Promise.all(
    relevantChecks.map(async (check) => {
      try {
        const { stdout } = await runner(check.command, check.args, { timeout: 4_000 });
        return {
          id: check.id,
          name: check.name,
          category: check.category,
          state: (check.state === 'experimental' ? 'experimental' : 'ready') as CapabilityInfo['state'],
          detail: check.parse(stdout) || 'Detected locally.',
          installUrl: check.installUrl,
        };
      } catch {
        return {
          id: check.id,
          name: check.name,
          category: check.category,
          state: 'missing' as const,
          detail: 'Not detected on this machine.',
          installUrl: check.installUrl,
        };
      }
    }),
  );
  return [studioCapability, ...detected];
}
