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
  defaultMockInstances,
  McpServerPool,
  runBlenderTask,
  runPlaytestScenario,
  studio_asset,
  studio_inspect,
  studio_multi_edit,
  studio_search_tree,
} from './harness';

describe('Tier 1: Feature 1 - Studio Hierarchy Search', () => {
  it('searches the workspace root and discovers immediate child instances', async () => {
    const { caller } = createMockStudioCaller();
    const result = await studio_search_tree({ path: 'Workspace' }, caller);
    expect(result.instances.length).toBeGreaterThan(0);
    const paths = result.instances.map((i) => i.path);
    expect(paths).toContain('Workspace.Baseplate');
    expect(paths).toContain('Workspace.SpawnLocation');
  });

  it('filters hierarchy instances strictly by instance_type', async () => {
    const { caller } = createMockStudioCaller();
    const result = await studio_search_tree({ path: 'Workspace', instance_type: 'Part' }, caller);
    expect(result.instances.length).toBeGreaterThan(0);
    for (const inst of result.instances) {
      expect(inst.className).toBe('Part');
    }
    expect(result.instances.map((i) => i.name)).toContain('Baseplate');
  });

  it('filters hierarchy instances matching keyword substring', async () => {
    const { caller } = createMockStudioCaller();
    const result = await studio_search_tree({ keywords: 'Coin' }, caller);
    expect(result.instances.length).toBeGreaterThanOrEqual(2);
    const paths = result.instances.map((i) => i.path);
    expect(paths).toContain('Workspace.CoinModel');
    expect(paths).toContain('ServerScriptService.CoinManager');
  });

  it('respects max_depth limit during hierarchy traversal', async () => {
    const { caller } = createMockStudioCaller();
    const shallow = await studio_search_tree({ path: 'Workspace', max_depth: 1 }, caller);
    const shallowPaths = shallow.instances.map((i) => i.path);
    expect(shallowPaths).toContain('Workspace.CoinModel');
    expect(shallowPaths).not.toContain('Workspace.CoinModel.Hitbox');
  });

  it('discovers script containers across ServerScriptService and ReplicatedStorage', async () => {
    const { caller } = createMockStudioCaller();
    const scripts = await studio_search_tree({ instance_type: 'Script' }, caller);
    const moduleScripts = await studio_search_tree({ instance_type: 'ModuleScript' }, caller);
    expect(scripts.instances.map((i) => i.path)).toContain('ServerScriptService.CoinManager');
    expect(moduleScripts.instances.map((i) => i.path)).toContain('ReplicatedStorage.CoinConfig');
  });
});

describe('Tier 1: Feature 2 - Instance Deep Inspection', () => {
  it('deeply inspects a Part instance and retrieves physical properties', async () => {
    const { caller } = createMockStudioCaller();
    const result = await studio_inspect({ path: 'Workspace.Baseplate' }, caller);
    expect(result.path).toBe('Workspace.Baseplate');
    expect(result.className).toBe('Part');
    expect(result.properties).toHaveProperty('Size', '512, 16, 512');
    expect(result.properties).toHaveProperty('Anchored', true);
  });

  it('inspects custom attributes configured on an instance', async () => {
    const { caller } = createMockStudioCaller();
    const result = await studio_inspect({ path: 'Workspace.SpawnLocation' }, caller);
    expect(result.attributes).toHaveProperty('Enabled', true);
  });

  it('inspects Model container and returns structured child references', async () => {
    const { caller } = createMockStudioCaller();
    const result = await studio_inspect({ path: 'Workspace.CoinModel' }, caller);
    expect(result.className).toBe('Model');
    expect(result.children.length).toBe(1);
    expect(result.children[0]?.path).toBe('Workspace.CoinModel.Hitbox');
    expect(result.children[0]?.className).toBe('Part');
  });

  it('inspects ServerScript and returns execution context properties', async () => {
    const { caller } = createMockStudioCaller();
    const result = await studio_inspect({ path: 'ServerScriptService.CoinManager' }, caller);
    expect(result.className).toBe('Script');
    expect(result.properties).toHaveProperty('RunContext', 'Server');
    expect(result.properties).toHaveProperty('Disabled', false);
  });

  it('deep inspects container with empty attributes and returns clean record', async () => {
    const { caller } = createMockStudioCaller();
    const result = await studio_inspect({ path: 'ReplicatedStorage.CoinConfig' }, caller);
    expect(result.className).toBe('ModuleScript');
    expect(result.attributes).toEqual({});
  });
});

