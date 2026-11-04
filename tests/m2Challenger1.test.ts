import { promises as fs } from 'node:fs';
import { tmpdir } from 'node:os';
import * as path from 'node:path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import type { McpCaller } from '../electron/mcpTools';
import {
  getActivePlaytestSession,
  resetPlaytestSession,
  runPlaytestScenario,
  stopPlaytestSafely,
} from '../electron/playtestRunner';
import type { McpToolSummary } from '../electron/types';

const VALID_PNG_BASE64 = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==';

interface MockCallerOptions {
  tools?: string[];
  running?: boolean;
  hangOnTool?: string;
  crashOnTool?: string;
  hangOnStop?: boolean;
  rejectOnStop?: boolean;
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
      if (name === 'start_stop_play' && (args.is_start === false || args.mode === 'Stop')) {
        if (options.hangOnStop) {
          return new Promise(() => {}); // Hang on stop
        }
        if (options.rejectOnStop) {
          throw new Error('Simulated stop RPC connection error');
        }
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
        return JSON.stringify({ properties: { Open: 'true' } });
      }
      return JSON.stringify({ status: 'OK' });
    },
  };

  return { caller, calls, setRunning: (val: boolean) => { isRunning = val; } };
}

describe('Challenger 1 Empirical Stress Tests: Lifecycle, Timeouts & Concurrency', () => {
  let tempDir: string;

  beforeEach(async () => {
    resetPlaytestSession();
    tempDir = await fs.mkdtemp(path.join(tmpdir(), 'openro-m2-challenger1-'));
  });

  afterEach(async () => {
    resetPlaytestSession();
    try {
      await fs.rm(tempDir, { recursive: true, force: true });
    } catch {
      // Ignore cleanup error
    }
  });

  // ==========================================
  // 1. Execution Timeout Handling
  // ==========================================
  describe('1. Execution Timeout Handling', () => {
    it('aborts hanging character_navigation within deadline and executes stop cleanup', async () => {
      const mock = createMockPlaytestCaller({
        hangOnTool: 'character_navigation',
      });

      const startTime = Date.now();
      const result = await runPlaytestScenario('Hanging Nav Scenario', {
        maxDurationSeconds: 1,
        spawnDelayMs: 0,
        projectRoot: tempDir,
        caller: mock.caller,
        actions: [{ type: 'navigate', params: { x: 50, y: 10, z: 50 } }],
        customChecks: [{ name: 'check', expected: 'true', actual: 'true' }],
      });
      const elapsed = Date.now() - startTime;

      expect(result.verdict).toBe('ERROR');
      expect(result.error).toMatch(/timeout limit/i);
      expect(elapsed).toBeLessThan(4000);

      const stopCalls = mock.calls.filter((c) => c.name === 'start_stop_play' && c.args.is_start === false);
      expect(stopCalls).toHaveLength(1);
    });

    it('aborts hanging user_keyboard_input action and executes stop cleanup', async () => {
      const mock = createMockPlaytestCaller({
        hangOnTool: 'user_keyboard_input',
      });

      const startTime = Date.now();
      const result = await runPlaytestScenario('Hanging Key Scenario', {
        maxDurationSeconds: 1,
        spawnDelayMs: 0,
        projectRoot: tempDir,
        caller: mock.caller,
        actions: [{ type: 'keyboard', params: { actions: [{ key: 'W', down: true }] } }],
        customChecks: [{ name: 'check', expected: 'true', actual: 'true' }],
      });
      const elapsed = Date.now() - startTime;

      expect(result.verdict).toBe('ERROR');
      expect(result.error).toMatch(/timeout limit/i);
      expect(elapsed).toBeLessThan(4000);

      const stopCalls = mock.calls.filter((c) => c.name === 'start_stop_play' && c.args.is_start === false);
      expect(stopCalls).toHaveLength(1);
    });

    it('aborts hanging user_mouse_input action and executes stop cleanup', async () => {
      const mock = createMockPlaytestCaller({
        hangOnTool: 'user_mouse_input',
      });

      const startTime = Date.now();
      const result = await runPlaytestScenario('Hanging Mouse Scenario', {
        maxDurationSeconds: 1,
        spawnDelayMs: 0,
        projectRoot: tempDir,
        caller: mock.caller,
        actions: [{ type: 'mouse', params: { actions: [{ type: 'click', button: 'Left' }] } }],
        customChecks: [{ name: 'check', expected: 'true', actual: 'true' }],
      });
      const elapsed = Date.now() - startTime;

      expect(result.verdict).toBe('ERROR');
      expect(result.error).toMatch(/timeout limit/i);
      expect(elapsed).toBeLessThan(4000);

      const stopCalls = mock.calls.filter((c) => c.name === 'start_stop_play' && c.args.is_start === false);
      expect(stopCalls).toHaveLength(1);
    });

    it('aborts hanging start_stop_play(Play) during startup without erroneous Stop call', async () => {
      const mock = createMockPlaytestCaller({
        hangOnTool: 'start_stop_play',
      });

      const startTime = Date.now();
      const result = await runPlaytestScenario('Hanging Startup Scenario', {
        maxDurationSeconds: 1,
        spawnDelayMs: 0,
        projectRoot: tempDir,
        caller: mock.caller,
        customChecks: [{ name: 'check', expected: 'true', actual: 'true' }],
      });
      const elapsed = Date.now() - startTime;

      expect(result.verdict).toBe('ERROR');
      expect(result.error).toMatch(/timeout limit/i);
      expect(elapsed).toBeLessThan(4000);

      // Startup never finished, so stopPlaytestSafely should not be invoked
      const stopCalls = mock.calls.filter((c) => c.name === 'start_stop_play' && c.args.is_start === false);
      expect(stopCalls).toHaveLength(0);
    });
  });

  // ==========================================
  // 2. Hanging Stop Cleanup Handling
  // ==========================================
  describe('2. Hanging Stop Cleanup Handling', () => {
    it('aborts hanging stop cleanup via stopTimeoutMs guard without deadlocking process', async () => {
      const mock = createMockPlaytestCaller({
        hangOnStop: true,
      });

      const result = await runPlaytestScenario('Hanging Stop Scenario', {
        maxDurationSeconds: 2,
        spawnDelayMs: 0,
        stopTimeoutMs: 100,
        projectRoot: tempDir,
        caller: mock.caller,
        customChecks: [{ name: 'probe', expected: 'ok', actual: 'ok' }],
      });

      expect(result.verdict).toBe('PASSED');
      expect(getActivePlaytestSession()).toBeNull();
    });

    it('handles stop RPC rejection gracefully without throwing unhandled rejection', async () => {
      const mock = createMockPlaytestCaller({
        rejectOnStop: true,
      });

      const result = await runPlaytestScenario('Rejecting Stop Scenario', {
        maxDurationSeconds: 2,
        spawnDelayMs: 0,
        projectRoot: tempDir,
        caller: mock.caller,
        customChecks: [{ name: 'probe', expected: 'ok', actual: 'ok' }],
      });

      expect(result.verdict).toBe('PASSED');
      expect(getActivePlaytestSession()).toBeNull();
    });

    it('directly tests stopPlaytestSafely with hanging caller and custom timeoutMs', async () => {
      const mock = createMockPlaytestCaller({
        hangOnStop: true,
      });

      const start = Date.now();
      await stopPlaytestSafely(mock.caller, 'studio-1', 100);
      const duration = Date.now() - start;

      expect(duration).toBeGreaterThanOrEqual(90);
      expect(duration).toBeLessThan(1000);
    });
  });

  // ==========================================
  // 3. MCP Server Crash and Disconnection Handling
  // ==========================================
  describe('3. MCP Server Crash and Disconnection Handling', () => {
    it('handles fatal MCP crash mid-scenario during character_navigation and fails closed', async () => {
      const mock = createMockPlaytestCaller({
        crashOnTool: 'character_navigation',
      });

      const result = await runPlaytestScenario('Crash Scenario', {
        maxDurationSeconds: 5,
        spawnDelayMs: 0,
        projectRoot: tempDir,
        caller: mock.caller,
        actions: [{ type: 'navigate', params: { x: 0, y: 0, z: 0 } }],
        customChecks: [{ name: 'check', expected: 'true', actual: 'true' }],
      });

      expect(result.verdict).toBe('ERROR');
      expect(result.error).toContain('MCP process crashed during character_navigation');
      expect(mock.caller.running).toBe(false);
      expect(getActivePlaytestSession()).toBeNull();

      const callsAfterCrash = mock.calls.filter((c) => c.name === 'start_stop_play' && c.args.is_start === false);
      expect(callsAfterCrash).toHaveLength(0);
    });

    it('handles fatal MCP crash during screen_capture and fails closed', async () => {
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

      // Verifies resolution of Gate 1 failure point 1: MUST fail closed with ERROR verdict
      expect(result.verdict).toBe('ERROR');
      expect(mock.caller.running).toBe(false);
      expect(getActivePlaytestSession()).toBeNull();
      expect(result.screenshotPaths).toHaveLength(0);
    });

    it('handles fatal MCP crash during get_console_output and fails closed', async () => {
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

      // Verifies resolution of Gate 1 failure point 2: MUST fail closed with ERROR verdict
      expect(result.verdict).toBe('ERROR');
      expect(mock.caller.running).toBe(false);
      expect(getActivePlaytestSession()).toBeNull();
    });

    it('handles asynchronous MCP disconnect mid-scenario during wait and fails closed', async () => {
      const mock = createMockPlaytestCaller();

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
            actions: [{ type: 'wait', params: { durationMs: 150 } }],
            customChecks: [{ name: 'probe', expected: '1', actual: '1' }],
          },
        );

        // Verifies resolution of Gate 1 failure point 3: MUST fail closed with ERROR verdict
        expect(result.verdict).toBe('ERROR');
        expect(mock.caller.running).toBe(false);
        expect(getActivePlaytestSession()).toBeNull();
      } finally {
        clearTimeout(disconnectTimer);
      }
    });

    it('allows a subsequent playtest with healthy caller after previous MCP crash', async () => {
      const crashedMock = createMockPlaytestCaller({ crashOnTool: 'character_navigation' });
      const crashResult = await runPlaytestScenario('Crash Run', {
        maxDurationSeconds: 5,
        spawnDelayMs: 0,
        projectRoot: tempDir,
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
        projectRoot: tempDir,
        caller: healthyMock.caller,
        customChecks: [{ name: 'c2', expected: 'ok', actual: 'ok' }],
      });
      expect(healthyResult.verdict).toBe('PASSED');
      expect(getActivePlaytestSession()).toBeNull();
    });
  });

  // ==========================================
  // 4. Concurrency Mutex and Overlap Protection
  // ==========================================
  describe('4. Concurrency Mutex and Overlap Protection', () => {
    it('rejects multiple concurrent calls with busy error while active run proceeds', async () => {
      const mock = createMockPlaytestCaller({
        hangOnTool: 'character_navigation',
      });

      const firstRunPromise = runPlaytestScenario('Primary Playtest', {
        maxDurationSeconds: 1,
        spawnDelayMs: 0,
        projectRoot: tempDir,
        caller: mock.caller,
        actions: [{ type: 'navigate', params: { x: 10, y: 10, z: 10 } }],
        customChecks: [{ name: 'c1', expected: '1', actual: '1' }],
      });

      // Small delay to ensure first run enters execution and acquires mutex
      await new Promise((r) => setTimeout(r, 50));

      const secondRun = await runPlaytestScenario('Overlapping Playtest', {
        maxDurationSeconds: 1,
        spawnDelayMs: 0,
        projectRoot: tempDir,
        caller: mock.caller,
        customChecks: [{ name: 'c2', expected: '1', actual: '1' }],
      });

      expect(secondRun.verdict).toBe('ERROR');
      expect(secondRun.error).toContain('A playtest is already in progress');

      const firstRun = await firstRunPromise;
      expect(firstRun.verdict).toBe('ERROR');
      expect(getActivePlaytestSession()).toBeNull();
    });

    it('releases lock cleanly if scenario encounters validation failure early', async () => {
      const mock = createMockPlaytestCaller();

      const earlyFailRun = await runPlaytestScenario('Invalid Max Duration', {
        maxDurationSeconds: 0,
        caller: mock.caller,
      });

      expect(earlyFailRun.verdict).toBe('ERROR');
      expect(earlyFailRun.error).toContain('maxDurationSeconds must be greater than 0');
      expect(getActivePlaytestSession()).toBeNull();

      const subsequentRun = await runPlaytestScenario('Subsequent Valid Run', {
        maxDurationSeconds: 5,
        spawnDelayMs: 0,
        projectRoot: tempDir,
        caller: mock.caller,
        customChecks: [{ name: 'probe', expected: '1', actual: '1' }],
      });

      expect(subsequentRun.verdict).toBe('PASSED');
      expect(getActivePlaytestSession()).toBeNull();
    });
  });

  // ==========================================
  // 5. Additional Adversarial Stress Tests (Iteration 2)
  // ==========================================
  describe('5. Additional Adversarial Stress Tests (Iteration 2)', () => {
    it('handles asynchronous MCP disconnect during avatar spawnDelayMs and fails closed with ERROR', async () => {
      const mock = createMockPlaytestCaller();

      // Disconnect during the 200ms avatar spawn stabilization window
      const disconnectTimer = setTimeout(() => {
        mock.setRunning(false);
      }, 60);

      try {
        const result = await runPlaytestScenario('Spawn Delay Disconnect Scenario', {
          maxDurationSeconds: 5,
          spawnDelayMs: 200,
          projectRoot: tempDir,
          caller: mock.caller,
          customChecks: [{ name: 'probe', expected: '1', actual: '1' }],
        });

        expect(result.verdict).toBe('ERROR');
        expect(result.error).toMatch(/disconnected/i);
        expect(mock.caller.running).toBe(false);
        expect(getActivePlaytestSession()).toBeNull();
      } finally {
        clearTimeout(disconnectTimer);
      }
    });

    it('strictly guarantees session mutex lock cleanup upon crash so getActivePlaytestSession() is null', async () => {
      const mock = createMockPlaytestCaller({
        crashOnTool: 'character_navigation',
      });

      expect(getActivePlaytestSession()).toBeNull();

      const runPromise = runPlaytestScenario('Crash Mutex Test', {
        maxDurationSeconds: 5,
        spawnDelayMs: 0,
        projectRoot: tempDir,
        caller: mock.caller,
        actions: [{ type: 'navigate', params: { x: 1, y: 1, z: 1 } }],
        customChecks: [{ name: 'c', expected: '1', actual: '1' }],
      });

      const result = await runPromise;
      expect(result.verdict).toBe('ERROR');
      expect(getActivePlaytestSession()).toBeNull();
    });

    it('verifies rapid sequential crashes followed by healthy playtest run without leaked mutex or state', async () => {
      // 1. Crash on screen_capture
      const mock1 = createMockPlaytestCaller({ crashOnTool: 'screen_capture' });
      const res1 = await runPlaytestScenario('Sequential Crash 1', {
        maxDurationSeconds: 5,
        spawnDelayMs: 0,
        projectRoot: tempDir,
        captureScreenshots: true,
        caller: mock1.caller,
        customChecks: [{ name: 'c', expected: '1', actual: '1' }],
      });
      expect(res1.verdict).toBe('ERROR');
      expect(getActivePlaytestSession()).toBeNull();

      // 2. Crash on get_console_output
      const mock2 = createMockPlaytestCaller({ crashOnTool: 'get_console_output' });
      const res2 = await runPlaytestScenario('Sequential Crash 2', {
        maxDurationSeconds: 5,
        spawnDelayMs: 0,
        projectRoot: tempDir,
        captureScreenshots: false,
        caller: mock2.caller,
        customChecks: [{ name: 'c', expected: '1', actual: '1' }],
      });
      expect(res2.verdict).toBe('ERROR');
      expect(getActivePlaytestSession()).toBeNull();

      // 3. Crash on character_navigation
      const mock3 = createMockPlaytestCaller({ crashOnTool: 'character_navigation' });
      const res3 = await runPlaytestScenario('Sequential Crash 3', {
        maxDurationSeconds: 5,
        spawnDelayMs: 0,
        projectRoot: tempDir,
        caller: mock3.caller,
        actions: [{ type: 'navigate', params: { x: 5, y: 5, z: 5 } }],
        customChecks: [{ name: 'c', expected: '1', actual: '1' }],
      });
      expect(res3.verdict).toBe('ERROR');
      expect(getActivePlaytestSession()).toBeNull();

      // 4. Healthy run succeeds with PASSED verdict and clean lock release
      const healthyMock = createMockPlaytestCaller();
      const healthyRes = await runPlaytestScenario('Healthy Recovery Run', {
        maxDurationSeconds: 5,
        spawnDelayMs: 0,
        projectRoot: tempDir,
        captureScreenshots: true,
        caller: healthyMock.caller,
        customChecks: [{ name: 'recovery', expected: 'success', actual: 'success' }],
      });
      expect(healthyRes.verdict).toBe('PASSED');
      expect(healthyRes.screenshotPaths.length).toBeGreaterThan(0);
      expect(getActivePlaytestSession()).toBeNull();
    });

    it('handles pre-evaluation disconnection guard when disconnect occurs right before check grading', async () => {
      let consoleCalled = false;
      const mock = createMockPlaytestCaller({
        toolHandler: (name) => {
          if (name === 'get_console_output') {
            consoleCalled = true;
            // Sever connection immediately after console output tool returns
            setTimeout(() => {
              mock.setRunning(false);
            }, 0);
            return 'Console logs';
          }
          if (name === 'screen_capture') {
            return JSON.stringify({ base64: VALID_PNG_BASE64 });
          }
          if (name === 'start_stop_play') {
            return JSON.stringify({ success: true, mode: 'Play' });
          }
          return JSON.stringify({ status: 'OK' });
        },
      });

      const result = await runPlaytestScenario('Late Disconnect Guard Test', {
        maxDurationSeconds: 5,
        spawnDelayMs: 0,
        projectRoot: tempDir,
        captureScreenshots: false,
        caller: mock.caller,
        customChecks: [{ name: 'probe', expected: '1', actual: '1' }],
      });

      // Allow microtask ticks for the disconnect to register
      if (consoleCalled && !mock.caller.running) {
        expect(result.verdict).toBe('ERROR');
      }
      expect(getActivePlaytestSession()).toBeNull();
    });
  });
});
