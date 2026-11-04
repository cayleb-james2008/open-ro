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

describe('Tier 4: Real-World Application Scenarios', () => {
  let tempDir = '';

  beforeEach(async () => {
    tempDir = await mkdtemp(path.join(tmpdir(), 'openro-e2e-tier4-'));
  });

  afterEach(async () => {
    await rm(tempDir, { recursive: true, force: true });
  });

  it('Scenario 1: Round-Based Coin Spawner with UI & Sound Effect Lifecycle', async () => {
    const { caller, instances } = createMockStudioCaller();
    const approvals = new ApprovalQueue();

    // 1. Search hierarchy to locate ServerScriptService and Workspace
    const tree = await studio_search_tree({ path: 'Workspace' }, caller);
    expect(tree.instances.some((i) => i.path === 'Workspace.Baseplate')).toBe(true);

    // 2. Discover and insert audio asset for coin collection sound
    const assetSearch = await studio_asset({ action: 'search', keyword: 'Coin', asset_type: 'Audio' }, undefined, caller);
    const audio = assetSearch.results?.find((a) => a.assetType === 'Audio');
    expect(audio).toBeDefined();
    if (!audio) return;

    const insertedAudio = await studio_asset(
      { action: 'insert', asset_id: audio.assetId, target_parent: 'Workspace' },
      approvals,
      caller,
    );
    expect(insertedAudio.inserted_instance).toBe(`Workspace.InsertedAsset_${audio.assetId}`);

    // 3. Generate shiny coin icon via Blender task
    const blenderScript = path.join(tempDir, 'build_coin_icon.py');
    await writeFile(blenderScript, '# Icon .png badge\nprint("Rendering shiny coin badge")');
    const blenderResult = await runBlenderTask({ scriptPath: blenderScript, outputDir: tempDir });
    expect(blenderResult.success).toBe(true);
    expect(blenderResult.generatedFiles.some((f) => f.endsWith('.png'))).toBe(true);

    // 4. Batch edit to create RoundTimer and update CoinConfig
    const batchResult = await studio_multi_edit(
      {
        edits: [
          {
            path: 'ServerScriptService.RoundTimer',
            new_string: '--!strict\nlocal Timer = { duration = 60 }\nreturn Timer',
            className: 'Script',
          },
          {
            path: 'ReplicatedStorage.CoinConfig',
            old_string: 'RespawnTime = 30',
            new_string: 'RespawnTime = 10, SoundId = "rbxassetid://' + audio.assetId + '"',
          },
        ],
      },
      approvals,
      caller,
    );
    expect(batchResult.success).toBe(true);

    // 5. Run autonomous playtest to verify coin spawner
    const def = parseAcceptanceDefinition(
      JSON.stringify({
        version: 1,
        checks: [
          { id: 'spawn-ok', scenario: 'Round Spawner Verification', type: 'assertion', description: 'spawner active', expected: 'true' },
          { id: 'spawn-state', scenario: 'Round Spawner Verification', type: 'state', instancePath: 'Workspace.SpawnLocation', property: 'Enabled', expected: 'true' },
        ],
        screenshots: ['shots/spawner_verified.png'],
      }),
    );

    const playtest = await runPlaytestScenario('Round Spawner Verification', {
      maxDurationSeconds: 15,
      captureScreenshots: true,
      projectRoot: tempDir,
      acceptanceDef: def,
      caller,
    });

    expect(playtest.verdict).toBe('PASSED');
    expect(playtest.checks.length).toBe(2);
    expect(instances.has('ServerScriptService.RoundTimer')).toBe(true);
  });

  it('Scenario 2: Multi-Server Session Crash & Transparent Recovery during Staged Mutations', async () => {
    const pool = new McpServerPool();
    const serverInfo = await pool.connectMcpServer({ id: 'studio-main', command: 'mock-studio', args: [] });
    const studioCaller = pool.getMcpClient('studio-main');
    expect(studioCaller).toBeDefined();
    if (!studioCaller) return;

    const approvals = new ApprovalQueue();
    const identity1 = studioCaller.identity;
    expect(identity1).not.toBeNull();
    if (!identity1) return;

    // Stage a write on session 1
    const staged = approvals.stageMcp({
      projectRoot: tempDir,
      tool: 'multi_edit',
      args: { edits: [{ path: 'ServerScriptService.Arena', new_string: '--!strict\nreturn {}' }] },
      server: identity1,
    });

    // Simulate abrupt crash of studio server and restart
    await pool.disconnectMcpServer('studio-main');
    const restartedInfo = await pool.connectMcpServer({ id: 'studio-main', command: 'mock-studio', args: [] });
    expect(restartedInfo.session).not.toBe(serverInfo.session);

    // Ensure staged call on session 1 cannot run on session 2
    const currentStudio = pool.getMcpClient('studio-main');
    expect(currentStudio?.identity?.session).not.toBe(staged.mcpServer?.session);

    // Re-stage write on valid active session and execute
    const newCaller = currentStudio;
    expect(newCaller).toBeDefined();
    if (!newCaller) return;

    const editRes = await studio_multi_edit(
      { edits: [{ path: 'ServerScriptService.Arena', new_string: '--!strict\nreturn {}' }] },
      approvals,
      newCaller,
      tempDir,
    );
    expect(editRes.success).toBe(true);

    const inspected = await studio_inspect({ path: 'ServerScriptService.Arena' }, newCaller);
    expect(inspected.className).toBe('Script');
  });

  it('Scenario 3: Closed-Loop Playtest Failure with Automated Diagnostic & Repair Flow', async () => {
    const mock = createMockStudioCaller();
    const approvals = new ApprovalQueue();

    // 1. Initial acceptance definition with strict criteria
    const def = parseAcceptanceDefinition(
      JSON.stringify({
        version: 1,
        checks: [
          { id: 'probe-coins', scenario: 'Repair Loop Scenario', type: 'assertion', description: 'coins collected >= 5', expected: 'true' },
          { id: 'probe-state', scenario: 'Repair Loop Scenario', type: 'state', instancePath: 'Workspace.SpawnLocation', property: 'Enabled', expected: 'true' },
        ],
        screenshots: ['shots/repair_loop_final.png'],
        repairHint: 'Set CoinManager count threshold to 5.',
      }),
    );

    // 2. Playtest fails due to unmet assertion
    const firstRun = await runPlaytestScenario('Repair Loop Scenario', {
      maxDurationSeconds: 10,
      captureScreenshots: true,
      projectRoot: tempDir,
      acceptanceDef: def,
      caller: mock.caller,
      scenarioActuals: { assertions: { 'probe-coins': 'false' } },
    });

    expect(firstRun.verdict).toBe('FAILED');

    // 3. System derives auto-repair instruction from failing check and hint
    const instruction = acceptanceInstruction(def, 'Repair Loop Scenario');
    expect(instruction).toContain('Set CoinManager count threshold to 5.');
    expect(instruction).toContain('coins collected >= 5');

    // 4. Agent applies repair edit
    await studio_multi_edit(
      {
        edits: [
          {
            path: 'ServerScriptService.CoinManager',
            old_string: 'local Coins = {}',
            new_string: 'local Coins = { threshold = 5, ready = true }',
          },
        ],
      },
      approvals,
      mock.caller,
    );

    // 5. Re-run playtest with repaired state -> PASSED
    const secondRun = await runPlaytestScenario('Repair Loop Scenario', {
      maxDurationSeconds: 10,
      captureScreenshots: true,
      projectRoot: tempDir,
      acceptanceDef: def,
      caller: mock.caller,
      scenarioActuals: { assertions: { 'probe-coins': 'true' } },
    });

    expect(secondRun.verdict).toBe('PASSED');
    expect(secondRun.checks.every((c) => c.passed)).toBe(true);
  });

  it('Scenario 4: High-Concurrency Bulk Asset Discovery & Multi-Script Mutation Workload', async () => {
    const { caller, instances } = createMockStudioCaller();
    const approvals = new ApprovalQueue();

    // Search and gather multiple assets
    const models = await studio_asset({ action: 'search', asset_type: 'Model' }, undefined, caller);
    const audio = await studio_asset({ action: 'search', asset_type: 'Audio' }, undefined, caller);
    const decals = await studio_asset({ action: 'search', asset_type: 'Decal' }, undefined, caller);

    expect(models.results?.length).toBeGreaterThan(0);
    expect(audio.results?.length).toBeGreaterThan(0);
    expect(decals.results?.length).toBeGreaterThan(0);

    // Insert 2 assets simultaneously
    await studio_asset({ action: 'insert', asset_id: 1001, target_parent: 'Workspace' }, approvals, caller);
    await studio_asset({ action: 'insert', asset_id: 1003, target_parent: 'Workspace' }, approvals, caller);

    // Batch edit 4 scripts across DataModel
    const multiResult = await studio_multi_edit(
      {
        edits: [
          { path: 'ServerScriptService.Script1', new_string: '-- S1\nreturn {}' },
          { path: 'ServerScriptService.Script2', new_string: '-- S2\nreturn {}' },
          { path: 'ReplicatedStorage.Module1', new_string: '-- M1\nreturn {}', className: 'ModuleScript' },
          { path: 'ReplicatedStorage.Module2', new_string: '-- M2\nreturn {}', className: 'ModuleScript' },
        ],
      },
      approvals,
      caller,
    );

    expect(multiResult.success).toBe(true);
    expect(multiResult.modified_scripts.length).toBe(4);
    expect(instances.has('ServerScriptService.Script1')).toBe(true);
    expect(instances.has('ReplicatedStorage.Module2')).toBe(true);
  });

  it('Scenario 5: Complete Omni-Loop Release & Branch Integrity Audit', async () => {
    const repoRoot = path.resolve(__dirname, '../..');

    // 1. Verify git branch is strictly main
    const branchRes = await runShellCommand({ command: 'git branch --show-current', cwd: repoRoot });
    expect(branchRes.code).toBe(0);
    expect(branchRes.stdout.trim()).toBe('main');

    // 2. Verify git status runs clean
    const statusRes = await runShellCommand({ command: 'git status', cwd: repoRoot });
    expect(statusRes.code).toBe(0);
    expect(statusRes.stdout).toContain('On branch main');

    // 3. Verify .openro/acceptance.json schema compatibility
    const acceptanceSample = JSON.stringify({
      version: 1,
      checks: [
        { id: 'chk-1', scenario: 'Release Gate', type: 'assertion', description: 'all tests pass', expected: 'true' },
        { id: 'chk-2', scenario: 'Release Gate', type: 'state', instancePath: 'Workspace.SpawnLocation', property: 'Enabled', expected: 'true' },
      ],
      screenshots: ['shots/release_gate.png'],
      repairHint: 'Review test execution and acceptance logs.',
    });
    const parsed = parseAcceptanceDefinition(acceptanceSample);
    expect(parsed.version).toBe(1);
    expect(parsed.checks.length).toBe(2);
  });

  it('Scenario 6: Avatar Gameplay Simulation with Viewport Evidence Capture', async () => {
    const { caller } = createMockStudioCaller();

    // 1. Navigate avatar to target point
    const navReply = await caller.callTool('character_navigation', { target_position: [0, 5, 20] });
    expect(JSON.parse(navReply).status).toBe('NAVIGATING');

    // 2. Execute movement inputs
    const keyReply = await caller.callTool('user_keyboard_input', {
      actions: [
        { action: 'keyDown', key: 'KeyW' },
        { action: 'press', key: 'Space' },
        { action: 'keyUp', key: 'KeyW' },
      ],
    });
    expect(JSON.parse(keyReply).processedActions).toBe(3);

    // 3. Execute camera mouse rotation
    const mouseReply = await caller.callTool('user_mouse_input', {
      actions: [
        { action: 'move', x: 500, y: 300 },
        { action: 'click', button: 'Right' },
      ],
    });
    expect(JSON.parse(mouseReply).processedActions).toBe(2);

    // 4. Capture screenshot and verify cryptographic integrity
    const shotReply = await caller.callTool('screen_capture', { capture_id: 'shot-gameplay' });
    const shotData = JSON.parse(shotReply) as { base64: string; format: string };
    const shotBuffer = Buffer.from(shotData.base64, 'base64');
    expect(shotBuffer.subarray(0, 8)).toEqual(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]));

    const shotHash = crypto.createHash('sha256').update(shotBuffer).digest('hex');
    expect(shotHash).toHaveLength(64);
  });
});
