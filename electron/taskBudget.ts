import { spawn } from 'node:child_process';

/**
 * Task Execution Budgets & Runaway Process Supervisor (docs/ARCHITECTURE.md §1, §3).
 *
 * Enforces hard step, tool call, time, and retry limits across agent turns and
 * background operations. Tracks active child processes across the system and
 * guarantees complete process-tree termination upon cancellation, timeout,
 * or runaway detection.
 */

export interface TaskBudgetOptions {
  /** Maximum number of reasoning/tool rounds allowed for this task (default 6). */
  maxRounds?: number;
  /** Maximum total tool invocations allowed across all rounds (default 25). */
  maxToolCalls?: number;
  /** Maximum wall-clock execution duration in milliseconds (default 300,000ms = 5m). */
  maxDurationMs?: number;
  /** Maximum consecutive failures for the same tool before halting the loop (default 3). */
  maxConsecutiveToolFailures?: number;
}

export interface TaskBudgetSummary {
  roundsUsed: number;
  maxRounds: number;
  toolCallsCount: number;
  maxToolCalls: number;
  elapsedMs: number;
  maxDurationMs: number;
  consecutiveFailures: Record<string, number>;
  isExhausted: boolean;
  exhaustionReason?: string;
}

export interface ProcessRecord {
  id: string;
  pid: number;
  command: string;
  startedAt: number;
  timeoutMs?: number;
  killer?: () => Promise<void> | void;
}

/** Safely terminate an entire child process tree across platforms. */
export function killProcessTree(pid: number): Promise<void> {
  return new Promise<void>((resolve) => {
    if (!pid || pid <= 0) {
      resolve();
      return;
    }
    if (process.platform === 'win32') {
      try {
        const killer = spawn('taskkill.exe', ['/pid', String(pid), '/t', '/f'], {
          windowsHide: true,
        });
        killer.on('exit', () => resolve());
        killer.on('error', () => {
          try {
            process.kill(pid, 'SIGKILL');
          } catch {
            // Process may already have terminated.
          }
          resolve();
        });
        // Hard fallback if taskkill stalls.
        setTimeout(() => {
          try {
            process.kill(pid, 'SIGKILL');
          } catch {
            // Process may already have terminated.
          }
          resolve();
        }, 1_500);
      } catch {
        try {
          process.kill(pid, 'SIGKILL');
        } catch {
          // Process may already have terminated.
        }
        resolve();
      }
    } else {
      try {
        process.kill(-pid, 'SIGKILL');
      } catch {
        try {
          process.kill(pid, 'SIGKILL');
        } catch {
          // Process may already have terminated.
        }
      }
      resolve();
    }
  });
}

/**
 * Tracks and limits resource consumption for an agent turn or multi-step workflow.
 */
export class TaskBudgetTracker {
  private readonly maxRounds: number;
  private readonly maxToolCalls: number;
  private readonly maxDurationMs: number;
  private readonly maxConsecutiveToolFailures: number;

  private roundsUsed = 0;
  private toolCallsCount = 0;
  private readonly startTime: number;
  private readonly consecutiveFailures = new Map<string, number>();
  private exhausted = false;
  private exhaustionReason?: string;

  constructor(options: TaskBudgetOptions = {}) {
    this.maxRounds = Math.max(1, options.maxRounds ?? 6);
    this.maxToolCalls = Math.max(1, options.maxToolCalls ?? 25);
    this.maxDurationMs = Math.max(1_000, options.maxDurationMs ?? 300_000);
    this.maxConsecutiveToolFailures = Math.max(1, options.maxConsecutiveToolFailures ?? 3);
    this.startTime = Date.now();
  }

  /** Current elapsed wall-clock milliseconds since task start. */
  public getElapsedMs(): number {
    return Math.max(0, Date.now() - this.startTime);
  }

  /** Remaining milliseconds before wall-clock deadline expires. */
  public getRemainingDurationMs(): number {
    return Math.max(0, this.maxDurationMs - this.getElapsedMs());
  }

  /** Check whether the budget has been exceeded without modifying state. */
  public canProceed(): { ok: boolean; reason?: string } {
    if (this.exhausted) {
      return { ok: false, reason: this.exhaustionReason };
    }
    if (this.getElapsedMs() >= this.maxDurationMs) {
      this.exhausted = true;
      this.exhaustionReason = `Task wall-clock timeout exceeded (${this.maxDurationMs}ms).`;
      return { ok: false, reason: this.exhaustionReason };
    }
    return { ok: true };
  }

  /** Record the start of a reasoning/tool round. */
  public recordRound(): { ok: boolean; reason?: string } {
    const check = this.canProceed();
    if (!check.ok) {
      return check;
    }
    if (this.roundsUsed >= this.maxRounds) {
      this.exhausted = true;
      this.exhaustionReason = `Maximum tool rounds limit reached (${this.maxRounds} rounds).`;
      return { ok: false, reason: this.exhaustionReason };
    }
    this.roundsUsed += 1;
    return { ok: true };
  }

