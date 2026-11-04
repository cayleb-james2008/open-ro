import { promises as fs } from 'node:fs';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import {
  parseAcceptanceDefinition,
  type AcceptanceDefinition,
} from '../electron/acceptanceChecks';
import { ApprovalQueue } from '../electron/approvals';
import type { McpCaller } from '../electron/mcpTools';
import {
  captureStudioScreenshot,
  getActivePlaytestSession,
  queryInstanceStateProperty,
  resetPlaytestSession,
  runPlaytestScenario,
  sanitizeScreenshotSlug,
  validateScreenshotPath,
  verifyContainedPath,
} from '../electron/playtestRunner';
import {
  stageStudioTool,
  StudioReads,
  studio_keyboard,
  studio_mouse,
  studio_navigate,
} from '../electron/studioTools';
import type { McpToolSummary } from '../electron/types';

const VALID_PNG_BASE64 = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==';

interface MockCallerOptions {
  tools?: string[];
  running?: boolean;
  hangOnTool?: string;
  crashOnTool?: string;
  toolHandler?: (name: string, args: Record<string, unknown>) => Promise<string> | string;
}

function createMockPlaytestCaller(options: MockCallerOptions = {}) {
  const calls: Array<{ name: string; args: Record<string, unknown>; timestamp: number }> = [];
  let isRunning = options.running ?? true;

  const toolList: McpToolSummary[] = (options.tools ?? [
    'start_stop_play',
    'character_navigation',
    'user_keyboard_input',
    'user_mouse_input',
    'get_console_output',
    'screen_capture',
    'inspect_instance',
  ]).map((name) => ({ name, description: '', inputSchema: { type: 'object' } }));

  const caller: McpCaller = {
    get running() {
      return isRunning;
    },
    setRunning(val: boolean) {
      isRunning = val;
    },
    identity: { command: 'MockStudioMCP', args: [], session: 'mock-session-challenger2' },
    listTools: () => toolList,
    callTool: async (name: string, args: Record<string, unknown>): Promise<string> => {
      calls.push({ name, args, timestamp: Date.now() });
      if (!isRunning) {
        throw new Error('Studio MCP server is disconnected.');
      }
      if (options.crashOnTool === name) {
        isRunning = false;
        throw new Error(`MCP process crashed during ${name}`);
      }
      if (options.hangOnTool === name) {
        return new Promise(() => {});
      }
      if (options.toolHandler) {
        return options.toolHandler(name, args);
      }
      if (name === 'start_stop_play') {
        const isStart = Boolean(args.is_start ?? (args.mode === 'Play' || args.mode === 'Run'));
        return JSON.stringify({ success: true, mode: isStart ? 'Play' : 'Stop' });
      }
      if (name === 'get_console_output') {
        return '[Server] Game running\n[Client] Avatar ready';
      }
      if (name === 'character_navigation') {
        return JSON.stringify({ status: 'NAVIGATED', destination: args.instance_path ?? [args.x, args.y, args.z] });
      }
      if (name === 'user_keyboard_input') {
        return JSON.stringify({ status: 'KEYBOARD_OK', processed: Array.isArray(args.actions) ? args.actions.length : 0 });
      }
      if (name === 'user_mouse_input') {
        return JSON.stringify({ status: 'MOUSE_OK', processed: Array.isArray(args.actions) ? args.actions.length : 0 });
      }
      if (name === 'screen_capture') {
        return JSON.stringify({ base64: VALID_PNG_BASE64 });
      }
      if (name === 'inspect_instance') {
        return JSON.stringify({
          path: args.path,
          className: 'Part',
          properties: { Anchored: 'true' },
          attributes: {},
          children: [],
        });
      }
      return JSON.stringify({ success: true });
    },
    callToolResult: async (name: string, args: Record<string, unknown>) => {
      calls.push({ name, args, timestamp: Date.now() });
      if (!isRunning) {
        throw new Error('Studio MCP server is disconnected.');
      }
      if (name === 'screen_capture') {
        return {
          text: 'screen_capture ok',
          images: [{ mimeType: 'image/png', data: VALID_PNG_BASE64 }],
          isError: false,
        };
      }
      const text = await caller.callTool(name, args);
      return { text, images: [], isError: false };
    },
  };

  return { caller, calls, setRunning: (val: boolean) => { isRunning = val; } };
}