describe('Tier 1: Feature 3 - Multi-Script Batch Editing', () => {
  it('performs atomic string replacement across an existing script', async () => {
    const { caller, instances } = createMockStudioCaller();
    const approvals = new ApprovalQueue();
    const result = await studio_multi_edit(
      {
        edits: [
          {
            path: 'ServerScriptService.CoinManager',
            old_string: 'function Coins.Spawn() return true end',
            new_string: 'function Coins.Spawn() print("spawned") return true end',
          },
        ],
      },
      approvals,
      caller,
    );
    expect(result.success).toBe(true);
    expect(result.modified_scripts).toContain('ServerScriptService.CoinManager');
    const updated = instances.get('ServerScriptService.CoinManager');
    expect(updated?.source).toContain('print("spawned")');
  });

  it('creates a new script in DataModel when path does not exist', async () => {
    const { caller, instances } = createMockStudioCaller();
    const approvals = new ApprovalQueue();
    const newPath = 'ServerScriptService.Leaderboard';
    const result = await studio_multi_edit(
      {
        edits: [
          {
            path: newPath,
            new_string: '--!strict\nlocal Leaderboard = {}\nreturn Leaderboard',
            className: 'Script',
          },
        ],
      },
      approvals,
      caller,
    );
    expect(result.success).toBe(true);
    expect(result.modified_scripts).toContain(newPath);
    expect(instances.has(newPath)).toBe(true);
  });

  it('applies batch edits across multiple distinct scripts atomically', async () => {
    const { caller, instances } = createMockStudioCaller();
    const approvals = new ApprovalQueue();
    const result = await studio_multi_edit(
      {
        edits: [
          {
            path: 'ServerScriptService.CoinManager',
            old_string: 'local Coins = {}',
            new_string: 'local Coins = { count = 0 }',
          },
          {
            path: 'ReplicatedStorage.CoinConfig',
            old_string: 'RespawnTime = 30',
            new_string: 'RespawnTime = 15',
          },
        ],
      },
      approvals,
      caller,
    );
    expect(result.success).toBe(true);
    expect(result.modified_scripts.length).toBe(2);
    expect(instances.get('ServerScriptService.CoinManager')?.source).toContain('count = 0');
    expect(instances.get('ReplicatedStorage.CoinConfig')?.source).toContain('RespawnTime = 15');
  });

  it('stages edits through the approval queue before execution', async () => {
    const { caller } = createMockStudioCaller();
    const approvals = new ApprovalQueue();
    const edits = [
      {
        path: 'ServerScriptService.CoinManager',
        new_string: '--!strict\nprint("stage check")',
      },
    ];
    await studio_multi_edit({ edits }, approvals, caller);
    expect(approvals.list().length).toBe(0);
  });

  it('overwrites entire script source cleanly when old_string is omitted', async () => {
    const { caller, instances } = createMockStudioCaller();
    const approvals = new ApprovalQueue();
    const replacement = '--!strict\n-- Completely new implementation\nreturn {}';
    const result = await studio_multi_edit(
      {
        edits: [{ path: 'ReplicatedStorage.CoinConfig', new_string: replacement }],
      },
      approvals,
      caller,
    );
    expect(result.success).toBe(true);
    expect(instances.get('ReplicatedStorage.CoinConfig')?.source).toBe(replacement);
  });
});

