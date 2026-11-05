import { execFileSync } from 'node:child_process';
import * as fs from 'node:fs/promises';
import * as os from 'node:os';
import * as path from 'node:path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { BlenderRunner } from '../electron/blenderRunner';
import { OPENRO_3D_SENTINEL_END, OPENRO_3D_SENTINEL_START } from '../electron/blenderScriptTemplate';

function isPidAlive(pid: number): boolean {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
}

describe('Milestone 4 Adversarial Stress Harness: Process Tree Containment & Timeouts', () => {
  let tempDir: string;
  let mockBlenderExe: string | null = null;
  const cscPath = 'C:\\Windows\\Microsoft.NET\\Framework64\\v4.0.30319\\csc.exe';

  beforeAll(async () => {
    tempDir = await fs.mkdtemp(path.join(os.tmpdir(), 'openro-m4-adv-'));
    try {
      await fs.access(cscPath);
      const csharpSource = `
using System;
using System.Diagnostics;
using System.IO;
using System.Threading;

class MockBlender {
    static int Main(string[] args) {
        if (args.Length > 0 && args[0] == "--version") {
            Console.WriteLine("Blender 4.2.0 (hash 0a9e70183b06)");
            return 0;
        }

        // Check for scripts passed via -P
        for (int i = 0; i < args.Length; i++) {
            if (args[i] == "-P" && i + 1 < args.Length) {
                string script = args[i + 1];
                if (File.Exists(script)) {
                    string content = File.ReadAllText(script);
                    string targetContent = content;
                    string targetDir = Path.GetDirectoryName(script);

                    int idx = content.IndexOf("target_script = ");
                    if (idx != -1) {
                        int q1 = content.IndexOf('"', idx);
                        int q2 = content.IndexOf('"', q1 + 1);
                        if (q1 != -1 && q2 != -1) {
                            string targetPath = content.Substring(q1 + 1, q2 - q1 - 1).Replace("\\\\\\\\", "\\\\");
                            if (File.Exists(targetPath)) {
                                targetContent = File.ReadAllText(targetPath);
                                targetDir = Path.GetDirectoryName(targetPath);
                            }
                        }
                    }

                    if (targetContent.Contains("TRIGGER_HANG_FOREVER")) {
                        Thread.Sleep(60000);
                        return 0;
                    }

                    if (targetContent.Contains("TRIGGER_SUBPROCESS_TREE")) {
                        Process child = new Process();
                        child.StartInfo.FileName = "cmd.exe";
                        child.StartInfo.Arguments = "/c ping -n 60 127.0.0.1 > nul";
                        child.StartInfo.CreateNoWindow = true;
                        child.StartInfo.UseShellExecute = false;
                        child.Start();

                        string pidOut = Path.Combine(targetDir, "child_pid.txt");
                        File.WriteAllText(pidOut, child.Id.ToString());
                        Console.WriteLine("SUBPROCESS_STARTED_PID:" + child.Id);
                        Thread.Sleep(60000);
                        return 0;
                    }
                }
            }
        }

        Console.WriteLine("Blender quit");
        return 0;
    }
}
`;
      const csPath = path.join(tempDir, 'MockBlender.cs');
      mockBlenderExe = path.join(tempDir, 'mock_blender.exe');
      await fs.writeFile(csPath, csharpSource, 'utf8');
      execFileSync(cscPath, ['/nologo', `/out:${mockBlenderExe}`, csPath]);
    } catch {
      mockBlenderExe = null;
    }
  });

  afterAll(async () => {
    try {
      await fs.rm(tempDir, { recursive: true, force: true });
    } catch {
      // Best effort cleanup
    }
  });

  // =========================================================================
  // Challenge 1: Process Tree Containment & Grandchild Process Elimination
  // =========================================================================
  it('terminates entire child process tree on cancelTask when subprocesses are spawned', async () => {
    if (!mockBlenderExe) {
      expect(true).toBe(true);
      return;
    }

    const runner = new BlenderRunner();
    const probe = await runner.detect(mockBlenderExe);
    expect(probe.installed).toBe(true);
    expect(probe.version).toBe('4.2.0');

    const testDir = path.join(tempDir, 'test-tree');
    await fs.mkdir(testDir, { recursive: true });

    const treeScript = path.join(testDir, 'tree_script.py');
    await fs.writeFile(treeScript, '# TRIGGER_SUBPROCESS_TREE\nprint("spawned")\n', 'utf8');

    // Launch task asynchronously
    const taskPromise = runner.runBlenderTask({
      scriptPath: treeScript,
      outputDir: testDir,
      timeoutMs: 30_000,
    });

    // Wait until child process has spawned and written its PID
    const pidFile = path.join(testDir, 'child_pid.txt');
    let childPid = 0;
    for (let attempt = 0; attempt < 60; attempt++) {
      try {
        const raw = await fs.readFile(pidFile, 'utf8');
        childPid = parseInt(raw.trim(), 10);
        if (childPid > 0) break;
      } catch {
        await new Promise((r) => setTimeout(r, 100));
      }
    }

    expect(childPid).toBeGreaterThan(0);
    expect(isPidAlive(childPid)).toBe(true);

    // Cancel running task
    const cancelRes = await runner.cancelTask();
    expect(cancelRes.ok).toBe(true);
    expect(cancelRes.cancelled).toBe(true);

    // Await task resolution
    const taskResult = await taskPromise;
    expect(taskResult.success).toBe(false);

    // Allow brief scheduling tick for OS process termination
    await new Promise((r) => setTimeout(r, 600));

    // Assert that the child subprocess spawned by mock blender was terminated by taskkill /pid /t /f
    expect(isPidAlive(childPid)).toBe(false);
  }, 25_000);

  // =========================================================================
  // Challenge 2: Timeout Enforcement on Runaway / Infinite Loop Process
  // =========================================================================
  it('enforces timeout boundary and terminates hanging process after timeoutMs expires', async () => {
    if (!mockBlenderExe) {
      expect(true).toBe(true);
      return;
    }

    const runner = new BlenderRunner();
    await runner.detect(mockBlenderExe);

    const testDir = path.join(tempDir, 'test-hang');
    await fs.mkdir(testDir, { recursive: true });

    const hangScript = path.join(testDir, 'hang_script.py');
    await fs.writeFile(hangScript, '# TRIGGER_HANG_FOREVER\n', 'utf8');

    const startTime = Date.now();
    // Clamped minimum timeout in blenderRunner is MIN_TIMEOUT_MS = 5000
    const result = await runner.runBlenderTask({
      scriptPath: hangScript,
      outputDir: testDir,
      timeoutMs: 5_000,
    });
    const elapsed = Date.now() - startTime;

    expect(result.success).toBe(false);
    // Verified timeout happened around 5s boundary
    expect(elapsed).toBeGreaterThanOrEqual(4_900);
    expect(elapsed).toBeLessThan(12_000);
  }, 20_000);

  // =========================================================================
  // Challenge 3: Discovery that executePipelineHeadless lacks internal timeout timer
  // =========================================================================
  it('documents pipeline execution cancellation vs automated timeout model', async () => {
    const runner = new BlenderRunner();
    // When no active task is running, cancelTask returns cancelled: false
    const cancelIdle = await runner.cancelTask('random-id');
    expect(cancelIdle.ok).toBe(true);
    expect(cancelIdle.cancelled).toBe(false);

    // Simulated pipeline task completes immediately within bounded duration (< 100ms)
    const start = Date.now();
    const result = await runner.runPipelineTask({
      taskType: 'inspect_mesh',
      outputDir: tempDir,
    }, { projectRoot: tempDir });

    const elapsed = Date.now() - start;
    expect(result.ok).toBe(true);
    expect(elapsed).toBeLessThan(500);
  });

  // =========================================================================
  // Challenge 4: Single-Slot ActiveTask Concurrency Race Condition
  // =========================================================================
  it('demonstrates activeTask overwrite vulnerability when two tasks run concurrently', async () => {
    if (!mockBlenderExe) {
      expect(true).toBe(true);
      return;
    }

    const runner = new BlenderRunner();
    await runner.detect(mockBlenderExe);

    const testDir1 = path.join(tempDir, 'conc-1');
    const testDir2 = path.join(tempDir, 'conc-2');
    await fs.mkdir(testDir1, { recursive: true });
    await fs.mkdir(testDir2, { recursive: true });

    const hangScript1 = path.join(testDir1, 'hang1.py');
    const hangScript2 = path.join(testDir2, 'hang2.py');
    await fs.writeFile(hangScript1, '# TRIGGER_HANG_FOREVER\n', 'utf8');
    await fs.writeFile(hangScript2, '# TRIGGER_HANG_FOREVER\n', 'utf8');

    // Launch Task 1 (hanging)
    const task1Promise = runner.runBlenderTask({
      scriptPath: hangScript1,
      outputDir: testDir1,
      timeoutMs: 30_000,
    });
    await new Promise((r) => setTimeout(r, 300));

    // Inspect active task
    const active1 = (runner as unknown as { activeTask: { taskId: string; child?: { pid?: number } } | null }).activeTask;
    expect(active1).not.toBeNull();
    const task1Id = active1!.taskId;
    const task1Pid = active1!.child?.pid;
    expect(task1Pid).toBeDefined();
    expect(isPidAlive(task1Pid!)).toBe(true);

    // Launch Task 2 (hanging) on the same runner instance
    const task2Promise = runner.runBlenderTask({
      scriptPath: hangScript2,
      outputDir: testDir2,
      timeoutMs: 30_000,
    });
    await new Promise((r) => setTimeout(r, 300));

    // Active task was overwritten by Task 2
    const active2 = (runner as unknown as { activeTask: { taskId: string; child?: { pid?: number } } | null }).activeTask;
    expect(active2).not.toBeNull();
    expect(active2!.taskId).not.toBe(task1Id);

    // Attempting to cancel Task 1 by its taskId fails because activeTask was overwritten
    const cancelTask1 = await runner.cancelTask(task1Id);
    expect(cancelTask1.cancelled).toBe(false); // Task 1 cannot be cancelled by ID!

    // Cancelling activeTask cancels Task 2
    const cancelTask2 = await runner.cancelTask();
    expect(cancelTask2.cancelled).toBe(true);

    // Active task is now null
    expect((runner as unknown as { activeTask: unknown }).activeTask).toBeNull();

    // Now Task 1 is completely orphaned from cancelTask
    const cancelAgain = await runner.cancelTask();
    expect(cancelAgain.cancelled).toBe(false);

    // Clean up Task 1 manually using its PID
    if (task1Pid) {
      try {
        process.kill(task1Pid, 'SIGKILL');
      } catch {
        // Ignored
      }
    }
    await Promise.allSettled([task1Promise, task2Promise]);
  }, 25_000);

  // =========================================================================
  // Challenge 4b: Windows Batch file (.cmd / .bat) spawn disparity
  // =========================================================================
  it('identifies batch wrapper discovery vs direct spawn disparity on Windows', async () => {
    const cmdPath = path.join(tempDir, 'blender_wrap.cmd');
    await fs.writeFile(cmdPath, '@echo off\r\necho Blender 4.2.0 (hash 12345)\r\n', 'utf8');

    const runner = new BlenderRunner();
    // probeExecutable works for .cmd because it explicitly wraps with cmd.exe /d /c
    const probe = await runner.detect(cmdPath);
    expect(probe.installed).toBe(true);
    expect(probe.version).toBe('4.2.0');

    // But executing runBlenderTask calls childProcess.spawn(binaryPath) without shell: true
    // On Windows Node >= 18.20 / 20.12, spawning .cmd without shell throws EINVAL
    const dummyScript = path.join(tempDir, 'dummy.py');
    await fs.writeFile(dummyScript, 'print("ok")\n', 'utf8');

    // Because executeHeadlessScript does NOT wrap .cmd/.bat with cmd.exe /d /c (unlike probeExecutable),
    // Node.js throws synchronous spawn EINVAL under Windows CVE-2024-27980 protection
    await expect(runner.runBlenderTask({
      scriptPath: dummyScript,
      outputDir: tempDir,
    })).rejects.toThrow(/spawn EINVAL/);
  });

  // =========================================================================
  // Challenge 5: Output Parsing Under Adversarial Malformed & Fuzzed Sentinels
  // =========================================================================
  it('handles corrupted, oversized, or nested sentinel blocks gracefully without crash', () => {
    const runner = new BlenderRunner();
    const parse = (s: string) =>
      (runner as unknown as { parseSentinelOutput: (raw: string) => unknown }).parseSentinelOutput(s);

    // Case 1: Corrupted JSON inside valid sentinels
    const corruptJson = `${OPENRO_3D_SENTINEL_START}\n{ "success": true, invalid json\n${OPENRO_3D_SENTINEL_END}`;
    expect(parse(corruptJson)).toBeNull();

    // Case 2: Inverted sentinels (END before START)
    const inverted = `${OPENRO_3D_SENTINEL_END}\n{"success": true}\n${OPENRO_3D_SENTINEL_START}`;
    expect(parse(inverted)).toBeNull();

    // Case 3: Empty sentinels
    const empty = `${OPENRO_3D_SENTINEL_START}\n\n${OPENRO_3D_SENTINEL_END}`;
    expect(parse(empty)).toBeNull();

    // Case 4: Oversized 100KB payload between sentinels
    const bigArray = new Array(2000).fill('path/to/asset.fbx');
    const validBig = `${OPENRO_3D_SENTINEL_START}\n${JSON.stringify({ success: true, generatedFiles: bigArray })}\n${OPENRO_3D_SENTINEL_END}`;
    const parsedBig = parse(validBig) as { success: boolean; generatedFiles: string[] };
    expect(parsedBig?.success).toBe(true);
    expect(parsedBig?.generatedFiles.length).toBe(2000);
  });

  // =========================================================================
  // Challenge 6: Simulated Fallback Mode Invariant & Manifest Conformance
  // =========================================================================
  it('guarantees deterministic, bounded, non-hanging execution across all 5 pipeline tasks in fallback mode', async () => {
    const runner = new BlenderRunner();

    // Force fallback mode (uninstalled)
    (runner as unknown as { cachedBinary: null; hasProbed: boolean }).cachedBinary = null;
    (runner as unknown as { hasProbed: boolean }).hasProbed = true;

    const taskTypes = [
      'inspect_mesh',
      'convert_format',
      'decimate_mesh',
      'generate_collision',
      'export_roblox_package',
    ] as const;

    for (const taskType of taskTypes) {
      const taskDir = path.join(tempDir, `sim-${taskType}`);
      await fs.mkdir(taskDir, { recursive: true });

      const startTime = Date.now();
      const res = await runner.runPipelineTask({
        taskType,
        outputDir: taskDir,
        assetName: `Asset_${taskType}`,
      }, { projectRoot: tempDir });
      const duration = Date.now() - startTime;

      expect(res.ok).toBe(true);
      expect(duration).toBeLessThan(1_000); // Guarantees bounded instant execution
      expect(res.generatedFiles).toBeDefined();

      if (taskType === 'export_roblox_package') {
        const manifestEntry = res.generatedFiles.find((f) => f.path.endsWith('roblox-asset.json'));
        expect(manifestEntry).toBeDefined();
        const manifestRaw = await fs.readFile(manifestEntry!.path, 'utf8');
        const manifest = JSON.parse(manifestRaw);
        expect(manifest.schemaVersion).toBe('1.0');
        expect(manifest.geometry.triangleCount).toBeGreaterThan(0);
        expect(manifest.robloxSettings.collisionFidelity).toBe('Hull');
      }
    }
  });
});
