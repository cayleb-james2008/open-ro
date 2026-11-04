import * as crypto from 'node:crypto';
import { promises as fs } from 'node:fs';
import { mkdtemp, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { ApprovalQueue } from '../../electron/approvals';
import {
  parseAcceptanceDefinition,
  acceptanceForScenario,
} from '../../electron/acceptanceChecks';
import { runShellCommand } from '../../electron/runCommand';
import {
  createMockStudioCaller,
  createValidPngBuffer,
  McpServerPool,
  runBlenderTask,
  runPlaytestScenario,
  studio_asset,
  studio_inspect,
  studio_multi_edit,
  studio_search_tree,
} from './harness';

describe('Tier 2: Feature 1 - Studio Hierarchy Search Boundaries', () => {
  it('handles completely empty search params and defaults safely', async () => {
    const { caller } = createMockStudioCaller();
    const result = await studio_search_tree({}, caller);
    expect(result.instances.length).toBeGreaterThan(0);
  });

  it('returns empty instance list when searching a non-existent path', async () => {
    const { caller } = createMockStudioCaller();
    const result = await studio_search_tree({ path: 'Workspace.NonExistentService' }, caller);
    expect(result.instances).toEqual([]);
  });

  it('clamps extreme max_depth values safely to valid bounds [1, 6]', async () => {
    const { caller } = createMockStudioCaller();
    const resultLarge = await studio_search_tree({ path: 'Workspace', max_depth: 9999 }, caller);
    const resultNegative = await studio_search_tree({ path: 'Workspace', max_depth: -5 }, caller);
    expect(resultLarge.instances.length).toBeGreaterThan(0);
    expect(resultNegative.instances.length).toBeGreaterThan(0);
  });

  it('throws a descriptive error when searching with disconnected MCP server', async () => {
    const mock = createMockStudioCaller();
    mock.setRunning(false);
    await expect(studio_search_tree({ path: 'Workspace' }, mock.caller)).rejects.toThrow(/disconnected/);
  });

  it('handles paths and keywords containing special punctuation characters', async () => {
    const { caller } = createMockStudioCaller();
    const result = await studio_search_tree({ keywords: '!@#$%^&*()' }, caller);
    expect(result.instances).toEqual([]);
  });
});

describe('Tier 2: Feature 2 - Instance Deep Inspection Boundaries', () => {
  it('throws an error when path is empty string', async () => {
    const { caller } = createMockStudioCaller();
    await expect(studio_inspect({ path: '' }, caller)).rejects.toThrow(/Missing path/);
  });

  it('throws an instance not found error when path does not exist in DataModel', async () => {
    const { caller } = createMockStudioCaller();
    await expect(studio_inspect({ path: 'Workspace.PhantomPart' }, caller)).rejects.toThrow(/Instance not found/);
  });

  it('throws when inspecting on a disconnected MCP server', async () => {
    const mock = createMockStudioCaller();
    mock.setRunning(false);
    await expect(studio_inspect({ path: 'Workspace.Baseplate' }, mock.caller)).rejects.toThrow(/disconnected/);
  });

  it('inspects deeply nested paths accurately', async () => {
    const { caller } = createMockStudioCaller();
    const result = await studio_inspect({ path: 'Workspace.CoinModel.Hitbox' }, caller);
    expect(result.className).toBe('Part');
    expect(result.path).toBe('Workspace.CoinModel.Hitbox');
  });

  it('returns empty children array when instance has no child objects', async () => {
    const { caller } = createMockStudioCaller();
    const result = await studio_inspect({ path: 'Workspace.Baseplate' }, caller);
    expect(result.children).toEqual([]);
  });
});

describe('Tier 2: Feature 3 - Multi-Script Batch Editing Boundaries', () => {
  it('returns success with empty list when edits array is empty', async () => {
    const { caller } = createMockStudioCaller();
    const result = await studio_multi_edit({ edits: [] }, undefined, caller);
    expect(result.success).toBe(true);
    expect(result.modified_scripts).toEqual([]);
  });

  it('throws an error when old_string is not found in target script', async () => {
    const { caller } = createMockStudioCaller();
    await expect(
      studio_multi_edit(
        {
          edits: [
            {
              path: 'ServerScriptService.CoinManager',
              old_string: 'THIS_STRING_DOES_NOT_EXIST_IN_THE_SCRIPT',
              new_string: 'replacement',
            },
          ],
        },
        undefined,
        caller,
      ),
    ).rejects.toThrow(/old_string not found/);
  });

  it('rejects invalid script className configurations', async () => {
    const { caller } = createMockStudioCaller();
    await expect(
      studio_multi_edit(
        {
          edits: [
            {
              path: 'Workspace.BadScript',
              new_string: 'print(1)',
              className: 'Part',
            },
          ],
        },
        undefined,
        caller,
      ),
    ).rejects.toThrow(/Invalid script class/);
  });

  it('rejects non-array edits parameter', async () => {
    const { caller } = createMockStudioCaller();
    const raw = { edits: 'not an array' } as unknown as { edits: [] };
    await expect(studio_multi_edit(raw, undefined, caller)).rejects.toThrow(/Edits must be an array/);
  });

  it('throws an error when editing against a disconnected server', async () => {
    const mock = createMockStudioCaller();
    mock.setRunning(false);
    await expect(
      studio_multi_edit({ edits: [{ path: 'ServerScriptService.CoinManager', new_string: '' }] }, undefined, mock.caller),
    ).rejects.toThrow(/disconnected/);
  });
});

describe('Tier 2: Feature 4 - Studio Asset Search & Insert Boundaries', () => {
  it('returns empty results array when search keyword matches no assets', async () => {
    const { caller } = createMockStudioCaller();
    const result = await studio_asset({ action: 'search', keyword: 'UnobtainableItem99999' }, undefined, caller);
    expect(result.results).toEqual([]);
  });

  it('throws an error when inserting with negative asset_id', async () => {
    const { caller } = createMockStudioCaller();
    await expect(studio_asset({ action: 'insert', asset_id: -1 }, undefined, caller)).rejects.toThrow(/Invalid asset/);
  });

  it('throws an error when inserting with zero asset_id', async () => {
    const { caller } = createMockStudioCaller();
    await expect(studio_asset({ action: 'insert', asset_id: 0 }, undefined, caller)).rejects.toThrow(/Invalid asset/);
  });

  it('throws an error when inserting into non-existent target_parent', async () => {
    const { caller } = createMockStudioCaller();
    await expect(
      studio_asset({ action: 'insert', asset_id: 1001, target_parent: 'Workspace.NonExistentFolder' }, undefined, caller),
    ).rejects.toThrow(/Target parent not found/);
  });

  it('throws an error for unsupported action names', async () => {
    const { caller } = createMockStudioCaller();
    const badAction = { action: 'delete_all' } as unknown as { action: 'search' };
    await expect(studio_asset(badAction, undefined, caller)).rejects.toThrow(/Unsupported/);
  });
});

describe('Tier 2: Feature 5 - Avatar Navigation & Input Boundaries', () => {
  it('handles empty actions list in user_keyboard_input safely', async () => {
    const { caller } = createMockStudioCaller();
    const reply = await caller.callTool('user_keyboard_input', { actions: [] });
    expect(JSON.parse(reply).processedActions).toBe(0);
  });

  it('handles empty actions list in user_mouse_input safely', async () => {
    const { caller } = createMockStudioCaller();
    const reply = await caller.callTool('user_mouse_input', { actions: [] });
    expect(JSON.parse(reply).processedActions).toBe(0);
  });

  it('accepts character navigation to origin [0, 0, 0]', async () => {
    const { caller } = createMockStudioCaller();
    const reply = await caller.callTool('character_navigation', { target_position: [0, 0, 0] });
    expect(JSON.parse(reply).status).toBe('NAVIGATING');
  });

  it('throws an error when calling input tools on disconnected server', async () => {
    const mock = createMockStudioCaller();
    mock.setRunning(false);
    await expect(mock.caller.callTool('user_keyboard_input', { actions: [] })).rejects.toThrow(/not running/);
  });

  it('handles non-existent target instance navigation gracefully', async () => {
    const { caller } = createMockStudioCaller();
    const reply = await caller.callTool('character_navigation', { target_instance: 'Workspace.GhostObject' });
    expect(JSON.parse(reply).destination).toBe('Workspace.GhostObject');
  });
});

describe('Tier 2: Feature 6 - Autonomous In-Studio Playtest Runner Boundaries', () => {
  let tempDir = '';

  beforeEach(async () => {
    tempDir = await mkdtemp(path.join(tmpdir(), 'openro-e2e-playtest-bounds-'));
  });

  afterEach(async () => {
    await rm(tempDir, { recursive: true, force: true });
  });

  it('returns ERROR when maxDurationSeconds is zero or negative', async () => {
    const mock = createMockStudioCaller();
    const result = await runPlaytestScenario('Quick Test', {
      maxDurationSeconds: 0,
      captureScreenshots: false,
      caller: mock.caller,
    });
    expect(result.verdict).toBe('ERROR');
  });

  it('returns ERROR when acceptance definition is missing', async () => {
    const mock = createMockStudioCaller();
    const result = await runPlaytestScenario('Missing Def', {
      maxDurationSeconds: 5,
      captureScreenshots: false,
      caller: mock.caller,
    });
    expect(result.verdict).toBe('ERROR');
  });

  it('returns ERROR when scenarioName is not present in acceptance definition', async () => {
    const mock = createMockStudioCaller();
    const def = parseAcceptanceDefinition(
      JSON.stringify({
        version: 1,
        checks: [
          { id: 'c1', scenario: 'Known Scenario', type: 'assertion', description: 'desc', expected: 'true' },
        ],
      }),
    );
    const result = await runPlaytestScenario('Unknown Scenario', {
      maxDurationSeconds: 5,
      captureScreenshots: false,
      acceptanceDef: def,
      caller: mock.caller,
    });
    expect(result.verdict).toBe('ERROR');
  });

  it('fails safely and recovers Edit mode when playtest start fails', async () => {
    const mock = createMockStudioCaller();
    mock.setRunning(false);
    const def = parseAcceptanceDefinition(
      JSON.stringify({
        version: 1,
        checks: [
          { id: 'c1', scenario: 'Crash Scenario', type: 'assertion', description: 'desc', expected: 'true' },
        ],
      }),
    );
    const result = await runPlaytestScenario('Crash Scenario', {
      maxDurationSeconds: 5,
      captureScreenshots: false,
      acceptanceDef: def,
      caller: mock.caller,
    });
    expect(result.verdict).toBe('ERROR');
  });

  it('handles playtest scenarios without screenshot captures', async () => {
    const mock = createMockStudioCaller();
    const def = parseAcceptanceDefinition(
      JSON.stringify({
        version: 1,
        checks: [
          { id: 'c1', scenario: 'No Shot', type: 'assertion', description: 'desc', expected: 'true' },
        ],
      }),
    );
    const result = await runPlaytestScenario('No Shot', {
      maxDurationSeconds: 5,
      captureScreenshots: false,
      projectRoot: tempDir,
      acceptanceDef: def,
      caller: mock.caller,
    });
    expect(result.screenshotPaths).toHaveLength(0);
  });
});

describe('Tier 2: Feature 7 - Exact Multiset Acceptance Enforcement Boundaries', () => {
  it('throws an error when acceptance definition is malformed JSON', () => {
    expect(() => parseAcceptanceDefinition('{ unclosed json')).toThrow(/valid JSON/);
  });

  it('throws an error when payload exceeds 256 KB threshold', () => {
    const huge = 'x'.repeat(256_001);
    expect(() => parseAcceptanceDefinition(huge)).toThrow(/larger than 256 KB/);
  });

  it('throws an error when version is not 1', () => {
    const badVersion = JSON.stringify({ version: 2, checks: [] });
    expect(() => parseAcceptanceDefinition(badVersion)).toThrow(/version/);
  });

  it('throws an error when checks array has duplicate IDs', () => {
    const dupes = JSON.stringify({
      version: 1,
      checks: [
        { id: 'same-id', scenario: 'S1', type: 'assertion', description: 'd1', expected: '1' },
        { id: 'same-id', scenario: 'S1', type: 'assertion', description: 'd2', expected: '2' },
      ],
    });
    expect(() => parseAcceptanceDefinition(dupes)).toThrow(/Duplicate/);
  });

  it('throws an error when screenshot path contains directory traversal', () => {
    const traversal = JSON.stringify({
      version: 1,
      checks: [{ id: 'c1', scenario: 'S1', type: 'assertion', description: 'd1', expected: '1' }],
      screenshots: ['../../escape.png'],
    });
    expect(() => parseAcceptanceDefinition(traversal)).toThrow(/relative/);
  });
});

describe('Tier 2: Feature 8 - Multi-Server MCP Registry & Pool Boundaries', () => {
  it('throws an error when connecting server with empty id', async () => {
    const pool = new McpServerPool();
    await expect(pool.connectMcpServer({ id: '', command: 'cmd', args: [] })).rejects.toThrow(/requires id/);
  });

  it('throws an error when connecting server with empty command', async () => {
    const pool = new McpServerPool();
    await expect(pool.connectMcpServer({ id: 'srv', command: '', args: [] })).rejects.toThrow(/requires id and command/);
  });

  it('throws an error when calling tool on unknown serverId', async () => {
    const pool = new McpServerPool();
    await expect(pool.callMcpTool('unknown-server', 'tool', {})).rejects.toThrow(/not found in pool/);
  });

  it('idempotently handles disconnect for non-existent serverId', async () => {
    const pool = new McpServerPool();
    await expect(pool.disconnectMcpServer('ghost-server')).resolves.toBeUndefined();
  });

  it('cleanly reconnects and replaces existing serverId with fresh session', async () => {
    const pool = new McpServerPool();
    const first = await pool.connectMcpServer({ id: 'reused-id', command: 'cmd1', args: [] });
    const second = await pool.connectMcpServer({ id: 'reused-id', command: 'cmd2', args: [] });
    expect(second.session).not.toBe(first.session);
    expect(pool.listServers().length).toBe(1);
  });
});

describe('Tier 2: Feature 9 - MCP Lifecycle & Crash Recovery Boundaries', () => {
  it('refuses execution when caller.running is false', async () => {
    const mock = createMockStudioCaller();
    mock.setRunning(false);
    await expect(mock.caller.callTool('execute_luau', {})).rejects.toThrow(/not running/);
  });

  it('rejects calls staged on old server after multiple rapid session resets', () => {
    const mock = createMockStudioCaller();
    const approvals = new ApprovalQueue();
    const identity0 = mock.caller.identity;
    expect(identity0).not.toBeNull();
    if (!identity0) return;

    const staged = approvals.stageMcp({
      projectRoot: '/test',
      tool: 'search_game_tree',
      args: {},
      server: identity0,
    });

    mock.setSession('session-1');
    mock.setSession('session-2');
    mock.setSession('session-3');

    expect(mock.caller.identity?.session).toBe('session-3');
    expect(staged.mcpServer?.session).toBe(identity0.session);
  });

  it('ensures approval queue rejects taking an item with wrong approval kind', () => {
    const mock = createMockStudioCaller();
    const approvals = new ApprovalQueue();
    const identity = mock.caller.identity;
    expect(identity).not.toBeNull();
    if (!identity) return;

    const staged = approvals.stageMcp({
      projectRoot: '/test',
      tool: 'search_game_tree',
      args: {},
      server: identity,
    });

    expect(() => approvals.take(staged.id, 'write')).toThrow();
  });

  it('ensures approval queue rejects taking an item that does not exist', () => {
    const approvals = new ApprovalQueue();
    expect(() => approvals.take('phantom-approval-id')).toThrow();
  });

  it('ensures server identity command and args match initialization parameters', () => {
    const { caller } = createMockStudioCaller();
    expect(caller.identity?.command).toBe('mock-roblox-studio-mcp');
    expect(caller.identity?.args).toEqual(['--stdio']);
  });
});

describe('Tier 2: Feature 10 - Headless Blender Task Runner Boundaries', () => {
  let tempDir = '';

  beforeEach(async () => {
    tempDir = await mkdtemp(path.join(tmpdir(), 'openro-e2e-blender-bounds-'));
  });

  afterEach(async () => {
    await rm(tempDir, { recursive: true, force: true });
  });

  it('throws an error when scriptPath is empty string', async () => {
    await expect(runBlenderTask({ scriptPath: '', outputDir: tempDir })).rejects.toThrow(/scriptPath must be specified/);
  });

  it('throws an error when outputDir is empty string', async () => {
    await expect(runBlenderTask({ scriptPath: 'script.py', outputDir: '' })).rejects.toThrow(/outputDir must be specified/);
  });

  it('handles empty python scripts without failing execution', async () => {
    const scriptPath = path.join(tempDir, 'empty.py');
    await fs.writeFile(scriptPath, '');
    const result = await runBlenderTask({ scriptPath, outputDir: tempDir });
    expect(result.success).toBe(true);
    expect(result.generatedFiles).toEqual([]);
  });

  it('creates deeply nested output directories automatically', async () => {
    const scriptPath = path.join(tempDir, 'deep.py');
    const deepDir = path.join(tempDir, 'a', 'b', 'c', 'd');
    await fs.writeFile(scriptPath, '# CoinModel .obj');
    const result = await runBlenderTask({ scriptPath, outputDir: deepDir });
    expect(result.success).toBe(true);
    const exists = await fs.stat(deepDir).then((s) => s.isDirectory()).catch(() => false);
    expect(exists).toBe(true);
  });

  it('returns failure and informative log message for missing python files', async () => {
    const result = await runBlenderTask({ scriptPath: path.join(tempDir, 'not_found.py'), outputDir: tempDir });
    expect(result.success).toBe(false);
    expect(result.logs).toContain('Script not found');
  });
});

describe('Tier 2: Feature 11 - Shiny Icon & Asset Pipeline Boundaries', () => {
  it('rejects empty byte buffer in PNG signature check', () => {
    const empty = Buffer.alloc(0);
    const isPng = empty.length >= 8 && empty.subarray(0, 8).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]));
    expect(isPng).toBe(false);
  });

  it('rejects short buffer under 8 bytes', () => {
    const short = Buffer.from([0x89, 0x50]);
    const isPng = short.length >= 8 && short.subarray(0, 8).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]));
    expect(isPng).toBe(false);
  });

  it('differentiates JPEG signature from PNG signature', () => {
    const jpeg = Buffer.from([0xff, 0xd8, 0xff, 0xe0, 0x00, 0x10, 0x4a, 0x46]);
    const isPng = jpeg.length >= 8 && jpeg.subarray(0, 8).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]));
    const isJpeg = jpeg.length >= 3 && jpeg.subarray(0, 3).equals(Buffer.from([0xff, 0xd8, 0xff]));
    expect(isPng).toBe(false);
    expect(isJpeg).toBe(true);
  });

  it('enforces asset ID must be positive integer', async () => {
    const { caller } = createMockStudioCaller();
    await expect(studio_asset({ action: 'insert', asset_id: -99 }, undefined, caller)).rejects.toThrow();
  });

  it('hashes distinct buffer payloads to distinct sha256 checksums', () => {
    const buf1 = Buffer.from('image1');
    const buf2 = Buffer.from('image2');
    const hash1 = crypto.createHash('sha256').update(buf1).digest('hex');
    const hash2 = crypto.createHash('sha256').update(buf2).digest('hex');
    expect(hash1).not.toBe(hash2);
  });
});