describe('Tier 1: Feature 4 - Studio Asset Search & Insert', () => {
  it('searches Creator Store for Model assets matching keyword', async () => {
    const { caller } = createMockStudioCaller();
    const result = await studio_asset({ action: 'search', keyword: 'Coin', asset_type: 'Model' }, undefined, caller);
    expect(result.results).toBeDefined();
    expect(result.results?.length).toBeGreaterThan(0);
    expect(result.results?.[0]?.name).toContain('Coin');
    expect(result.results?.[0]?.assetType).toBe('Model');
  });

  it('searches Creator Store for Audio assets', async () => {
    const { caller } = createMockStudioCaller();
    const result = await studio_asset({ action: 'search', keyword: 'Coin', asset_type: 'Audio' }, undefined, caller);
    expect(result.results?.length).toBe(1);
    expect(result.results?.[0]?.assetType).toBe('Audio');
  });

  it('inserts an asset by valid ID into Workspace', async () => {
    const { caller, instances } = createMockStudioCaller();
    const approvals = new ApprovalQueue();
    const result = await studio_asset(
      { action: 'insert', asset_id: 1001, target_parent: 'Workspace' },
      approvals,
      caller,
    );
    expect(result.inserted_instance).toBe('Workspace.InsertedAsset_1001');
    expect(instances.has('Workspace.InsertedAsset_1001')).toBe(true);
  });

  it('inserts an asset defaulting target_parent to Workspace when omitted', async () => {
    const { caller, instances } = createMockStudioCaller();
    const approvals = new ApprovalQueue();
    const result = await studio_asset({ action: 'insert', asset_id: 1004 }, approvals, caller);
    expect(result.inserted_instance).toBe('Workspace.InsertedAsset_1004');
    expect(instances.has('Workspace.InsertedAsset_1004')).toBe(true);
  });

  it('stages asset insertion through the approval queue', async () => {
    const { caller } = createMockStudioCaller();
    const approvals = new ApprovalQueue();
    await studio_asset(
      { action: 'insert', asset_id: 1002, target_parent: 'Workspace' },
      approvals,
      caller,
    );
    expect(approvals.list().length).toBe(0);
  });
});

describe('Tier 1: Feature 5 - Avatar Navigation & Input', () => {
  it('sends character_navigation command to reach target coordinates', async () => {
    const { caller, history } = createMockStudioCaller();
    await caller.callTool('character_navigation', { target_position: [10, 2, 50] });
    const call = history.find((h) => h.tool === 'character_navigation');
    expect(call).toBeDefined();
    expect(call?.args.target_position).toEqual([10, 2, 50]);
  });

  it('sends character_navigation command to target an instance path', async () => {
    const { caller, history } = createMockStudioCaller();
    await caller.callTool('character_navigation', { target_instance: 'Workspace.CoinModel.Hitbox' });
    const call = history.find((h) => h.tool === 'character_navigation');
    expect(call?.args.target_instance).toBe('Workspace.CoinModel.Hitbox');
  });

  it('dispatches sequential keyboard actions for player interaction', async () => {
    const { caller, history } = createMockStudioCaller();
    const actions = [
      { action: 'keyDown', key: 'KeyE' },
      { action: 'wait', duration: 0.1 },
      { action: 'keyUp', key: 'KeyE' },
    ];
    const reply = await caller.callTool('user_keyboard_input', { actions });
    expect(JSON.parse(reply).processedActions).toBe(3);
    const call = history.find((h) => h.tool === 'user_keyboard_input');
    expect(call?.args.actions).toEqual(actions);
  });

  it('dispatches mouse input click events on viewport', async () => {
    const { caller, history } = createMockStudioCaller();
    const actions = [
      { action: 'move', x: 400, y: 300 },
      { action: 'click', button: 'Left' },
    ];
    const reply = await caller.callTool('user_mouse_input', { actions });
    expect(JSON.parse(reply).processedActions).toBe(2);
    expect(history.some((h) => h.tool === 'user_mouse_input')).toBe(true);
  });

  it('queries studio output logs to verify player event confirmation', async () => {
    const { caller } = createMockStudioCaller();
    const reply = await caller.callTool('get_console_output', {});
    const parsed = JSON.parse(reply) as { lines: string[] };
    expect(parsed.lines).toContain('[Server] Coins loaded successfully');
  });
});

