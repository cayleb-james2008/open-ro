import { promises as fs } from 'node:fs';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { parseAcceptanceDefinition } from '../electron/acceptanceChecks';
import type { McpCaller } from '../electron/mcpTools';
import {
  captureStudioScreenshot,
  delayWithSignal,
  getActivePlaytestSession,
  queryInstanceStateProperty,
  resetPlaytestSession,
  runPlaytestScenario,
  sanitizeScreenshotSlug,
  stopPlaytestSafely,
  validateScreenshotPath,
  verifyContainedPath,
} from '../electron/playtestRunner';
import {
  studio_capture,
  studio_console,
  studio_keyboard,
  studio_mouse,
  studio_navigate,
  studio_playtest,
} from '../electron/studioTools';
import type { McpToolSummary } from '../electron/types';

const VALID_PNG_BASE64 = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==';

interface MockCallerOptions {
  tools?: string[];
  running?: boolean;
  hangOnTool?: string;
  crashOnTool?: string;
  hangOnStop?: boolean;
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
    identity: { command: 'MockStudioMCP', args: [], session: 'mock-session-m2' },
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
        return new Promise(() => {}); // Hang indefinitely
      }
      if (name === 'start_stop_play' && (args.is_start === false || args.mode === 'Stop') && options.hangOnStop) {
        return new Promise(() => {}); // Hang on stop
      }
      if (options.toolHandler) {
        return options.toolHandler(name, args);
      }
      if (name === 'start_stop_play') {
        const isStart = Boolean(args.is_start ?? (args.mode === 'Play' || args.mode === 'Run'));
        return JSON.stringify({ success: true, mode: isStart ? 'Play' : 'Stop' });
      }
      if (name === 'get_console_output') {
        return '[Server] Systems initialized\n[Client] Avatar ready';
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
          properties: { Enabled: 'true' },
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
      if (options.crashOnTool === name) {
        isRunning = false;
        throw new Error(`MCP process crashed during ${name}`);
      }
      if (options.hangOnTool === name) {
        return new Promise(() => {});
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

describe('Autonomous Playtest Runner (electron/playtestRunner.ts)', () => {
  let tempDir: string;

  beforeEach(async () => {
    resetPlaytestSession();
    tempDir = await fs.mkdtemp(path.join(tmpdir(), 'openro-playtest-runner-'));
  });

  afterEach(async () => {
    resetPlaytestSession();
    await fs.rm(tempDir, { recursive: true, force: true });
  });

  // ==========================================
  // Suite 1: Lifecycle & Happy Path
  // ==========================================
  describe('Suite 1: Lifecycle & Happy Path', () => {
    it('executes complete playtest scenario and records pass', async () => {
      const mock = createMockPlaytestCaller();
      const def = parseAcceptanceDefinition(
        JSON.stringify({
          version: 1,
          checks: [
            { id: 'c1', scenario: 'Happy Path Scenario', type: 'assertion', description: 'quest completed', expected: 'true' },
            { id: 'c2', scenario: 'Happy Path Scenario', type: 'state', instancePath: 'Workspace.Chest', property: 'Opened', expected: 'true' },
          ],
          screenshots: ['shots/happy_path.png'],
        }),
      );

      const result = await runPlaytestScenario(
        'Happy Path Scenario',
        {
          maxDurationSeconds: 10,
          captureScreenshots: true,
          projectRoot: tempDir,
          acceptanceDef: def,
          caller: mock.caller,
          actions: [
            { type: 'navigate', params: { x: 10, y: 0, z: 20, speed_multiplier: 1.5 } },
            { type: 'keyboard', params: { actions: [{ action: 'keyPress', key_code: 'Space' }] } },
            { type: 'mouse', params: { actions: [{ action: 'moveTo', x: 200, y: 300 }, { action: 'mouseButtonClick', mouse_button: 'left' }] } },
            { type: 'inspect', params: { path: 'Workspace.Chest' } },
          ],
          scenarioActuals: {
            assertions: { c1: 'true' },
            states: { c2: 'true' },
          },
        },
      );

      expect(result.verdict).toBe('PASSED');
      expect(result.checks).toHaveLength(2);
      expect(result.checks.every((c) => c.passed)).toBe(true);
      expect(result.screenshotPaths).toHaveLength(1);
      expect(result.screenshotPaths[0]).toBe('shots/happy_path.png');
      expect(result.durationMs).toBeGreaterThanOrEqual(0);

      // Verify sequence of tool calls
      const toolNames = mock.calls.map((c) => c.name);
      expect(toolNames[0]).toBe('start_stop_play');
      expect(mock.calls[0].args.is_start).toBe(true);

      expect(toolNames).toContain('character_navigation');
      expect(toolNames).toContain('user_keyboard_input');
      expect(toolNames).toContain('user_mouse_input');
      expect(toolNames).toContain('inspect_instance');
      expect(toolNames).toContain('screen_capture');
      expect(toolNames).toContain('get_console_output');

      const lastCall = mock.calls[mock.calls.length - 1];
      expect(lastCall.name).toBe('start_stop_play');
      expect(lastCall.args.is_start).toBe(false);

      // Check screenshot was actually written on disk
      const shotFullPath = path.join(tempDir, 'shots', 'happy_path.png');
      const shotStat = await fs.stat(shotFullPath);
      expect(shotStat.isFile()).toBe(true);
      expect(shotStat.size).toBeGreaterThan(0);
    });

    it('tolerates scenario with no acceptance file when custom checks match', async () => {
      const mock = createMockPlaytestCaller();

      const result = await runPlaytestScenario(
        'Custom Scenario Without File',
        {
          maxDurationSeconds: 10,
          projectRoot: tempDir,
          caller: mock.caller,
          customChecks: [
            { name: 'player spawned', expected: 'true', actual: 'true' },
            { name: 'health is full', expected: 100, actual: 100 },
          ],
          stateChecks: [
            { instancePath: 'Workspace.Spawn', property: 'Enabled', expected: 'true', actual: 'true' },
          ],
        },
      );

      expect(result.verdict).toBe('PASSED');
      expect(result.checks).toHaveLength(3);
      expect(result.checks.every((c) => c.passed)).toBe(true);

      // Ensured restored to stop mode
      const stopCalls = mock.calls.filter((c) => c.name === 'start_stop_play' && c.args.is_start === false);
      expect(stopCalls.length).toBeGreaterThanOrEqual(1);
    });
  });

  // ==========================================
  // Suite 2: Assertion & State Failures
  // ==========================================
  describe('Suite 2: Assertion & State Failures', () => {
    it('records FAILED verdict when assertion check fails', async () => {
      const mock = createMockPlaytestCaller();
      const def = parseAcceptanceDefinition(
        JSON.stringify({
          version: 1,
          checks: [
            { id: 'c1', scenario: 'Failing Assertion Scenario', type: 'assertion', description: 'Coins collected', expected: '1' },
            { id: 'c2', scenario: 'Failing Assertion Scenario', type: 'state', instancePath: 'Workspace.Coin', property: 'Transparency', expected: '1' },
          ],
          screenshots: ['shots/fail_shot.png'],
        }),
      );

      const result = await runPlaytestScenario(
        'Failing Assertion Scenario',
        {
          maxDurationSeconds: 10,
          projectRoot: tempDir,
          acceptanceDef: def,
          caller: mock.caller,
          scenarioActuals: {
            assertions: { c1: '0' }, // Mismatch: expected 1, got 0
            states: { c2: '1' },
          },
        },
      );

      expect(result.verdict).toBe('FAILED');
      expect(result.error).toMatch(/Coins collected.*expected 1.*got 0/);

      // Verify stop was still executed
      const stopCalls = mock.calls.filter((c) => c.name === 'start_stop_play' && c.args.is_start === false);
      expect(stopCalls).toHaveLength(1);
    });

    it('records FAILED verdict when acceptance multiset does not match', async () => {
      const mock = createMockPlaytestCaller();
      const def = parseAcceptanceDefinition(
        JSON.stringify({
          version: 1,
          checks: [
            { id: 'c1', scenario: 'Multiset Mismatch', type: 'assertion', description: 'coin 1', expected: 'true' },
            { id: 'c2', scenario: 'Multiset Mismatch', type: 'assertion', description: 'coin 2', expected: 'true' },
            { id: 's1', scenario: 'Multiset Mismatch', type: 'state', instancePath: 'Workspace.Coin', property: 'Enabled', expected: 'true' },
          ],
          screenshots: ['shots/multiset_fail.png'],
        }),
      );

      // Custom report with extra unexpected assertion and missing coin 2
      const result = await runPlaytestScenario(
        'Multiset Mismatch',
        {
          maxDurationSeconds: 10,
          projectRoot: tempDir,
          acceptanceDef: def,
          caller: mock.caller,
          customChecks: [
            { name: 'unexpected extra assertion', expected: 'true', actual: 'true' },
          ],
        },
      );

      expect(result.verdict).toBe('FAILED');
      expect(result.error).toBeDefined();

      const stopCalls = mock.calls.filter((c) => c.name === 'start_stop_play' && c.args.is_start === false);
      expect(stopCalls).toHaveLength(1);
    });

    it('queries inspect_instance for live state checks when state is not in scenarioActuals', async () => {
      const mock = createMockPlaytestCaller({
        toolHandler: (name, args) => {
          if (name === 'inspect_instance' && args.path === 'Workspace.Door') {
            return JSON.stringify({
              path: 'Workspace.Door',
              className: 'Part',
              properties: { Open: 'true' },
              attributes: {},
              children: [],
            });
          }
          if (name === 'start_stop_play') {
            return JSON.stringify({ success: true, mode: args.is_start ? 'Play' : 'Stop' });
          }
          if (name === 'screen_capture') {
            return JSON.stringify({ base64: VALID_PNG_BASE64 });
          }
          return JSON.stringify({ success: true });
        },
      });

      const def = parseAcceptanceDefinition(
        JSON.stringify({
          version: 1,
          checks: [
            { id: 'a1', scenario: 'Live State Scenario', type: 'assertion', description: 'door unlocked', expected: 'true' },
            { id: 's1', scenario: 'Live State Scenario', type: 'state', instancePath: 'Workspace.Door', property: 'Open', expected: 'true' },
          ],
          screenshots: ['shots/live_state.png'],
        }),
      );

      const result = await runPlaytestScenario(
        'Live State Scenario',
        {
          maxDurationSeconds: 5,
          spawnDelayMs: 0,
          projectRoot: tempDir,
          acceptanceDef: def,
          caller: mock.caller,
          // scenarioActuals provides assertion but omits state, forcing live inspect_instance query
          scenarioActuals: { assertions: { a1: 'true' } },
        },
      );

      expect(result.verdict).toBe('PASSED');
      expect(result.checks).toHaveLength(2);
      expect(result.checks[1].passed).toBe(true);
      expect(result.checks[1].actual).toBe('true');
      expect(mock.calls.some((c) => c.name === 'inspect_instance')).toBe(true);
    });

    it('records FAILED verdict when live inspect_instance property mismatches expectation', async () => {
      const mock = createMockPlaytestCaller({
        toolHandler: (name, args) => {
          if (name === 'inspect_instance' && args.path === 'Workspace.Door') {
            return JSON.stringify({
              path: 'Workspace.Door',
              className: 'Part',
              properties: { Open: 'false' },
              attributes: {},
              children: [],
            });
          }
          if (name === 'start_stop_play') {
            return JSON.stringify({ success: true, mode: args.is_start ? 'Play' : 'Stop' });
          }
          if (name === 'screen_capture') {
            return JSON.stringify({ base64: VALID_PNG_BASE64 });
          }
          return JSON.stringify({ success: true });
        },
      });

      const def = parseAcceptanceDefinition(
        JSON.stringify({
          version: 1,
          checks: [
            { id: 's1', scenario: 'Mismatched State Scenario', type: 'state', instancePath: 'Workspace.Door', property: 'Open', expected: 'true' },
          ],
          screenshots: ['shots/mismatch_state.png'],
        }),
      );

      const result = await runPlaytestScenario(
        'Mismatched State Scenario',
        {
          maxDurationSeconds: 5,
          spawnDelayMs: 0,
          projectRoot: tempDir,
          acceptanceDef: def,
          caller: mock.caller,
        },
      );

      expect(result.verdict).toBe('FAILED');
      expect(result.checks[0].passed).toBe(false);
      expect(result.checks[0].actual).toBe('false');
    });

    it('records unobserved actual and FAILED verdict when assertion is missing from scenarioActuals', async () => {
      const mock = createMockPlaytestCaller();
      const def = parseAcceptanceDefinition(
        JSON.stringify({
          version: 1,
          checks: [
            { id: 'c1', scenario: 'Missing Assertion Scenario', type: 'assertion', description: 'quest completed', expected: 'true' },
          ],
          screenshots: ['shots/missing_assertion.png'],
        }),
      );

      const result = await runPlaytestScenario(
        'Missing Assertion Scenario',
        {
          maxDurationSeconds: 5,
          spawnDelayMs: 0,
          projectRoot: tempDir,
          acceptanceDef: def,
          caller: mock.caller,
          scenarioActuals: { assertions: {} },
        },
      );

      expect(result.verdict).toBe('FAILED');
      expect(result.checks[0].passed).toBe(false);
      expect(result.checks[0].actual).toBeUndefined();
    });
  });

  // ==========================================
  // Suite 3: Timeout & Deadlock
  // ==========================================
  describe('Suite 3: Timeout & Deadlock', () => {
    it('enforces maxDurationSeconds timeout and aborts hanging action', async () => {
      const mock = createMockPlaytestCaller({
        hangOnTool: 'character_navigation',
      });

      const startTime = Date.now();
      const result = await runPlaytestScenario(
        'Hanging Action Scenario',
        {
          maxDurationSeconds: 1,
          projectRoot: tempDir,
          caller: mock.caller,
          actions: [
            { type: 'navigate', params: { x: 50, y: 10, z: 50 } },
          ],
          customChecks: [{ name: 'check', expected: 'true', actual: 'true' }],
        },
      );
      const elapsed = Date.now() - startTime;

      expect(result.verdict).toBe('ERROR');
      expect(result.error).toMatch(/timeout limit/);
      expect(elapsed).toBeLessThan(4000); // Should abort around 1s, well before 4s

      // Fail-safe cleanup executed Stop
      const stopCalls = mock.calls.filter((c) => c.name === 'start_stop_play' && c.args.is_start === false);
      expect(stopCalls).toHaveLength(1);
    });

    it('handles hanging stop cleanup gracefully without deadlocking process', async () => {
      const mock = createMockPlaytestCaller({
        hangOnStop: true,
      });

      const result = await runPlaytestScenario(
        'Hanging Stop Scenario',
        {
          maxDurationSeconds: 2,
          stopTimeoutMs: 100, // Bound stop timeout to 100ms for fast test execution
          projectRoot: tempDir,
          caller: mock.caller,
          customChecks: [{ name: 'probe', expected: 'ok', actual: 'ok' }],
        },
      );

      expect(result.verdict).toBe('PASSED');
      expect(getActivePlaytestSession()).toBeNull();
    });
  });

  // ==========================================
  // Suite 4: MCP Disconnection & Crashes
  // ==========================================
  describe('Suite 4: MCP Disconnection & Crashes', () => {
    it('refuses execution immediately when MCP caller is not running', async () => {
      const mock = createMockPlaytestCaller({ running: false });

      const result = await runPlaytestScenario(
        'Disconnected MCP Scenario',
        {
          maxDurationSeconds: 5,
          caller: mock.caller,
          customChecks: [{ name: 'check', expected: 'true', actual: 'true' }],
        },
      );

      expect(result.verdict).toBe('ERROR');
      expect(result.error).toContain('Studio MCP server is disconnected');
      expect(mock.calls).toHaveLength(0);
    });

    it('handles MCP crash during scenario without throwing unhandled error', async () => {
      const mock = createMockPlaytestCaller({
        crashOnTool: 'character_navigation',
      });

      const result = await runPlaytestScenario(
        'Crash Scenario',
        {
          maxDurationSeconds: 5,
          caller: mock.caller,
          actions: [
            { type: 'navigate', params: { x: 0, y: 0, z: 0 } },
          ],
          customChecks: [{ name: 'check', expected: 'true', actual: 'true' }],
        },
      );

      expect(result.verdict).toBe('ERROR');
      expect(result.error).toContain('MCP process crashed during character_navigation');
      expect(mock.caller.running).toBe(false);

      // Verify no further calls made to dead process
      const callsAfterCrash = mock.calls.filter((c) => c.name === 'start_stop_play' && c.args.is_start === false);
      expect(callsAfterCrash).toHaveLength(0);
    });

    it('handles fatal MCP crash during screen_capture and fails closed with ERROR verdict', async () => {
      const mock = createMockPlaytestCaller({
        crashOnTool: 'screen_capture',
      });

      const result = await runPlaytestScenario(
        'Crash During Screen Capture Scenario',
        {
          maxDurationSeconds: 5,
          spawnDelayMs: 0,
          projectRoot: tempDir,
          captureScreenshots: true,
          caller: mock.caller,
          customChecks: [{ name: 'probe', expected: '1', actual: '1' }],
        },
      );

      // Must fail closed with ERROR verdict, never PASSED
      expect(result.verdict).toBe('ERROR');
      expect(result.error).toMatch(/screen_capture|disconnected|crash|screenshot/i);
      expect(mock.caller.running).toBe(false);
      expect(getActivePlaytestSession()).toBeNull();

      // No fabricated screenshot evidence should be recorded
      expect(result.screenshotPaths).toHaveLength(0);

      // Dead caller must not receive subsequent calls
      const stopCalls = mock.calls.filter((c) => c.name === 'start_stop_play' && c.args.is_start === false);
      expect(stopCalls).toHaveLength(0);
    });

    it('rejects captureStudioScreenshot when MCP caller crashes or is disconnected', async () => {
      const mock = createMockPlaytestCaller({
        crashOnTool: 'screen_capture',
      });

      await expect(
        captureStudioScreenshot(mock.caller, 'Direct Capture Crash', tempDir),
      ).rejects.toThrow(/screen_capture|disconnected|crash/i);
    });

    it('handles fatal MCP crash during get_console_output and fails closed with ERROR verdict', async () => {
      const mock = createMockPlaytestCaller({
        crashOnTool: 'get_console_output',
      });

      const result = await runPlaytestScenario(
        'Crash During Console Output Scenario',
        {
          maxDurationSeconds: 5,
          spawnDelayMs: 0,
          projectRoot: tempDir,
          captureScreenshots: false,
          caller: mock.caller,
          customChecks: [{ name: 'probe', expected: '1', actual: '1' }],
        },
      );

      expect(result.verdict).toBe('ERROR');
      expect(result.error).toMatch(/get_console_output|console|disconnected|crash/i);
      expect(mock.caller.running).toBe(false);
      expect(getActivePlaytestSession()).toBeNull();

      const stopCalls = mock.calls.filter((c) => c.name === 'start_stop_play' && c.args.is_start === false);
      expect(stopCalls).toHaveLength(0);
    });

    it('handles asynchronous MCP disconnect mid-scenario during wait and fails closed with ERROR verdict', async () => {
      const mock = createMockPlaytestCaller();

      // Simulate unexpected process termination / pipe break during wait interval
      const disconnectTimer = setTimeout(() => {
        mock.setRunning(false);
      }, 50);

      try {
        const result = await runPlaytestScenario(
          'Async Disconnect During Wait Scenario',
          {
            maxDurationSeconds: 5,
            spawnDelayMs: 0,
            projectRoot: tempDir,
            captureScreenshots: false,
            caller: mock.caller,
            actions: [
              { type: 'wait', params: { durationMs: 150 } },
            ],
            customChecks: [{ name: 'probe', expected: '1', actual: '1' }],
          },
        );

        expect(result.verdict).toBe('ERROR');
        expect(result.error).toMatch(/disconnected|crash|not running/i);
        expect(mock.caller.running).toBe(false);
        expect(getActivePlaytestSession()).toBeNull();
      } finally {
        clearTimeout(disconnectTimer);
      }
    });

    it('allows a subsequent playtest with healthy caller after an MCP crash', async () => {
      const crashedMock = createMockPlaytestCaller({ crashOnTool: 'character_navigation' });
      const crashResult = await runPlaytestScenario('Crash Run', {
        maxDurationSeconds: 5,
        spawnDelayMs: 0,
        caller: crashedMock.caller,
        actions: [{ type: 'navigate', params: { x: 0, y: 0, z: 0 } }],
        customChecks: [{ name: 'c1', expected: '1', actual: '1' }],
      });
      expect(crashResult.verdict).toBe('ERROR');
      expect(getActivePlaytestSession()).toBeNull();

      const healthyMock = createMockPlaytestCaller();
      const healthyResult = await runPlaytestScenario('Healthy Run After Crash', {
        maxDurationSeconds: 5,
        spawnDelayMs: 0,
        caller: healthyMock.caller,
        customChecks: [{ name: 'c2', expected: 'ok', actual: 'ok' }],
      });
      expect(healthyResult.verdict).toBe('PASSED');
      expect(getActivePlaytestSession()).toBeNull();
    });
  });

  // ==========================================
  // Suite 5: Screenshot Security
  // ==========================================
  describe('Suite 5: Screenshot Security', () => {
    it('sanitizes malicious capture identifiers and prevents directory traversal', () => {
      expect(sanitizeScreenshotSlug('../../etc/passwd')).toBe('etc_passwd');
      expect(sanitizeScreenshotSlug('..\\..\\windows\\system32')).toBe('windows_system32');
      expect(sanitizeScreenshotSlug('CON')).toBe('safe_CON');
      expect(sanitizeScreenshotSlug('PRN')).toBe('safe_PRN');
      expect(sanitizeScreenshotSlug('NUL')).toBe('safe_NUL');
      expect(sanitizeScreenshotSlug('valid_scenario-123')).toBe('valid_scenario-123');
    });

    it('creates capture directory recursively if missing', async () => {
      const mock = createMockPlaytestCaller();
      const nestedCaptureDir = path.join(tempDir, 'deeply', 'nested', 'capture', 'dir');

      const shotPath = await captureStudioScreenshot(
        mock.caller,
        'Deep Nested Scenario',
        nestedCaptureDir,
        'studio-1',
      );

      const stat = await fs.stat(shotPath);
      expect(stat.isFile()).toBe(true);
      expect(verifyContainedPath(nestedCaptureDir, shotPath)).toBe(path.resolve(shotPath));
    });

    it('rejects screenshot paths attempting to escape target directory', () => {
      expect(() => verifyContainedPath(tempDir, path.join(tempDir, '..', 'escaped.png'))).toThrow(
        /escapes target directory/,
      );
    });

    it('rejects targetRelPath matching Windows reserved device names (CON.png, NUL, etc.)', async () => {
      const mock = createMockPlaytestCaller();
      const reservedPaths = [
        'CON.png',
        'con.png',
        'NUL',
        'nul.png',
        'PRN.png',
        'AUX.png',
        'COM1.png',
        'com2.png',
        'LPT1.png',
        'lpt9.png',
        'shots/CON.png',
        'shots\\NUL.png',
        'aux/frame.png',
        'shots/com1/output.png',
      ];

      for (const relPath of reservedPaths) {
        await expect(
          captureStudioScreenshot(
            mock.caller,
            'Reserved Test Scenario',
            tempDir,
            'studio-1',
            relPath,
            tempDir,
          ),
        ).rejects.toThrow(/reserved/i);
      }
    });

    it('fails closed with ERROR verdict when scenario acceptance defines Windows reserved screenshot path', async () => {
      const mock = createMockPlaytestCaller();
      const def = parseAcceptanceDefinition(
        JSON.stringify({
          version: 1,
          checks: [
            { id: 'c1', scenario: 'Reserved Acceptance Scenario', type: 'assertion', description: 'quest completed', expected: 'true' },
          ],
          screenshots: ['shots/CON.png'],
        }),
      );

      const result = await runPlaytestScenario(
        'Reserved Acceptance Scenario',
        {
          maxDurationSeconds: 5,
          spawnDelayMs: 0,
          projectRoot: tempDir,
          acceptanceDef: def,
          caller: mock.caller,
          scenarioActuals: { assertions: { c1: 'true' } },
        },
      );

      expect(result.verdict).toBe('ERROR');
      expect(result.error).toMatch(/reserved/i);
      expect(getActivePlaytestSession()).toBeNull();
    });
  });

  // ==========================================
  // Suite 6: Concurrency & Re-entrancy
  // ==========================================
  describe('Suite 6: Concurrency & Re-entrancy', () => {
    it('rejects concurrent playtest runs with busy error', async () => {
      const mock = createMockPlaytestCaller();

      // Launch first run with delay action
      const firstRunPromise = runPlaytestScenario(
        'First Concurrent Run',
        {
          maxDurationSeconds: 5,
          caller: mock.caller,
          actions: [{ type: 'wait', params: { durationMs: 600 } }],
          customChecks: [{ name: 'first', expected: 'ok', actual: 'ok' }],
        },
      );

      // Small tick to ensure first run has started and acquired mutex
      await new Promise((r) => setTimeout(r, 50));

      // Attempt second run while first is active
      const secondRun = await runPlaytestScenario(
        'Second Concurrent Run',
        {
          maxDurationSeconds: 5,
          caller: mock.caller,
          customChecks: [{ name: 'second', expected: 'ok', actual: 'ok' }],
        },
      );

      expect(secondRun.verdict).toBe('ERROR');
      expect(secondRun.error).toContain('A playtest is already in progress.');

      const firstRun = await firstRunPromise;
      expect(firstRun.verdict).toBe('PASSED');
    });

    it('allows subsequent playtest run after previous run completes', async () => {
      const mock = createMockPlaytestCaller();

      const run1 = await runPlaytestScenario(
        'Sequential Run 1',
        {
          maxDurationSeconds: 5,
          caller: mock.caller,
          customChecks: [{ name: 'run1', expected: '1', actual: '1' }],
        },
      );
      expect(run1.verdict).toBe('PASSED');
      expect(getActivePlaytestSession()).toBeNull();

      const run2 = await runPlaytestScenario(
        'Sequential Run 2',
        {
          maxDurationSeconds: 5,
          caller: mock.caller,
          customChecks: [{ name: 'run2', expected: '2', actual: '2' }],
        },
      );
      expect(run2.verdict).toBe('PASSED');
      expect(getActivePlaytestSession()).toBeNull();
    });
  });

  // ==========================================
  // Suite 7: Standalone Studio Avatar Interaction Tools
  // ==========================================
  describe('Suite 7: Standalone Studio Avatar Interaction Tools', () => {
    it('executes studio_navigate with coordinate and instance targets', async () => {
      const mock = createMockPlaytestCaller();

      const coordRes = await studio_navigate(mock.caller, { x: 10, y: 5, z: -20, speed_multiplier: 2.0, studioId: 'S1' });
      expect(coordRes.success).toBe(true);

      const instRes = await studio_navigate({ instance_path: 'Workspace.Goal', studioId: 'S1' }, mock.caller);
      expect(instRes.success).toBe(true);

      const navCalls = mock.calls.filter((c) => c.name === 'character_navigation');
      expect(navCalls).toHaveLength(2);
      expect(navCalls[0].args).toMatchObject({ datamodel_type: 'Client', x: 10, y: 5, z: -20, speed_multiplier: 2.0, studio_id: 'S1' });
      expect(navCalls[1].args).toMatchObject({ datamodel_type: 'Client', instance_path: 'Workspace.Goal', studio_id: 'S1' });
    });

    it('executes studio_keyboard and studio_mouse inputs', async () => {
      const mock = createMockPlaytestCaller();

      const kbRes = await studio_keyboard(mock.caller, {
        actions: [{ action: 'textInput', text: 'hello' }],
        studioId: 'S1',
      });
      expect(kbRes.success).toBe(true);
      const kbCall = mock.calls.find((c) => c.name === 'user_keyboard_input');
      expect(kbCall?.args.actions).toEqual([{ action: 'textInput', text: 'hello', text_inputs: 'hello' }]);

      const mouseRes = await studio_mouse(mock.caller, {
        actions: [{ action: 'moveTo', x: 50, y: 100 }],
        studioId: 'S1',
      });
      expect(mouseRes.success).toBe(true);
      const mouseCall = mock.calls.find((c) => c.name === 'user_mouse_input');
      expect(mouseCall?.args.actions).toEqual([{ action: 'moveTo', x: 50, y: 100 }]);
    });

    it('executes studio_playtest, studio_console, and studio_capture', async () => {
      const mock = createMockPlaytestCaller();

      const playRes = await studio_playtest(mock.caller, { mode: 'Play', studioId: 'S1' });
      expect(playRes.success).toBe(true);
      expect(mock.calls.find((c) => c.name === 'start_stop_play')?.args.is_start).toBe(true);

      const stopRes = await studio_playtest(mock.caller, { mode: 'Stop', studioId: 'S1' });
      expect(stopRes.success).toBe(true);
      expect(mock.calls.filter((c) => c.name === 'start_stop_play')[1]?.args.is_start).toBe(false);

      const consoleRes = await studio_console(mock.caller, { studioId: 'S1' });
      expect(consoleRes.logs).toHaveLength(2);
      expect(consoleRes.rawText).toContain('[Server] Systems initialized');

      const captureRes = await studio_capture(mock.caller, { captureId: 'test-cap', studioId: 'S1' });
      expect(captureRes.bytes).toBeGreaterThan(0);
      expect(captureRes.mimeType).toBe('image/png');
    });
  });
});