  /** Record an intended tool call invocation. */
  public recordToolCall(toolName: string): { ok: boolean; reason?: string } {
    const check = this.canProceed();
    if (!check.ok) {
      return check;
    }
    if (this.toolCallsCount >= this.maxToolCalls) {
      this.exhausted = true;
      this.exhaustionReason = `Maximum tool calls budget reached (${this.maxToolCalls} calls).`;
      return { ok: false, reason: this.exhaustionReason };
    }
    const failures = this.consecutiveFailures.get(toolName) ?? 0;
    if (failures >= this.maxConsecutiveToolFailures) {
      this.exhausted = true;
      this.exhaustionReason = `Tool '${toolName}' reached consecutive failure limit (${this.maxConsecutiveToolFailures} attempts). Aborting runaway retry loop.`;
      return { ok: false, reason: this.exhaustionReason };
    }
    this.toolCallsCount += 1;
    return { ok: true };
  }

  /** Record tool execution outcome to adjust failure counters. */
  public recordToolResult(toolName: string, ok: boolean, error?: string): { ok: boolean; reason?: string } {
    if (ok) {
      this.consecutiveFailures.delete(toolName);
      return { ok: true };
    }
    const current = (this.consecutiveFailures.get(toolName) ?? 0) + 1;
    this.consecutiveFailures.set(toolName, current);
    if (current >= this.maxConsecutiveToolFailures) {
      this.exhausted = true;
      this.exhaustionReason = `Tool '${toolName}' failed ${current} consecutive times${error ? `: ${error}` : '.'} Aborting runaway retry loop.`;
      return { ok: false, reason: this.exhaustionReason };
    }
    return { ok: true };
  }

  /** Summarize the current budget status. */
  public getSummary(): TaskBudgetSummary {
    const failures: Record<string, number> = {};
    for (const [k, v] of this.consecutiveFailures.entries()) {
      failures[k] = v;
    }
    return {
      roundsUsed: this.roundsUsed,
      maxRounds: this.maxRounds,
      toolCallsCount: this.toolCallsCount,
      maxToolCalls: this.maxToolCalls,
      elapsedMs: this.getElapsedMs(),
      maxDurationMs: this.maxDurationMs,
      consecutiveFailures: failures,
      isExhausted: this.exhausted || !this.canProceed().ok,
      exhaustionReason: this.exhaustionReason,
    };
  }
}

/**
 * Global supervisor tracking spawned processes to prevent orphaned and runaway background jobs.
 */
export class ProcessSupervisor {
  private active = new Map<string, ProcessRecord>();
  private nextId = 1;

  /**
   * Register a newly spawned child process.
   * Returns a cleanup callback that must be called when the process exits.
   */
  public register(options: {
    pid: number;
    command: string;
    timeoutMs?: number;
    killer?: () => Promise<void> | void;
  }): () => void {
    const id = `proc-${this.nextId++}-${options.pid}`;
    const record: ProcessRecord = {
      id,
      pid: options.pid,
      command: options.command,
      startedAt: Date.now(),
      timeoutMs: options.timeoutMs,
      killer: options.killer,
    };
    this.active.set(id, record);

    return () => {
      this.active.delete(id);
    };
  }

  /** List currently active tracked child processes. */
  public listActive(): Array<{ id: string; pid: number; command: string; runningMs: number }> {
    const now = Date.now();
    return Array.from(this.active.values()).map((proc) => ({
      id: proc.id,
      pid: proc.pid,
      command: proc.command,
      runningMs: Math.max(0, now - proc.startedAt),
    }));
  }

  /** Terminate a specific process by its OS pid. */
  public async killProcess(pid: number): Promise<boolean> {
    const targets = Array.from(this.active.entries()).filter(([, proc]) => proc.pid === pid);
    if (targets.length === 0) {
      // Not tracked or already exited; still attempt a process tree kill
      await killProcessTree(pid);
      return false;
    }
    for (const [id, proc] of targets) {
      if (proc.killer) {
        try {
          await proc.killer();
        } catch {
          await killProcessTree(proc.pid);
        }
      } else {
        await killProcessTree(proc.pid);
      }
      this.active.delete(id);
    }
    return true;
  }

  /**
   * Terminate all tracked child processes immediately.
   * Used during task cancellation, project close, or app shutdown.
   */
  public async terminateAll(reason?: string): Promise<number> {
    void reason;
    const records = Array.from(this.active.values());
    this.active.clear();

    const terminations = records.map(async (proc) => {
      if (proc.killer) {
        try {
          await proc.killer();
        } catch {
          await killProcessTree(proc.pid);
        }
      } else {
        await killProcessTree(proc.pid);
      }
    });

    await Promise.all(terminations);
    return records.length;
  }

  /**
   * Scan and terminate any process that has exceeded its allowed lifetime.
   */
  public async reapRunaways(maxAgeMs = 300_000): Promise<number> {
    const now = Date.now();
    const toReap: ProcessRecord[] = [];

    for (const [, proc] of this.active.entries()) {
      const allowedAge = proc.timeoutMs ?? maxAgeMs;
      if (now - proc.startedAt > allowedAge) {
        toReap.push(proc);
      }
    }

    for (const proc of toReap) {
      await this.killProcess(proc.pid);
    }
    return toReap.length;
  }

  /** Clear all tracked state (for testing purposes). */
  public clear(): void {
    this.active.clear();
  }
}

/** Global process supervisor instance. */
export const globalProcessSupervisor = new ProcessSupervisor();