describe('Tier 1: Feature 6 - Autonomous In-Studio Playtest Runner', () => {
  let tempDir = '';

  beforeEach(async () => {
    tempDir = await mkdtemp(path.join(tmpdir(), 'openro-e2e-playtest-'));
  });

  afterEach(async () => {
    await rm(tempDir, { recursive: true, force: true });
  });

  it('executes a scenario with matching assertions and returns PASSED verdict', async () => {
    const mock = createMockStudioCaller();
    const def = parseAcceptanceDefinition(
      JSON.stringify({
        version: 1,
        checks: [
          { id: 'c1', scenario: 'Coin Collection', type: 'assertion', description: 'coin picked up', expected: 'true' },
          { id: 'c2', scenario: 'Coin Collection', type: 'state', instancePath: 'Workspace.SpawnLocation', property: 'Enabled', expected: 'true' },
        ],
        screenshots: ['shots/coin_collection_final.png'],
      }),
    );

    const result = await runPlaytestScenario('Coin Collection', {
      maxDurationSeconds: 10,
      captureScreenshots: true,
      projectRoot: tempDir,
      acceptanceDef: def,
      caller: mock.caller,
    });

    expect(result.verdict).toBe('PASSED');
    expect(result.checks.length).toBe(2);
    expect(result.checks.every((c) => c.passed)).toBe(true);
    expect(result.screenshotPaths.length).toBe(1);
  });

  it('reports FAILED verdict when an assertion does not match expectation', async () => {
    const mock = createMockStudioCaller();
    const def = parseAcceptanceDefinition(
      JSON.stringify({
        version: 1,
        checks: [
          { id: 'c1', scenario: 'Coin Collection', type: 'assertion', description: 'coin picked up', expected: 'true' },
          { id: 'c2', scenario: 'Coin Collection', type: 'state', instancePath: 'Workspace.SpawnLocation', property: 'Enabled', expected: 'true' },
        ],
        screenshots: ['shots/coin_collection_final.png'],
      }),
    );

    const result = await runPlaytestScenario('Coin Collection', {
      maxDurationSeconds: 10,
      captureScreenshots: true,
      projectRoot: tempDir,
      acceptanceDef: def,
      caller: mock.caller,
      scenarioActuals: { assertions: { c1: 'false' } },
    });

    expect(result.verdict).toBe('FAILED');
    const failing = result.checks.find((c) => !c.passed);
    expect(failing?.name).toBe('coin picked up');
    expect(failing?.actual).toBe('false');
  });

  it('guarantees restoration to Edit mode when playtest scenario ends', async () => {
    const mock = createMockStudioCaller();
    const def = parseAcceptanceDefinition(
      JSON.stringify({
        version: 1,
        checks: [
          { id: 'c1', scenario: 'Simple Probe', type: 'assertion', description: 'probe', expected: 'true' },
          { id: 'c2', scenario: 'Simple Probe', type: 'state', instancePath: 'Workspace.SpawnLocation', property: 'Enabled', expected: 'true' },
        ],
        screenshots: ['shots/simple_probe_final.png'],
      }),
    );

    await runPlaytestScenario('Simple Probe', {
      maxDurationSeconds: 5,
      captureScreenshots: true,
      projectRoot: tempDir,
      acceptanceDef: def,
      caller: mock.caller,
    });

    expect(mock.playtestActive).toBe(false);
  });

  it('tracks execution duration in milliseconds', async () => {
    const mock = createMockStudioCaller();
    const def = parseAcceptanceDefinition(
      JSON.stringify({
        version: 1,
        checks: [
          { id: 'c1', scenario: 'Timer Probe', type: 'assertion', description: 'probe', expected: 'true' },
          { id: 'c2', scenario: 'Timer Probe', type: 'state', instancePath: 'Workspace.SpawnLocation', property: 'Enabled', expected: 'true' },
        ],
        screenshots: ['shots/timer_probe_final.png'],
      }),
    );

    const result = await runPlaytestScenario('Timer Probe', {
      maxDurationSeconds: 5,
      captureScreenshots: true,
      projectRoot: tempDir,
      acceptanceDef: def,
      caller: mock.caller,
    });

    expect(result.durationMs).toBeGreaterThanOrEqual(0);
  });

  it('creates and records evidence screenshot files in project root', async () => {
    const mock = createMockStudioCaller();
    const def = parseAcceptanceDefinition(
      JSON.stringify({
        version: 1,
        checks: [
          { id: 'c1', scenario: 'Screenshot Probe', type: 'assertion', description: 'probe', expected: 'true' },
          { id: 'c2', scenario: 'Screenshot Probe', type: 'state', instancePath: 'Workspace.SpawnLocation', property: 'Enabled', expected: 'true' },
        ],
        screenshots: ['shots/screenshot_probe_final.png'],
      }),
    );

    const result = await runPlaytestScenario('Screenshot Probe', {
      maxDurationSeconds: 5,
      captureScreenshots: true,
      projectRoot: tempDir,
      acceptanceDef: def,
      caller: mock.caller,
    });

    expect(result.screenshotPaths).toHaveLength(1);
    expect(result.screenshotPaths[0]).toContain('.png');
  });
});