describe('Milestone 2 Challenger 2 Empirical Stress Verification', () => {
  let tempDir: string;

  beforeEach(async () => {
    resetPlaytestSession();
    tempDir = await fs.mkdtemp(path.join(tmpdir(), 'openro-challenger2-'));
  });

  afterEach(async () => {
    resetPlaytestSession();
    await fs.rm(tempDir, { recursive: true, force: true });
  });

  // =========================================================================
  // Focus Area 1: Win32 DOS Reserved Device Names & Path Safety
  // =========================================================================
  describe('Focus Area 1: Win32 DOS Reserved Device Names & Path Safety', () => {
    it('rejects reserved device names across mixed cases, compound extensions, and bare stems', () => {
      const reservedTargets = [
        'nul.png',
        'NUL.PNG',
        'NuL.png',
        'con.png',
        'CON.PNG',
        'cOn.png',
        'prn.png',
        'PRN.tar.gz',
        'PrN.bin',
        'aux.png',
        'AUX.ext',
        'AuX.bin',
        'com1.png',
        'COM1.tar.gz',
        'cOm2.bin',
        'COM9.dat',
        'lpt1.png',
        'LPT1.tar.gz',
        'lPt5.bin',
        'LPT9.output',
        'nul',
        'CON',
        'aux',
      ];

      for (const target of reservedTargets) {
        expect(
          () => validateScreenshotPath(target),
          `Expected validateScreenshotPath to reject reserved target: "${target}"`,
        ).toThrow(/contains reserved Windows device name/);
      }
    });

    it('rejects reserved device names in deep subdirectories and nested segments', () => {
      const nestedTargets = [
        'shots/sub/nul.png',
        'captures/deep/nested/path/COM1.png',
        'a/b/c/d/aux.bin',
        'shots/nested/sub/lpt2.png',
        'deep/dir/PRN.tar.gz',
        'shots/sub/nul',
        'shots\\sub\\nul.png',
        'captures\\deep\\nested\\COM1.png',
        'aux/frame.png',
        'shots/com1/output.png',
        'nul/test.png',
        'shots/prn/sub/shot.png',
        'folder/LPT9/evidence.png',
      ];

      for (const target of nestedTargets) {
        expect(
          () => validateScreenshotPath(target),
          `Expected validateScreenshotPath to reject nested target: "${target}"`,
        ).toThrow(/contains reserved Windows device name/);
      }
    });

    it('permits legitimate filenames that contain reserved substrings but are not reserved device names', () => {
      const legitimatePaths = [
        'connect.png',
        'nulled.bin',
        'auxiliary.png',
        'computer.tar.gz',
        'prn_screen.png',
        'lpt_port.png',
        'console_output.png',
        'com10.png',
        'lpt10.png',
        'shots/sub/connected.png',
        'captures/deep/nested/computer_view.png',
        'shots/announcement_screen.png',
      ];

      for (const target of legitimatePaths) {
        expect(
          () => validateScreenshotPath(target),
          `Expected validateScreenshotPath to allow legitimate target: "${target}"`,
        ).not.toThrow();
      }
    });

    it('defends against aggressive directory traversal attacks in validateScreenshotPath and verifyContainedPath', () => {
      const traversalPaths = [
        '../evil.png',
        '../../secret.txt',
        'shots/../../etc/passwd',
        '..\\windows\\system32\\cmd.exe',
        'shots/sub/../../../pwned.png',
      ];

      for (const target of traversalPaths) {
        expect(
          () => validateScreenshotPath(target),
          `Expected validateScreenshotPath to reject traversal target: "${target}"`,
        ).toThrow(/escapes the capture directory/);
      }

      expect(() => verifyContainedPath(tempDir, path.join(tempDir, '../outside.png'))).toThrow(
        /escapes target directory/,
      );
      expect(() => verifyContainedPath(tempDir, `${tempDir}-sibling/evil.png`)).toThrow(
        /escapes target directory/,
      );
    });

    it('sanitizes Windows reserved names and special characters via sanitizeScreenshotSlug', () => {
      expect(sanitizeScreenshotSlug('nul')).toBe('safe_nul');
      expect(sanitizeScreenshotSlug('NUL')).toBe('safe_NUL');
      expect(sanitizeScreenshotSlug('CON')).toBe('safe_CON');
      expect(sanitizeScreenshotSlug('aux')).toBe('safe_aux');
      expect(sanitizeScreenshotSlug('COM1')).toBe('safe_COM1');
      expect(sanitizeScreenshotSlug('lpt9')).toBe('safe_lpt9');
      expect(sanitizeScreenshotSlug('../../../windows/system32')).toBe('windows_system32');
      expect(sanitizeScreenshotSlug('..\\..\\secret.txt')).toBe('secret_txt');
      expect(sanitizeScreenshotSlug('hello@#world$$%123')).toBe('hello_world_123');
    });

    it('captureStudioScreenshot rejects targetRelPath with deep reserved names without writing files to disk', async () => {
      const { caller } = createMockPlaytestCaller();
      const captureDir = path.join(tempDir, 'captures');

      const reservedPaths = ['shots/sub/nul.png', 'captures/deep/nested/COM1.png', 'aux/frame.png'];

      for (const resPath of reservedPaths) {
        await expect(
          captureStudioScreenshot(caller, 'attack-scenario', captureDir, undefined, resPath, tempDir),
        ).rejects.toThrow(/contains reserved Windows device name/);

        // Verify no file was written
        const fullTarget = path.join(tempDir, resPath);
        await expect(fs.access(fullTarget)).rejects.toThrow();
      }
    });
  });

  // =========================================================================
  // Focus Area 2: Non-Self-Certifying Checks & Acceptance Multiset Enforcement
  // =========================================================================
  describe('Focus Area 2: Non-Self-Certifying Checks & Acceptance Multiset Enforcement', () => {
    it('rejects scenario with FAILED verdict when scenarioActuals assertions do not match expectations', async () => {
      const { caller } = createMockPlaytestCaller();
      const acceptanceDef: AcceptanceDefinition = {
        version: 1,
        screenshots: [],
        checks: [
          {
            id: 'assert-coin-collected',
            scenario: 'coin-scenario',
            type: 'assertion',
            description: 'Coin collected count',
            expected: '10',
          },
        ],
      };

      const result = await runPlaytestScenario(
        'coin-scenario',
        {
          acceptanceDef,
          spawnDelayMs: 0,
          scenarioActuals: {
            assertions: {
              'assert-coin-collected': '5', // Mismatch: expected 10, got 5
            },
          },
        },
        caller,
        { projectRoot: tempDir },
      );

      expect(result.verdict).toBe('FAILED');
      expect(result.checks).toHaveLength(1);
      expect(result.checks[0]).toMatchObject({
        name: 'Coin collected count',
        expected: '10',
        actual: '5',
        passed: false,
      });
    });

    it('rejects scenario with FAILED verdict and unobserved actual when assertion is missing from scenarioActuals', async () => {
      const { caller } = createMockPlaytestCaller();
      const acceptanceDef: AcceptanceDefinition = {
        version: 1,
        screenshots: [],
        checks: [
          {
            id: 'assert-observed',
            scenario: 'unobserved-scenario',
            type: 'assertion',
            description: 'Observed assertion',
            expected: 'true',
          },
          {
            id: 'assert-missing',
            scenario: 'unobserved-scenario',
            type: 'assertion',
            description: 'Unobserved assertion',
            expected: 'true',
          },
        ],
      };

      const result = await runPlaytestScenario(
        'unobserved-scenario',
        {
          acceptanceDef,
          spawnDelayMs: 0,
          scenarioActuals: {
            assertions: {
              'assert-observed': 'true',
              // 'assert-missing' omitted intentionally
            },
          },
        },
        caller,
        { projectRoot: tempDir },
      );

      expect(result.verdict).toBe('FAILED');
      const missingCheck = result.checks.find((c) => c.name === 'Unobserved assertion');
      expect(missingCheck).toBeDefined();
      expect(missingCheck?.actual).toBeUndefined();
      expect(missingCheck?.passed).toBe(false);
    });

    it('rejects scenario with FAILED verdict when scenarioActuals state checks do not match expectations', async () => {
      const { caller } = createMockPlaytestCaller();
      const acceptanceDef: AcceptanceDefinition = {
        version: 1,
        screenshots: [],
        checks: [
          {
            id: 'state-door-open',
            scenario: 'door-scenario',
            type: 'state',
            instancePath: 'Workspace.Door',
            property: 'Transparency',
            expected: '1.0',
          },
        ],
      };

      const result = await runPlaytestScenario(
        'door-scenario',
        {
          acceptanceDef,
          spawnDelayMs: 0,
          scenarioActuals: {
            states: {
              'state-door-open': '0.0', // Mismatch: expected 1.0, got 0.0
            },
          },
        },
        caller,
        { projectRoot: tempDir },
      );

      expect(result.verdict).toBe('FAILED');
      expect(result.checks).toHaveLength(1);
      expect(result.checks[0]).toMatchObject({
        name: 'Workspace.Door.Transparency',
        expected: '1.0',
        actual: '0.0',
        passed: false,
      });
    });

    it('enforces exact multiset matching on acceptance assertions (detects extra and missing checks)', async () => {
      const { caller } = createMockPlaytestCaller();
      const acceptanceDef: AcceptanceDefinition = {
        version: 1,
        screenshots: [],
        checks: [
          {
            id: 'chk-1',
            scenario: 'multiset-scenario',
            type: 'assertion',
            description: 'Primary objective',
            expected: 'completed',
          },
        ],
      };

      // 1. Extra unexpected check injected
      const extraResult = await runPlaytestScenario(
        'multiset-scenario',
        {
          acceptanceDef,
          spawnDelayMs: 0,
          customChecks: [
            { name: 'Primary objective', expected: 'completed', actual: 'completed' },
            { name: 'Extra rogue check', expected: 'unauthorized', actual: 'unauthorized' },
          ],
        },
        caller,
        { projectRoot: tempDir },
      );

      expect(extraResult.verdict).toBe('FAILED');
      expect(extraResult.error).toMatch(/assertion set does not match/);
      expect(extraResult.error).toMatch(/unexpected/);

      // 2. Count mismatch (expected 1, report has 2 of the same)
      const duplicateResult = await runPlaytestScenario(
        'multiset-scenario',
        {
          acceptanceDef,
          spawnDelayMs: 0,
          customChecks: [
            { name: 'Primary objective', expected: 'completed', actual: 'completed' },
            { name: 'Primary objective', expected: 'completed', actual: 'completed' },
          ],
        },
        caller,
        { projectRoot: tempDir },
      );

      expect(duplicateResult.verdict).toBe('FAILED');
      expect(duplicateResult.error).toMatch(/assertion set does not match/);
    });

    it('enforces exact multiset matching on instance state checks', async () => {
      const { caller } = createMockPlaytestCaller();
      const acceptanceDef: AcceptanceDefinition = {
        version: 1,
        screenshots: [],
        checks: [
          {
            id: 'state-1',
            scenario: 'state-multiset',
            type: 'state',
            instancePath: 'Workspace.Coin',
            property: 'Anchored',
            expected: 'true',
          },
        ],
      };

      // Extra state check injected
      const extraStateResult = await runPlaytestScenario(
        'state-multiset',
        {
          acceptanceDef,
          spawnDelayMs: 0,
          stateChecks: [
            { instancePath: 'Workspace.Coin', property: 'Anchored', expected: 'true', actual: 'true' },
            { instancePath: 'Workspace.Bonus', property: 'CanCollide', expected: 'false', actual: 'false' },
          ],
        },
        caller,
        { projectRoot: tempDir },
      );

      expect(extraStateResult.verdict).toBe('FAILED');
      expect(extraStateResult.error).toMatch(/state set does not match/);
      expect(extraStateResult.error).toMatch(/unexpected/);
    });

    it('validates against real .openro/acceptance.json file on filesystem', async () => {
      const { caller } = createMockPlaytestCaller();
      const openroDir = path.join(tempDir, '.openro');
      await fs.mkdir(openroDir, { recursive: true });

      const realAcceptance = {
        version: 1,
        screenshots: [],
        checks: [
          {
            id: 'fs-assert-1',
            scenario: 'filesystem-scenario',
            type: 'assertion',
            description: 'Avatar spawned cleanly',
            expected: 'true',
          },
          {
            id: 'fs-state-1',
            scenario: 'filesystem-scenario',
            type: 'state',
            instancePath: 'Workspace.SpawnLocation',
            property: 'Enabled',
            expected: 'true',
          },
        ],
      };
      await fs.writeFile(path.join(openroDir, 'acceptance.json'), JSON.stringify(realAcceptance, null, 2), 'utf8');

      // 1. Matching scenarioActuals passes cleanly
      const passResult = await runPlaytestScenario(
        'filesystem-scenario',
        {
          spawnDelayMs: 0,
          scenarioActuals: {
            assertions: {
              'fs-assert-1': 'true',
            },
            states: {
              'fs-state-1': 'true',
            },
          },
        },
        caller,
        { projectRoot: tempDir },
      );
      expect(passResult.verdict).toBe('PASSED');

      // 2. Mismatched scenarioActuals fails
      const failResult = await runPlaytestScenario(
        'filesystem-scenario',
        {
          spawnDelayMs: 0,
          scenarioActuals: {
            assertions: {
              'fs-assert-1': 'false',
            },
            states: {
              'fs-state-1': 'true',
            },
          },
        },
        caller,
        { projectRoot: tempDir },
      );
      expect(failResult.verdict).toBe('FAILED');
    });
  });

  // =========================================================================
  // Focus Area 3: Live Inspection Fallback (queryInstanceStateProperty & Integration)
  // =========================================================================
  describe('Focus Area 3: Live Inspection Fallback', () => {
    it('extracts properties from JSON response with top-level properties object', async () => {
      const { caller } = createMockPlaytestCaller({
        toolHandler: (name) => {
          if (name === 'inspect_instance') {
            return JSON.stringify({
              path: 'Workspace.Avatar',
              className: 'Model',
              properties: { WalkSpeed: 16, Health: 100 },
              attributes: {},
            });
          }
          return JSON.stringify({ success: true });
        },
      });

      const walkSpeed = await queryInstanceStateProperty(caller, 'Workspace.Avatar', 'WalkSpeed');
      expect(walkSpeed).toBe(16);

      const health = await queryInstanceStateProperty(caller, 'Workspace.Avatar', 'Health');
      expect(health).toBe(100);
    });

    it('extracts attributes from JSON response with top-level attributes object', async () => {
      const { caller } = createMockPlaytestCaller({
        toolHandler: (name) => {
          if (name === 'inspect_instance') {
            return JSON.stringify({
              path: 'Workspace.PlayerScore',
              className: 'Configuration',
              properties: {},
              attributes: { CurrentScore: 99, QuestComplete: true },
            });
          }
          return JSON.stringify({ success: true });
        },
      });

      const score = await queryInstanceStateProperty(caller, 'Workspace.PlayerScore', 'CurrentScore');
      expect(score).toBe(99);

      const complete = await queryInstanceStateProperty(caller, 'Workspace.PlayerScore', 'QuestComplete');
      expect(complete).toBe(true);
    });

    it('extracts direct fields from JSON response with top-level instance data', async () => {
      const { caller } = createMockPlaytestCaller({
        toolHandler: (name) => {
          if (name === 'inspect_instance') {
            return JSON.stringify({
              path: 'Workspace.Part',
              className: 'Part',
              Anchored: true,
              Transparency: 0.5,
            });
          }
          return JSON.stringify({ success: true });
        },
      });

      const anchored = await queryInstanceStateProperty(caller, 'Workspace.Part', 'Anchored');
      expect(anchored).toBe(true);

      const transparency = await queryInstanceStateProperty(caller, 'Workspace.Part', 'Transparency');
      expect(transparency).toBe(0.5);
    });

    it('extracts properties and attributes from JSON response containing a matches array', async () => {
      const { caller } = createMockPlaytestCaller({
        toolHandler: (name) => {
          if (name === 'inspect_instance') {
            return JSON.stringify({
              matches: [
                {
                  path: 'Workspace.TreasureChest',
                  className: 'Model',
                  properties: { Locked: false },
                  attributes: { GoldCount: 250 },
                },
              ],
            });
          }
          return JSON.stringify({ success: true });
        },
      });

      const locked = await queryInstanceStateProperty(caller, 'Workspace.TreasureChest', 'Locked');
      expect(locked).toBe(false);

      const gold = await queryInstanceStateProperty(caller, 'Workspace.TreasureChest', 'GoldCount');
      expect(gold).toBe(250);
    });

    it('returns undefined when property is absent across properties, attributes, and data', async () => {
      const { caller } = createMockPlaytestCaller({
        toolHandler: (name) => {
          if (name === 'inspect_instance') {
            return JSON.stringify({
              path: 'Workspace.Part',
              properties: { Name: 'Coin' },
              attributes: { Rarity: 'Common' },
            });
          }
          return JSON.stringify({ success: true });
        },
      });

      const nonExistent = await queryInstanceStateProperty(caller, 'Workspace.Part', 'NonExistentProperty');
      expect(nonExistent).toBeUndefined();
    });

    it('returns undefined gracefully on malformed JSON or inspection throw', async () => {
      const { caller } = createMockPlaytestCaller({
        toolHandler: (name) => {
          if (name === 'inspect_instance') {
            return '{ malformed json: not valid';
          }
          return JSON.stringify({ success: true });
        },
      });

      const result = await queryInstanceStateProperty(caller, 'Workspace.Part', 'AnyProp');
      expect(result).toBeUndefined();
    });

    it('returns undefined when caller is disconnected or inspect_instance tool is missing', async () => {
      const { caller, setRunning } = createMockPlaytestCaller();
      setRunning(false);

      const resDisconnected = await queryInstanceStateProperty(caller, 'Workspace.Part', 'AnyProp');
      expect(resDisconnected).toBeUndefined();

      const { caller: noInspectCaller } = createMockPlaytestCaller({
        tools: ['start_stop_play', 'screen_capture'],
      });
      const resNoTool = await queryInstanceStateProperty(noInspectCaller, 'Workspace.Part', 'AnyProp');
      expect(resNoTool).toBeUndefined();
    });

    it('resolves live state check via inspect_instance fallback when scenarioActuals is omitted in runPlaytestScenario', async () => {
      let inspectCalled = false;
      const { caller } = createMockPlaytestCaller({
        toolHandler: (name, args) => {
          if (name === 'inspect_instance') {
            inspectCalled = true;
            return JSON.stringify({
              matches: [
                {
                  path: args.path,
                  className: 'Part',
                  properties: { Anchored: 'true' },
                },
              ],
            });
          }
          if (name === 'start_stop_play') {
            return JSON.stringify({ success: true, mode: args.mode });
          }
          if (name === 'screen_capture') {
            return JSON.stringify({ base64: VALID_PNG_BASE64 });
          }
          return JSON.stringify({ success: true });
        },
      });

      const acceptanceDef: AcceptanceDefinition = {
        version: 1,
        screenshots: [],
        checks: [
          {
            id: 'assert-live-spawn',
            scenario: 'live-inspect-scenario',
            type: 'assertion',
            description: 'Avatar spawned successfully',
            expected: 'true',
          },
          {
            id: 'state-live-anchored',
            scenario: 'live-inspect-scenario',
            type: 'state',
            instancePath: 'Workspace.AnchorPart',
            property: 'Anchored',
            expected: 'true',
          },
        ],
      };

      const result = await runPlaytestScenario(
        'live-inspect-scenario',
        {
          acceptanceDef,
          spawnDelayMs: 0,
          // Note: scenarioActuals is omitted intentionally to trigger live inspection fallback
        },
        caller,
        { projectRoot: tempDir },
      );

      expect(inspectCalled).toBe(true);
      expect(result.verdict).toBe('PASSED');
      expect(result.checks).toHaveLength(2);
      const stateCheck = result.checks.find((c) => c.name === 'Workspace.AnchorPart.Anchored');
      expect(stateCheck).toMatchObject({
        name: 'Workspace.AnchorPart.Anchored',
        expected: 'true',
        actual: 'true',
        passed: true,
      });
    });

    it('records FAILED verdict and unobserved actual when live inspect_instance property is missing', async () => {
      const { caller } = createMockPlaytestCaller({
        toolHandler: (name) => {
          if (name === 'inspect_instance') {
            return JSON.stringify({
              properties: { OtherProp: 'abc' },
            });
          }
          if (name === 'start_stop_play') {
            return JSON.stringify({ success: true });
          }
          if (name === 'screen_capture') {
            return JSON.stringify({ base64: VALID_PNG_BASE64 });
          }
          return JSON.stringify({ success: true });
        },
      });

      const acceptanceDef: AcceptanceDefinition = {
        version: 1,
        screenshots: [],
        checks: [
          {
            id: 'state-missing-prop',
            scenario: 'missing-prop-scenario',
            type: 'state',
            instancePath: 'Workspace.Part',
            property: 'RequiredProp',
            expected: 'true',
          },
        ],
      };

      const result = await runPlaytestScenario(
        'missing-prop-scenario',
        {
          acceptanceDef,
          spawnDelayMs: 0,
        },
        caller,
        { projectRoot: tempDir },
      );

      expect(result.verdict).toBe('FAILED');
      expect(result.checks[0]).toMatchObject({
        name: 'Workspace.Part.RequiredProp',
        expected: 'true',
        actual: undefined,
        passed: false,
      });
    });
  });

  // =========================================================================
  // Focus Area 4: Standalone Studio Avatar Interaction Tools & Approval Staging
  // =========================================================================
  describe('Focus Area 4: Standalone Studio Avatar Interaction Tools & Approval Staging', () => {
    it('validates studio_navigate coordinates and instance targets', async () => {
      const { caller, calls } = createMockPlaytestCaller();

      const resCoord = await studio_navigate(caller, {
        x: 10,
        y: 5,
        z: -20,
        studio_id: 'studio-test-1',
      });
      expect(resCoord.success).toBe(true);

      const navCall = calls.find((c) => c.name === 'character_navigation');
      expect(navCall).toBeDefined();
      expect(navCall?.args).toMatchObject({
        datamodel_type: 'Client',
        studio_id: 'studio-test-1',
        x: 10,
        y: 5,
        z: -20,
      });

      const resInstance = await studio_navigate(caller, {
        instance_path: 'Workspace.SpawnLocation',
        studio_id: 'studio-test-1',
      });
      expect(resInstance.success).toBe(true);
    });

    it('enforces bounds clamping on studio_navigate speed_multiplier', async () => {
      const { caller, calls } = createMockPlaytestCaller();

      // Lower clamp bound (0.1)
      await studio_navigate(caller, { x: 0, y: 0, z: 0, speed_multiplier: 0.01 });
      expect(calls[calls.length - 1].args.speed_multiplier).toBe(0.1);

      await studio_navigate(caller, { x: 0, y: 0, z: 0, speed_multiplier: -5 });
      expect(calls[calls.length - 1].args.speed_multiplier).toBe(0.1);

      // Upper clamp bound (10.0)
      await studio_navigate(caller, { x: 0, y: 0, z: 0, speed_multiplier: 999.0 });
      expect(calls[calls.length - 1].args.speed_multiplier).toBe(10.0);

      // NaN fallback to 1.0
      await studio_navigate(caller, { x: 0, y: 0, z: 0, speed_multiplier: Number.NaN });
      expect(calls[calls.length - 1].args.speed_multiplier).toBe(1.0);
    });

    it('rejects invalid studio_navigate calls missing both coordinates and instance path', async () => {
      const { caller } = createMockPlaytestCaller();

      await expect(studio_navigate(caller, {} as any)).rejects.toThrow(
        /character_navigation requires instance_path or coordinates/,
      );
    });

    it('validates studio_keyboard actions and auto-maps text to text_inputs', async () => {
      const { caller, calls } = createMockPlaytestCaller();

      const res = await studio_keyboard(caller, {
        actions: [{ action: 'textInput', text: 'Hello Roblox!' }],
        studio_id: 'studio-test-1',
      });

      expect(res.success).toBe(true);
      const kbCall = calls.find((c) => c.name === 'user_keyboard_input');
      expect(kbCall).toBeDefined();
      expect(kbCall?.args.actions).toEqual([
        { action: 'textInput', text: 'Hello Roblox!', text_inputs: 'Hello Roblox!' },
      ]);

      await expect(studio_keyboard(caller, { actions: [] })).rejects.toThrow(
        /actions must be a non-empty array/,
      );
    });

    it('validates studio_mouse actions with coordinates and buttons', async () => {
      const { caller, calls } = createMockPlaytestCaller();

      const res = await studio_mouse(caller, {
        actions: [
          { action: 'move', x: 200, y: 300 },
          { action: 'click', button: 'Left' },
        ],
        studio_id: 'studio-test-1',
      });

      expect(res.success).toBe(true);
      const mouseCall = calls.find((c) => c.name === 'user_mouse_input');
      expect(mouseCall).toBeDefined();
      expect(mouseCall?.args.actions).toHaveLength(2);

      await expect(studio_mouse(caller, { actions: [] })).rejects.toThrow(
        /actions must be a non-empty array/,
      );
    });

    it('tests stageStudioTool with studio_navigate for approval flow integrity', () => {
      const approvals = new ApprovalQueue();
      const reads = new StudioReads();
      const { caller } = createMockPlaytestCaller();

      const staged = stageStudioTool(approvals, caller, reads, {
        projectRoot: tempDir,
        name: 'studio_navigate',
        args: {
          studioId: 'studio-staged-1',
          instance_path: 'Workspace.Checkpoint1',
          speed_multiplier: 25.0, // Should be clamped to 10.0
        },
      });

      expect(staged.ok).toBe(true);
      expect(staged.output).toMatch(/Staged character navigation/);
      expect(staged.pendingApproval).toBeDefined();

      const item = approvals.take(staged.pendingApproval!.id, 'studio');
      expect(item).toBeDefined();
      expect(item.kind).toBe('studio');
      expect(item.mcpTool).toBe('character_navigation');
      const parsedArgs = JSON.parse(item.mcpArgs ?? '{}');
      expect(parsedArgs).toMatchObject({
        datamodel_type: 'Client',
        studio_id: 'studio-staged-1',
        instance_path: 'Workspace.Checkpoint1',
        speed_multiplier: 10.0,
      });
    });
  });
});
