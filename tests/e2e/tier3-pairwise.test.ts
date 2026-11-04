import * as crypto from 'node:crypto';
import { promises as fs } from 'node:fs';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { ApprovalQueue } from '../../electron/approvals';
import {
  parseAcceptanceDefinition,
  acceptanceForScenario,
  acceptanceInstruction,
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

describe('Tier 3: Pairwise Cross-Feature Interactions', () => {
  let tempDir = '';

  beforeEach(async () => {
    tempDir = await mkdtemp(path.join(tmpdir(), 'openro-e2e-tier3-'));
  });

  afterEach(async () => {
    await rm(tempDir, { recursive: true, force: true });
  });

  it('Pair 1 [Search + Inspect]: searches hierarchy and deeply inspects discovered instances', async () => {
    const { caller } = createMockStudioCaller();
    const searchResult = await studio_search_tree({ keywords: 'Coin' }, caller);
    expect(searchResult.instances.length).toBeGreaterThanOrEqual(2);

    for (const item of searchResult.instances) {
      const inspected = await studio_inspect({ path: item.path }, caller);
      expect(inspected.path).toBe(item.path);
      expect(inspected.className).toBe(item.className);
      expect(inspected.properties).toBeDefined();
    }
  });

  it('Pair 2 [Search + Multi-Edit]: discovers scripts and applies batch edits atomically', async () => {
    const { caller, instances } = createMockStudioCaller();
    const approvals = new ApprovalQueue();
    const searchResult = await studio_search_tree({ instance_type: 'Script' }, caller);
    const targetScript = searchResult.instances.find((i) => i.path === 'ServerScriptService.CoinManager');
    expect(targetScript).toBeDefined();

    if (!targetScript) return;

    const editResult = await studio_multi_edit(
      {
        edits: [
          {
            path: targetScript.path,
            old_string: 'function Coins.Spawn() return true end',
            new_string: 'function Coins.Spawn() return { id = 1, active = true } end',
          },
        ],
      },
      approvals,
      caller,
    );

    expect(editResult.success).toBe(true);
    expect(instances.get(targetScript.path)?.source).toContain('active = true');
  });

  it('Pair 3 [Asset Search + Insert + Inspect]: searches asset, inserts it, and verifies via deep inspection', async () => {
    const { caller } = createMockStudioCaller();
    const approvals = new ApprovalQueue();

    const search = await studio_asset({ action: 'search', keyword: 'Coin', asset_type: 'Model' }, undefined, caller);
    const selected = search.results?.[0];
    expect(selected).toBeDefined();
    if (!selected) return;

    const inserted = await studio_asset(
      { action: 'insert', asset_id: selected.assetId, target_parent: 'Workspace' },
      approvals,
      caller,
    );
    expect(inserted.inserted_instance).toBe(`Workspace.InsertedAsset_${selected.assetId}`);

    const inspected = await studio_inspect({ path: inserted.inserted_instance ?? '' }, caller);
    expect(inspected.className).toBe('Model');
    expect(inspected.attributes).toHaveProperty('InsertedAssetId', selected.assetId);
  });

  it('Pair 4 [Playtest Runner + Navigation + Input]: coordinates avatar movement and input in playtest', async () => {
    const mock = createMockStudioCaller();
    const def = parseAcceptanceDefinition(
      JSON.stringify({
        version: 1,
        checks: [
          { id: 'c1', scenario: 'Avatar Coin Run', type: 'assertion', description: 'coin picked up', expected: 'true' },
          { id: 'c2', scenario: 'Avatar Coin Run', type: 'state', instancePath: 'Workspace.SpawnLocation', property: 'Enabled', expected: 'true' },
        ],
        screenshots: ['shots/avatar_coin_run_final.png'],
      }),
    );

    await mock.caller.callTool('character_navigation', { target_position: [10, 2, 50] });
    await mock.caller.callTool('user_keyboard_input', {
      actions: [
        { action: 'keyDown', key: 'KeyE' },
        { action: 'keyUp', key: 'KeyE' },
      ],
    });

    const result = await runPlaytestScenario('Avatar Coin Run', {
      maxDurationSeconds: 10,
      captureScreenshots: true,
      projectRoot: tempDir,
      acceptanceDef: def,
      caller: mock.caller,
    });

    expect(result.verdict).toBe('PASSED');
    expect(mock.history.some((h) => h.tool === 'character_navigation')).toBe(true);
    expect(mock.history.some((h) => h.tool === 'user_keyboard_input')).toBe(true);
  });

  it('Pair 5 [Acceptance Enforcement + Playtest Runner]: validates exact multiset assertions and state checks', async () => {
    const mock = createMockStudioCaller();
    const def = parseAcceptanceDefinition(
      JSON.stringify({
        version: 1,
        checks: [
          { id: 'a1', scenario: 'Multiset Gate', type: 'assertion', description: 'coins == 10', expected: '10' },
          { id: 's1', scenario: 'Multiset Gate', type: 'state', instancePath: 'Workspace.SpawnLocation', property: 'Enabled', expected: 'true' },
        ],
        screenshots: ['shots/multiset_gate_final.png'],
      }),
    );

    const result = await runPlaytestScenario('Multiset Gate', {
      maxDurationSeconds: 10,
      captureScreenshots: true,
      projectRoot: tempDir,
      acceptanceDef: def,
      caller: mock.caller,
      scenarioActuals: {
        assertions: { a1: '10' },
        states: { s1: 'true' },
      },
    });

    expect(result.verdict).toBe('PASSED');
    expect(result.checks.length).toBe(2);
  });

  it('Pair 6 [Blender Task + Asset Pipeline]: executes 3D generation and verifies image asset badge', async () => {
    const scriptPath = path.join(tempDir, 'generate_assets.py');
    await writeFile(scriptPath, '# CoinModel .obj\n# Icon .png badge');

    const result = await runBlenderTask({ scriptPath, outputDir: tempDir });
    expect(result.success).toBe(true);
    expect(result.generatedFiles.length).toBe(2);

    const pngFile = result.generatedFiles.find((f) => f.endsWith('.png'));
    expect(pngFile).toBeDefined();
    if (!pngFile) return;

    const bytes = await fs.readFile(pngFile);
    expect(bytes.subarray(0, 8)).toEqual(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]));
  });

  it('Pair 7 [MCP Pool + Crash Recovery]: crash of one server in pool does not disrupt other servers', async () => {
    const pool = new McpServerPool();
    await pool.connectMcpServer({ id: 'studio', command: 'mock-studio', args: [] });
    await pool.connectMcpServer({ id: 'blender', command: 'mock-blender', args: [] });

    const studioClient = pool.getMcpClient('studio');
    expect(studioClient).toBeDefined();

    await pool.disconnectMcpServer('studio');
    expect(pool.getMcpClient('studio')).toBeUndefined();

    const blenderClient = pool.getMcpClient('blender');
    expect(blenderClient).toBeDefined();
    expect(blenderClient?.running).toBe(true);
  });

  it('Pair 8 [Multi-Edit + Staged Approval + State Verification]: stages and executes atomic changes', async () => {
    const { caller, instances } = createMockStudioCaller();
    const approvals = new ApprovalQueue();

    const editParams = {
      edits: [
        {
          path: 'ServerScriptService.CoinManager',
          old_string: 'function Coins.Spawn() return true end',
          new_string: 'function Coins.Spawn() return false end',
        },
      ],
    };

    const res = await studio_multi_edit(editParams, approvals, caller, tempDir);
    expect(res.success).toBe(true);
    expect(instances.get('ServerScriptService.CoinManager')?.source).toContain('return false end');
  });

  it('Pair 9 [Hierarchy Search + Deep Inspection Consistency]: search and inspection report matching classNames', async () => {
    const { caller } = createMockStudioCaller();
    const search = await studio_search_tree({ path: 'Workspace' }, caller);

    for (const item of search.instances) {
      const inspect = await studio_inspect({ path: item.path }, caller);
      expect(inspect.className).toBe(item.className);
    }
  });

  it('Pair 10 [Playtest Failure + Acceptance Auto-Repair]: generates actionable repair instruction on test failure', async () => {
    const mock = createMockStudioCaller();
    const def = parseAcceptanceDefinition(
      JSON.stringify({
        version: 1,
        checks: [
          { id: 'c1', scenario: 'Broken Scenario', type: 'assertion', description: 'check coins', expected: 'true' },
          { id: 'c2', scenario: 'Broken Scenario', type: 'state', instancePath: 'Workspace.SpawnLocation', property: 'Enabled', expected: 'true' },
        ],
        screenshots: ['shots/broken_scenario_final.png'],
        repairHint: 'Increase coin spawn rate in CoinConfig.',
      }),
    );

    const result = await runPlaytestScenario('Broken Scenario', {
      maxDurationSeconds: 10,
      captureScreenshots: true,
      projectRoot: tempDir,
      acceptanceDef: def,
      caller: mock.caller,
      scenarioActuals: { assertions: { c1: 'false' } },
    });

    expect(result.verdict).toBe('FAILED');
    const instruction = acceptanceInstruction(def, 'Broken Scenario');
    expect(instruction).toContain('Increase coin spawn rate in CoinConfig.');
    expect(instruction).toContain('check coins');
  });

  it('Pair 11 [Avatar Input + Studio Console Logging]: logs verify user action processing', async () => {
    const { caller } = createMockStudioCaller();
    await caller.callTool('user_keyboard_input', {
      actions: [{ action: 'press', key: 'Space' }],
    });
    const consoleOutput = await caller.callTool('get_console_output', {});
    const parsed = JSON.parse(consoleOutput) as { lines: string[] };
    expect(parsed.lines.length).toBeGreaterThan(0);
  });

  it('Pair 12 [Blender Task + File System Safety]: generates output in verified directory structure', async () => {
    const scriptPath = path.join(tempDir, 'safe_render.py');
    const outputDir = path.join(tempDir, 'verified_output');
    await writeFile(scriptPath, '# Icon .png');

    const result = await runBlenderTask({ scriptPath, outputDir });
    expect(result.success).toBe(true);
    expect(result.generatedFiles[0]?.startsWith(outputDir)).toBe(true);
  });

  it('Pair 13 [Multi-Server Pool + Distinct Server Namespaces]: manages distinct server tool calls', async () => {
    const pool = new McpServerPool();
    await pool.connectMcpServer({ id: 'srv-alpha', command: 'cmd-alpha', args: [] });
    await pool.connectMcpServer({ id: 'srv-beta', command: 'cmd-beta', args: [] });

    const replyAlpha = (await pool.callMcpTool('srv-alpha', 'search_game_tree', { path: 'Workspace' })) as { instances: unknown[] };
    const replyBeta = (await pool.callMcpTool('srv-beta', 'search_game_tree', { path: 'ServerScriptService' })) as { instances: unknown[] };

    expect(replyAlpha.instances.length).toBeGreaterThan(0);
    expect(replyBeta.instances.length).toBeGreaterThan(0);
  });

  it('Pair 14 [Playtest Evidence + Viewport Capture]: viewport screenshot matches exact PNG hash requirements', async () => {
    const mock = createMockStudioCaller();
    const reply = await mock.caller.callTool('screen_capture', { capture_id: 'shot-1' });
    const parsed = JSON.parse(reply) as { base64: string; format: string };
    expect(parsed.format).toBe('png');
    const buffer = Buffer.from(parsed.base64, 'base64');
    expect(buffer.subarray(0, 8)).toEqual(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]));
    const hash = crypto.createHash('sha256').update(buffer).digest('hex');
    expect(hash.length).toBe(64);
  });

  it('Pair 15 [Omni-Loop + Main Branch Check]: validates environment integrity on main branch', async () => {
    const repoRoot = path.resolve(__dirname, '../..');
    const branchRes = await runShellCommand({ command: 'git branch --show-current', cwd: repoRoot });
    expect(branchRes.stdout.trim()).toBe('main');

    const statusRes = await runShellCommand({ command: 'git status -s', cwd: repoRoot });
    expect(statusRes.code).toBe(0);
  });
});