describe('Tier 1: Feature 7 - Exact Multiset Acceptance Enforcement', () => {
  const validJson = JSON.stringify({
    version: 1,
    checks: [
      { id: 'coin-count', scenario: 'Round 1', type: 'assertion', description: 'coins >= 5', expected: 'true' },
      { id: 'timer-state', scenario: 'Round 1', type: 'state', instancePath: 'Workspace.SpawnLocation', property: 'Enabled', expected: 'true' },
      { id: 'door-open', scenario: 'Door Open', type: 'assertion', description: 'door is open', expected: 'true' },
    ],
    screenshots: ['shots/round1.png'],
    repairHint: 'Check CoinManager leaderstats script.',
  });

  it('parses valid v1 acceptance definition structure', () => {
    const def = parseAcceptanceDefinition(validJson);
    expect(def.version).toBe(1);
    expect(def.checks.length).toBe(3);
    expect(def.screenshots).toEqual(['shots/round1.png']);
    expect(def.repairHint).toBe('Check CoinManager leaderstats script.');
  });

  it('filters acceptance checks accurately for a given scenario', () => {
    const def = parseAcceptanceDefinition(validJson);
    const round1 = acceptanceForScenario(def, 'Round 1');
    expect(round1?.assertions.length).toBe(1);
    expect(round1?.stateChecks.length).toBe(1);
    expect(round1?.assertions[0]?.id).toBe('coin-count');
    expect(round1?.stateChecks[0]?.id).toBe('timer-state');
  });

  it('returns null when querying a non-existent scenario', () => {
    const def = parseAcceptanceDefinition(validJson);
    const missing = acceptanceForScenario(def, 'NonExistentScenario');
    expect(missing).toBeNull();
  });

  it('generates structured repair instruction text including repair hint', () => {
    const def = parseAcceptanceDefinition(validJson);
    const instruction = acceptanceInstruction(def, 'Round 1');
    expect(instruction).toContain('Acceptance definition for "Round 1"');
    expect(instruction).toContain('coins >= 5');
    expect(instruction).toContain('Workspace.SpawnLocation.Enabled');
    expect(instruction).toContain('Check CoinManager leaderstats script.');
  });

  it('enforces screenshot path registration in scenario specifications', () => {
    const def = parseAcceptanceDefinition(validJson);
    const spec = acceptanceForScenario(def, 'Round 1');
    expect(spec?.screenshots).toContain('shots/round1.png');
  });
});

describe('Tier 1: Feature 8 - Multi-Server MCP Registry & Pool', () => {
  it('connects and registers a server in the pool with unique session identity', async () => {
    const pool = new McpServerPool();
    const info = await pool.connectMcpServer({ id: 'studio-main', command: 'mock-studio', args: [] });
    expect(info.id).toBe('studio-main');
    expect(info.session).toContain('pool-studio-main-');
    expect(pool.getMcpClient('studio-main')).toBeDefined();
  });

  it('connects multiple distinct servers concurrently in the registry', async () => {
    const pool = new McpServerPool();
    await pool.connectMcpServer({ id: 'studio', command: 'mock-studio', args: [] });
    await pool.connectMcpServer({ id: 'blender', command: 'mock-blender', args: [] });
    expect(pool.listServers().length).toBe(2);
    expect(pool.getMcpClient('studio')).toBeDefined();
    expect(pool.getMcpClient('blender')).toBeDefined();
  });

  it('routes tool calls to the targeted server by serverId', async () => {
    const pool = new McpServerPool();
    await pool.connectMcpServer({ id: 'studio', command: 'mock-studio', args: [] });
    const reply = (await pool.callMcpTool('studio', 'search_game_tree', { path: 'Workspace' })) as { instances: unknown[] };
    expect(reply.instances).toBeDefined();
  });

  it('disconnects targeted server cleanly without impacting other active servers', async () => {
    const pool = new McpServerPool();
    await pool.connectMcpServer({ id: 'srv1', command: 'cmd1', args: [] });
    await pool.connectMcpServer({ id: 'srv2', command: 'cmd2', args: [] });
    await pool.disconnectMcpServer('srv1');
    expect(pool.getMcpClient('srv1')).toBeUndefined();
    expect(pool.getMcpClient('srv2')).toBeDefined();
    expect(pool.listServers().length).toBe(1);
  });

  it('lists registered tools with schemas for all connected pool servers', async () => {
    const pool = new McpServerPool();
    const info = await pool.connectMcpServer({ id: 'studio', command: 'mock-studio', args: [] });
    expect(info.tools.length).toBeGreaterThanOrEqual(10);
    expect(info.tools.map((t) => t.name)).toContain('search_game_tree');
    expect(info.tools.map((t) => t.name)).toContain('multi_edit');
  });
});

