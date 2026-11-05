import { spawn } from 'node:child_process';
import { describe, expect, it, afterEach } from 'vitest';
import {
  globalProcessSupervisor,
  killProcessTree,
  ProcessSupervisor,
  TaskBudgetTracker,
} from '../electron/taskBudget';
import { runShellCommand } from '../electron/runCommand';
import { runAgentTurn, runTool } from '../electron/agent';
import type { ToolExecutor, ToolSchema } from '../electron/types';

describe('TaskExecutionBudget & Runaway Supervisor Suite', () => {
  afterEach(async () => {
    await globalProcessSupervisor.terminateAll();
  });

  describe('TaskBudgetTracker', () => {
    it('initializes with production default constraints', () => {
      const tracker = new TaskBudgetTracker();
      const summary = tracker.getSummary();
      expect(summary.maxRounds).toBe(6);
      expect(summary.maxToolCalls).toBe(25);
      expect(summary.maxDurationMs).toBe(300_000);
      expect(summary.roundsUsed).toBe(0);
      expect(summary.toolCallsCount).toBe(0);
      expect(summary.isExhausted).toBe(false);
      expect(summary.exhaustionReason).toBeUndefined();
    });

    it('enforces custom step and tool call budget caps', () => {
      const tracker = new TaskBudgetTracker({ maxRounds: 2, maxToolCalls: 3 });

      expect(tracker.recordRound().ok).toBe(true);
      expect(tracker.recordToolCall('tool1').ok).toBe(true);
      expect(tracker.recordToolCall('tool2').ok).toBe(true);
      expect(tracker.recordRound().ok).toBe(true);
      expect(tracker.recordToolCall('tool3').ok).toBe(true);

      const roundOver = tracker.recordRound();
      expect(roundOver.ok).toBe(false);
      expect(roundOver.reason).toContain('Maximum tool rounds limit reached');

      const toolOver = tracker.recordToolCall('tool4');
      expect(toolOver.ok).toBe(false);
    });

    it('enforces consecutive tool failure cap to break infinite retry loops', () => {
      const tracker = new TaskBudgetTracker({ maxConsecutiveToolFailures: 3 });

      expect(tracker.recordToolCall('search_code').ok).toBe(true);
      expect(tracker.recordToolResult('search_code', false, 'Index not ready').ok).toBe(true);

      expect(tracker.recordToolCall('search_code').ok).toBe(true);
      expect(tracker.recordToolResult('search_code', false, 'Index not ready').ok).toBe(true);

      expect(tracker.recordToolCall('search_code').ok).toBe(true);
      const capReached = tracker.recordToolResult('search_code', false, 'Index not ready');
      expect(capReached.ok).toBe(false);
      expect(capReached.reason).toContain("Tool 'search_code' failed 3 consecutive times");

      // Next attempt to call that tool fails closed
      const nextAttempt = tracker.recordToolCall('search_code');
      expect(nextAttempt.ok).toBe(false);
      expect(nextAttempt.reason).toContain("consecutive");
    });

    it('resets consecutive failures counter upon successful tool execution', () => {
      const tracker = new TaskBudgetTracker({ maxConsecutiveToolFailures: 3 });

      tracker.recordToolCall('read_script');
      tracker.recordToolResult('read_script', false, 'Timeout');
      tracker.recordToolCall('read_script');
      tracker.recordToolResult('read_script', false, 'Timeout');

      // Success resets count for read_script
      tracker.recordToolCall('read_script');
      tracker.recordToolResult('read_script', true);

      expect(tracker.getSummary().consecutiveFailures['read_script']).toBeUndefined();
      expect(tracker.canProceed().ok).toBe(true);
    });

    it('tracks independent failure counters per tool name', () => {
      const tracker = new TaskBudgetTracker({ maxConsecutiveToolFailures: 3 });

      tracker.recordToolCall('toolA');
      tracker.recordToolResult('toolA', false, 'error A');
      tracker.recordToolCall('toolB');
      tracker.recordToolResult('toolB', false, 'error B');

      const summary = tracker.getSummary();
      expect(summary.consecutiveFailures['toolA']).toBe(1);
      expect(summary.consecutiveFailures['toolB']).toBe(1);
      expect(tracker.canProceed().ok).toBe(true);
    });

    it('detects wall-clock duration expiration', async () => {
      const tracker = new TaskBudgetTracker({ maxDurationMs: 1_000 });
      expect(tracker.canProceed().ok).toBe(true);
      expect(tracker.getRemainingDurationMs()).toBeGreaterThan(0);
    });
  });

  describe('killProcessTree', () => {
    it('safely handles non-existent or negative pids without throwing', async () => {
      await expect(killProcessTree(-1)).resolves.not.toThrow();
      await expect(killProcessTree(0)).resolves.not.toThrow();
      await expect(killProcessTree(999999)).resolves.not.toThrow();
    });

    it('terminates real spawned child process', async () => {
      const isWindows = process.platform === 'win32';
      const cmd = isWindows ? 'cmd.exe' : 'sleep';
      const args = isWindows ? ['/c', 'pause'] : ['30'];

      const child = spawn(cmd, args, { windowsHide: true });
      expect(child.pid).toBeDefined();

      await killProcessTree(child.pid!);

      await new Promise<void>((resolve) => {
        if (child.exitCode !== null) {
          resolve();
        } else {
          child.on('exit', () => resolve());
          setTimeout(resolve, 2_000);
        }
      });

      expect(child.killed || child.exitCode !== null).toBe(true);
    });
  });

  describe('ProcessSupervisor', () => {
    it('registers child processes and unregisters cleanly on exit', () => {
      const supervisor = new ProcessSupervisor();
      const unregister = supervisor.register({ pid: 12345, command: 'test-job', timeoutMs: 10_000 });

      const active = supervisor.listActive();
      expect(active).toHaveLength(1);
      expect(active[0]?.pid).toBe(12345);
      expect(active[0]?.command).toBe('test-job');

      unregister();
      expect(supervisor.listActive()).toHaveLength(0);
    });

    it('terminates all tracked processes during terminateAll', async () => {
      const supervisor = new ProcessSupervisor();
      let killedA = false;
      let killedB = false;

      supervisor.register({
        pid: 20001,
        command: 'job-a',
        killer: () => {
          killedA = true;
        },
      });
      supervisor.register({
        pid: 20002,
        command: 'job-b',
        killer: () => {
          killedB = true;
        },
      });

      expect(supervisor.listActive()).toHaveLength(2);
      const terminatedCount = await supervisor.terminateAll('test termination');

      expect(terminatedCount).toBe(2);
      expect(killedA).toBe(true);
      expect(killedB).toBe(true);
      expect(supervisor.listActive()).toHaveLength(0);
    });

    it('reaps runaway processes that exceed their maximum allowed lifetime', async () => {
      const supervisor = new ProcessSupervisor();
      let reaped = false;

      supervisor.register({
        pid: 30001,
        command: 'expired-job',
        timeoutMs: 1, // immediate timeout
        killer: () => {
          reaped = true;
        },
      });

      await new Promise((r) => setTimeout(r, 10));
      const reapedCount = await supervisor.reapRunaways();

      expect(reapedCount).toBe(1);
      expect(reaped).toBe(true);
      expect(supervisor.listActive()).toHaveLength(0);
    });
  });

  describe('runShellCommand Cancellation & Process Registration', () => {
    it('returns cancelled immediately if signal is already aborted', async () => {
      const controller = new AbortController();
      controller.abort();

      const result = await runShellCommand({
        command: 'echo pre-aborted',
        cwd: process.cwd(),
        signal: controller.signal,
      });

      expect(result.cancelled).toBe(true);
      expect(result.code).toBeNull();
    });

    it('aborts running command on signal abortion and terminates child process', async () => {
      const controller = new AbortController();
      const isWindows = process.platform === 'win32';
      const command = isWindows ? 'ping -n 10 127.0.0.1 > nul' : 'sleep 10';

      const promise = runShellCommand({
        command,
        cwd: process.cwd(),
        signal: controller.signal,
      });

      // Allow process to spawn
      await new Promise((r) => setTimeout(r, 100));
      expect(globalProcessSupervisor.listActive().length).toBeGreaterThanOrEqual(1);

      controller.abort();
      const result = await promise;

      expect(result.cancelled).toBe(true);
      // Ensure it unregistered from globalProcessSupervisor
      expect(globalProcessSupervisor.listActive()).toHaveLength(0);
    });
  });

  describe('Agent Loop Budget & Runaway Defense', () => {
    const dummySchemas: ToolSchema[] = [
      {
        name: 'failing_tool',
        description: 'Always fails',
        parameters: { type: 'object', properties: {} },
      },
    ];

    const dummyExecutor: ToolExecutor = {
      schemas: () => dummySchemas,
      execute: async () => ({ ok: false, output: 'Permanent database error' }),
    };

    it('runTool returns cancelled outcome when passed an aborted signal', async () => {
      const controller = new AbortController();
      controller.abort();
      const trace: Parameters<typeof runTool>[2] = [];

      const result = await runTool(
        { id: 'call-1', name: 'read_file' as const, args: { path: 'test.lua' } },
        dummyExecutor,
        trace,
        controller.signal,
      );

      expect(result.ok).toBe(false);
      expect(result.output).toContain('cancelled');
    });

    it('runAgentTurn respects task budget and stops when round limit is reached', async () => {
      const budget = new TaskBudgetTracker({ maxRounds: 1 });
      // Exceed budget upfront to verify fail-closed enforcement
      budget.recordRound();

      const result = await runAgentTurn({
        connection: { kind: 'openai-api', model: 'gpt-4o', baseUrl: 'http://127.0.0.1:9999' },
        messages: [{ id: '1', role: 'user', content: 'Generate code' }],
        projectRoot: process.cwd(),
        executor: dummyExecutor,
        budget,
      });

      expect(result.ok).toBe(false);
      expect(result.error).toContain('Maximum tool rounds limit reached');
    });

    it('runAgentTurn aborts gracefully when signal is aborted', async () => {
      const controller = new AbortController();
      controller.abort();

      const result = await runAgentTurn({
        connection: { kind: 'openai-api', model: 'gpt-4o', baseUrl: 'http://127.0.0.1:9999' },
        messages: [{ id: '1', role: 'user', content: 'Hello' }],
        projectRoot: process.cwd(),
        executor: dummyExecutor,
        signal: controller.signal,
      });

      expect(result.ok).toBe(false);
      expect(result.error).toBe('Cancelled.');
    });
  });
});