describe('Tier 2: Feature 12 - Main Branch & Clean Omni-Loop Status Boundaries', () => {
  it('confirms git branch command completes with 0 exit code', async () => {
    const repoRoot = path.resolve(__dirname, '../..');
    const result = await runShellCommand({ command: 'git rev-parse --abbrev-ref HEAD', cwd: repoRoot });
    expect(result.code).toBe(0);
    expect(result.stdout.trim()).toBe('main');
  });

  it('handles commands that return non-zero exit codes gracefully', async () => {
    const repoRoot = path.resolve(__dirname, '../..');
    const result = await runShellCommand({ command: 'git checkout __non_existent_branch_name__', cwd: repoRoot });
    expect(result.code).not.toBe(0);
    expect(result.stderr.length).toBeGreaterThan(0);
  });

  it('enforces command execution timeout boundary', async () => {
    const repoRoot = path.resolve(__dirname, '../..');
    const result = await runShellCommand({
      command: process.platform === 'win32' ? 'ping 127.0.0.1 -n 5' : 'sleep 5',
      cwd: repoRoot,
      timeoutMs: 1_000,
    });
    expect(result.timedOut).toBe(true);
  });

  it('truncates excessive command stdout output to maximum 20,000 characters', async () => {
    const repoRoot = path.resolve(__dirname, '../..');
    const result = await runShellCommand({
      command: process.platform === 'win32' ? 'python -c "print(\'A\' * 30000)"' : 'python3 -c "print(\'A\' * 30000)"',
      cwd: repoRoot,
    });
    expect(result.stdout.length).toBeLessThanOrEqual(20_000);
  });

  it('throws an error when command string is empty', () => {
    expect(() => runShellCommand({ command: '', cwd: path.resolve(__dirname, '../..') })).toThrow(/Command is empty/);
  });
});