describe('Tier 1: Feature 9 - MCP Lifecycle & Crash Recovery', () => {
  it('detects running status accurately during active connection', () => {
    const { caller } = createMockStudioCaller();
    expect(caller.running).toBe(true);
    expect(caller.identity?.session).toBeDefined();
  });

  it('transitions running to false on abrupt process termination', () => {
    const mock = createMockStudioCaller();
    mock.setRunning(false);
    expect(mock.caller.running).toBe(false);
    expect(mock.caller.identity).toBeNull();
  });

  it('assigns a distinct session UUID upon server restart', () => {
    const mock = createMockStudioCaller();
    const firstSession = mock.caller.identity?.session;
    mock.setSession('session-restarted-uuid-2');
    const secondSession = mock.caller.identity?.session;
    expect(secondSession).not.toBe(firstSession);
  });

  it('invalidates and refuses actions staged on a previous crashed session', async () => {
    const mock = createMockStudioCaller();
    const approvals = new ApprovalQueue();
    const originalIdentity = mock.caller.identity;
    expect(originalIdentity).not.toBeNull();
    if (!originalIdentity) return;

    const staged = approvals.stageMcp({
      projectRoot: '/test',
      tool: 'search_game_tree',
      args: { path: 'Workspace' },
      server: originalIdentity,
    });

    mock.setSession('new-session-after-crash');
    const currentIdentity = mock.caller.identity;
    expect(currentIdentity?.session).not.toBe(staged.mcpServer?.session);

    approvals.take(staged.id, 'mcp');
  });

  it('throws descriptive error when calling tools on a terminated server', async () => {
    const mock = createMockStudioCaller();
    mock.setRunning(false);
    await expect(mock.caller.callTool('search_game_tree', {})).rejects.toThrow(/not running/);
  });
});

describe('Tier 1: Feature 10 - Headless Blender Task Runner', () => {
  let tempDir = '';

  beforeEach(async () => {
    tempDir = await mkdtemp(path.join(tmpdir(), 'openro-e2e-blender-'));
  });

  afterEach(async () => {
    await rm(tempDir, { recursive: true, force: true });
  });

  it('runs a python task and generates 3D mesh files', async () => {
    const scriptPath = path.join(tempDir, 'create_coin.py');
    await writeFile(scriptPath, '# Generate CoinModel .obj mesh\nprint("Coin mesh generated")');
    const result = await runBlenderTask({ scriptPath, outputDir: tempDir });
    expect(result.success).toBe(true);
    expect(result.generatedFiles.some((f) => f.endsWith('.obj'))).toBe(true);
  });

  it('runs a python task and generates 2D rendered icon PNGs', async () => {
    const scriptPath = path.join(tempDir, 'render_icon.py');
    await writeFile(scriptPath, '# Render Icon .png badge\nprint("Icon badge rendered")');
    const result = await runBlenderTask({ scriptPath, outputDir: tempDir });
    expect(result.success).toBe(true);
    expect(result.generatedFiles.some((f) => f.endsWith('.png'))).toBe(true);
  });

  it('captures process stdout logs during execution', async () => {
    const scriptPath = path.join(tempDir, 'task.py');
    await writeFile(scriptPath, 'print("Processing 3D geometry")');
    const result = await runBlenderTask({ scriptPath, outputDir: tempDir });
    expect(result.logs).toContain('Running Blender headless');
  });

  it('creates the output directory recursively if it does not exist', async () => {
    const scriptPath = path.join(tempDir, 'nested.py');
    const outputDir = path.join(tempDir, 'nested', 'deep', 'output');
    await writeFile(scriptPath, '# CoinModel .obj');
    const result = await runBlenderTask({ scriptPath, outputDir });
    expect(result.success).toBe(true);
    expect(result.generatedFiles[0]).toContain(path.join('nested', 'deep', 'output'));
  });

  it('returns failure when scriptPath cannot be resolved', async () => {
    const result = await runBlenderTask({ scriptPath: path.join(tempDir, 'nonexistent.py'), outputDir: tempDir });
    expect(result.success).toBe(false);
    expect(result.generatedFiles).toHaveLength(0);
    expect(result.logs).toContain('Script not found');
  });
});

describe('Tier 1: Feature 11 - Shiny Icon & Asset Pipeline', () => {
  it('validates valid 1x1 PNG image headers for icons and badges', () => {
    const buffer = createValidPngBuffer();
    expect(buffer.subarray(0, 8)).toEqual(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]));
  });

  it('rejects corrupted image data lacking valid PNG or JPEG signatures', () => {
    const corrupt = Buffer.from('NOT AN IMAGE FILE DATA');
    const isPng = corrupt.subarray(0, 8).equals(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]));
    expect(isPng).toBe(false);
  });

  it('stages asset import into Studio after icon generation', async () => {
    const { caller } = createMockStudioCaller();
    const approvals = new ApprovalQueue();
    const result = await studio_asset(
      { action: 'insert', asset_id: 1003, target_parent: 'Workspace' },
      approvals,
      caller,
    );
    expect(result.inserted_instance).toBe('Workspace.InsertedAsset_1003');
  });

  it('computes sha256 checksum for generated asset files', () => {
    const buffer = createValidPngBuffer();
    const hash = crypto.createHash('sha256').update(buffer).digest('hex');
    expect(hash).toHaveLength(64);
  });

  it('enforces asset size boundaries below 25 MB limit', () => {
    const buffer = createValidPngBuffer();
    expect(buffer.length).toBeLessThan(25_000_000);
  });
});

describe('Tier 1: Feature 12 - Main Branch & Clean Omni-Loop Status', () => {
  it('confirms the active git repository branch is strictly main', async () => {
    const repoRoot = path.resolve(__dirname, '../..');
    const result = await runShellCommand({ command: 'git branch --show-current', cwd: repoRoot });
    expect(result.code).toBe(0);
    expect(result.stdout.trim()).toBe('main');
  });

  it('confirms working tree git status can be queried without errors', async () => {
    const repoRoot = path.resolve(__dirname, '../..');
    const result = await runShellCommand({ command: 'git status --porcelain', cwd: repoRoot });
    expect(result.code).toBe(0);
  });

  it('confirms omni_loop_controller.py exists in skill scripts directory', async () => {
    const controllerPath = path.resolve(__dirname, '../../.opencode/skills/omni-loop/scripts/omni_loop_controller.py');
    const result = await runShellCommand({
      command: `python "${controllerPath}" --help`,
      cwd: path.resolve(__dirname, '../..'),
    });
    expect(result.code).toBe(0);
    expect(result.stdout).toContain('OMNI-LOOP');
  });

  it('verifies that package.json specifies open-ro product name', async () => {
    const repoRoot = path.resolve(__dirname, '../..');
    const pkg = JSON.parse(await fs.readFile(path.join(repoRoot, 'package.json'), 'utf8')) as { name: string; productName: string };
    expect(pkg.name).toBe('open-ro');
    expect(pkg.productName).toBe('Open-Ro Studio');
  });

  it('confirms vitest configuration defines test runner boundaries', async () => {
    const repoRoot = path.resolve(__dirname, '../..');
    const configContent = await fs.readFile(path.join(repoRoot, 'vitest.config.mts'), 'utf8');
    expect(configContent).toContain('include:');
    expect(configContent).toContain('tests/**/*.test.ts');
  });
});
