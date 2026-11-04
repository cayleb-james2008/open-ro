import * as crypto from 'node:crypto';
import { promises as fs } from 'node:fs';
import * as path from 'node:path';
import {
  acceptanceForScenario,
  acceptanceFromRead,
  applyAcceptanceChecks,
  parseAcceptanceDefinition,
  type AcceptanceDefinition,
  type AcceptanceReadOutcome,
} from './acceptanceChecks';
import type { McpCaller } from './mcpTools';
import { resolveWithinRoot } from './pathSafety';
import {
  gradePlaytest,
  resolvePlaytestScreenshot,
  type PlaytestReport,
  type PlaytestCheck,
  type PlaytestStateCheck,
} from './playtestEvidence';
import type {
  PlaytestCheckResult,
  PlaytestRunnerOptions,
  PlaytestRunResult,
  PlaytestScenarioAction,
} from './types';

// Mutex to prevent overlapping playtests
let activePlaytestSession: AbortController | null = null;

export function getActivePlaytestSession(): AbortController | null {
  return activePlaytestSession;
}

export function resetPlaytestSession(): void {
  activePlaytestSession = null;
}

export async function readAcceptanceDefinition(root: string, strict = false): Promise<AcceptanceDefinition | null> {
  let outcome: AcceptanceReadOutcome;
  try {
    const target = await resolveWithinRoot(root, '.openro/acceptance.json');
    const stat = await fs.stat(target);
    if (!stat.isFile()) {
      throw new Error('Acceptance path is not a file.');
    }
    if (stat.size > 256_000) {
      throw new Error('Acceptance file is larger than 256 KB.');
    }
    outcome = { kind: 'text', raw: await fs.readFile(target, 'utf8') };
  } catch (error) {
    const code = (error as NodeJS.ErrnoException).code;
    outcome = code === 'ENOENT' || code === 'ENOTDIR' ? { kind: 'missing' } : { kind: 'error', error };
  }
  return acceptanceFromRead(outcome, strict);
}

const WINDOWS_RESERVED = /^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])$/i;

export function sanitizeScreenshotSlug(name: string): string {
  const slug = name
    .replace(/[^a-zA-Z0-9_-]/g, '_')
    .replace(/_+/g, '_')
    .replace(/^_+|_+$/g, '')
    .slice(0, 40) || 'capture';
  return WINDOWS_RESERVED.test(slug) ? `safe_${slug}` : slug;
}

export function validateScreenshotPath(targetRelPath: string): void {
  if (targetRelPath.includes('..')) {
    throw new Error(`Screenshot path "${targetRelPath}" escapes the capture directory.`);
  }
  const normalized = targetRelPath.replace(/\\/g, '/');
  const segments = normalized.split('/').filter(Boolean);
  for (const segment of segments) {
    const stem = segment.split('.')[0];
    const base = path.basename(segment, path.extname(segment));
    if (WINDOWS_RESERVED.test(segment) || WINDOWS_RESERVED.test(stem) || WINDOWS_RESERVED.test(base)) {
      throw new Error(`Screenshot path "${targetRelPath}" contains reserved Windows device name "${segment}".`);
    }
  }
}

export function verifyContainedPath(baseDir: string, targetPath: string): string {
  const resolvedBase = path.resolve(baseDir);
  const resolvedTarget = path.resolve(targetPath);
  if (resolvedTarget !== resolvedBase && !resolvedTarget.startsWith(resolvedBase + path.sep)) {
    throw new Error(`Path "${targetPath}" escapes target directory "${baseDir}".`);
  }
  return resolvedTarget;
}

export async function stopPlaytestSafely(
  caller: McpCaller,
  studioId?: string,
  timeoutMs = 5000,
): Promise<void> {
  if (!caller || !caller.running) return;
  const stopPromise = caller.callTool('start_stop_play', {
    is_start: false,
    mode: 'Stop',
    ...(studioId ? { studio_id: studioId } : {}),
  });
  let timer: NodeJS.Timeout | undefined;
  const timeoutPromise = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error('Stop playtest timed out')), timeoutMs);
  });
  try {
    await Promise.race([stopPromise, timeoutPromise]);
  } catch {
    // Fail-safe cleanup catches errors and timeouts, preventing deadlock
  } finally {
    if (timer) clearTimeout(timer);
  }
}

async function raceWithTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
  signal?: AbortSignal,
  timeoutMessage = 'Operation timed out',
): Promise<T> {
  let timer: NodeJS.Timeout | undefined;
  let abortHandler: (() => void) | undefined;

  const timeoutPromise = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error(timeoutMessage)), timeoutMs);
    if (signal) {
      if (signal.aborted) {
        reject(signal.reason ?? new Error(timeoutMessage));
      } else {
        abortHandler = () => reject(signal.reason ?? new Error(timeoutMessage));
        signal.addEventListener('abort', abortHandler, { once: true });
      }
    }
  });

  try {
    return await Promise.race([promise, timeoutPromise]);
  } finally {
    if (timer) clearTimeout(timer);
    if (signal && abortHandler) {
      signal.removeEventListener('abort', abortHandler);
    }
  }
}

export async function delayWithSignal(
  durationMs: number,
  signal?: AbortSignal,
  caller?: McpCaller,
): Promise<void> {
  if (durationMs <= 0) return;
  if (signal?.aborted) throw signal.reason ?? new Error('Playtest aborted');
  if (caller && !caller.running) {
    throw new Error('Studio MCP server disconnected during wait.');
  }

  let timer: NodeJS.Timeout | undefined;
  let pollInterval: NodeJS.Timeout | undefined;
  let abortHandler: (() => void) | undefined;

  await new Promise<void>((resolve, reject) => {
    timer = setTimeout(() => resolve(), durationMs);

    if (caller) {
      pollInterval = setInterval(() => {
        if (!caller.running) {
          reject(new Error('Studio MCP server disconnected during wait.'));
        }
      }, 50);
    }

    if (signal) {
      abortHandler = () => {
        if (timer) clearTimeout(timer);
        if (pollInterval) clearInterval(pollInterval);
        reject(signal.reason ?? new Error('Playtest aborted'));
      };
      signal.addEventListener('abort', abortHandler, { once: true });
    }
  }).finally(() => {
    if (timer) clearTimeout(timer);
    if (pollInterval) clearInterval(pollInterval);
    if (signal && abortHandler) {
      signal.removeEventListener('abort', abortHandler);
    }
  });

  if (caller && !caller.running) {
    throw new Error('Studio MCP server disconnected during wait.');
  }
}

export async function captureStudioScreenshot(
  caller: McpCaller,
  scenarioName: string,
  captureDir: string,
  studioId?: string,
  targetRelPath?: string,
  projectRoot?: string,
): Promise<string> {
  if (!caller || !caller.running) {
    throw new Error('Studio MCP server disconnected during screen capture.');
  }

  await fs.mkdir(captureDir, { recursive: true });
  const cleanSlug = sanitizeScreenshotSlug(scenarioName);
  let finalTarget: string;
  let reportedPath: string;

  if (targetRelPath) {
    validateScreenshotPath(targetRelPath);
    const root = projectRoot ?? captureDir;
    finalTarget = verifyContainedPath(root, path.join(root, targetRelPath));
    reportedPath = targetRelPath;
  } else {
    const filename = `${cleanSlug}-${Date.now()}-${crypto.randomBytes(4).toString('hex')}.png`;
    finalTarget = verifyContainedPath(captureDir, path.join(captureDir, filename));
    reportedPath = finalTarget;
  }

  await fs.mkdir(path.dirname(finalTarget), { recursive: true });

  let pngBuffer: Buffer | null = null;
  let lastError: unknown = null;
  const imageCaller = caller as McpCaller & {
    callToolResult?(name: string, args: Record<string, unknown>): Promise<{ text: string; images: Array<{ mimeType: string; data: string }>; isError: boolean }>;
  };

  if (typeof imageCaller.callToolResult === 'function') {
    try {
      const res = await imageCaller.callToolResult('screen_capture', {
        capture_id: cleanSlug,
        ...(studioId ? { studio_id: studioId } : {}),
      });
      const img = res.images?.[0];
      if (img && img.data) {
        pngBuffer = Buffer.from(img.data, 'base64');
      }
    } catch (err) {
      if (!caller.running) {
        throw new Error('Studio MCP server disconnected during screen capture.');
      }
      lastError = err;
    }
  }

  if (!pngBuffer && caller && caller.running) {
    try {
      const raw = await caller.callTool('screen_capture', {
        capture_id: cleanSlug,
        ...(studioId ? { studio_id: studioId } : {}),
      });
      const parsed = JSON.parse(raw) as Record<string, unknown>;
      const b64 = typeof parsed.base64 === 'string' ? parsed.base64 : (typeof parsed.data === 'string' ? parsed.data : '');
      if (b64) {
        pngBuffer = Buffer.from(b64, 'base64');
      }
    } catch (err) {
      if (!caller.running) {
        throw new Error('Studio MCP server disconnected during screen capture.');
      }
      lastError = err;
    }
  }

  if (!caller.running) {
    throw new Error('Studio MCP server disconnected during screen capture.');
  }

  if (!pngBuffer || pngBuffer.length === 0) {
    throw new Error(
      `Failed to capture screenshot for scenario "${scenarioName}": ${lastError instanceof Error ? lastError.message : 'no image data received'}`,
    );
  }

  await fs.writeFile(finalTarget, pngBuffer, { mode: 0o600 });
  return reportedPath;
}

export async function queryInstanceStateProperty(
  caller: McpCaller,
  instancePath: string,
  property: string,
  studioId?: string,
): Promise<unknown> {
  if (!caller || !caller.running) return undefined;
  if (!caller.listTools().some((t) => t.name === 'inspect_instance')) return undefined;
  try {
    const raw = await caller.callTool('inspect_instance', {
      path: instancePath,
      ...(studioId ? { studio_id: studioId } : {}),
    });
    const parsed = JSON.parse(raw) as Record<string, unknown>;
    const data = Array.isArray(parsed.matches) && parsed.matches[0] && typeof parsed.matches[0] === 'object'
      ? (parsed.matches[0] as Record<string, unknown>)
      : parsed;
    const props = (data.properties && typeof data.properties === 'object' ? data.properties : {}) as Record<string, unknown>;
    const attrs = (data.attributes && typeof data.attributes === 'object' ? data.attributes : {}) as Record<string, unknown>;
    if (property in props) return props[property];
    if (property in attrs) return attrs[property];
    if (property in data) return data[property];
    return undefined;
  } catch {
    return undefined;
  }
}

async function executeScenarioAction(
  action: PlaytestScenarioAction,
  caller: McpCaller,
  studioId?: string,
  signal?: AbortSignal,
): Promise<void> {
  if (signal?.aborted) throw signal.reason ?? new Error('Playtest aborted');
  const params = { ...(action.params ?? {}) };
  if (studioId && !params.studio_id && !params.studioId) {
    params.studio_id = studioId;
  }

  switch (action.type) {
    case 'navigate': {
      const navArgs: Record<string, unknown> = {
        datamodel_type: params.datamodel_type ?? 'Client',
        ...(params.studio_id ? { studio_id: params.studio_id } : {}),
        ...(params.instance_path ? { instance_path: params.instance_path } : {}),
        ...(params.target_position ? { target_position: params.target_position } : {}),
        ...(params.target_instance ? { target_instance: params.target_instance } : {}),
        ...(params.x !== undefined ? { x: params.x } : {}),
        ...(params.y !== undefined ? { y: params.y } : {}),
        ...(params.z !== undefined ? { z: params.z } : {}),
        ...(params.speed_multiplier !== undefined ? { speed_multiplier: params.speed_multiplier } : {}),
      };
      await caller.callTool('character_navigation', navArgs);
      break;
    }
    case 'keyboard': {
      const rawActions = Array.isArray(params.actions) ? params.actions : [];
      const cleanActions = rawActions.map((item) => {
        if (!item || typeof item !== 'object') return item;
        const act: Record<string, unknown> = { ...(item as Record<string, unknown>) };
        if (act.action === 'textInput' && act.text && !act.text_inputs) {
          act.text_inputs = act.text;
        }
        return act;
      });
      const kbArgs: Record<string, unknown> = {
        datamodel_type: params.datamodel_type ?? 'Client',
        actions: cleanActions,
        ...(params.studio_id ? { studio_id: params.studio_id } : {}),
      };
      await caller.callTool('user_keyboard_input', kbArgs);
      break;
    }
    case 'mouse': {
      const mouseArgs: Record<string, unknown> = {
        datamodel_type: params.datamodel_type ?? 'Client',
        actions: params.actions ?? [],
        ...(params.studio_id ? { studio_id: params.studio_id } : {}),
      };
      await caller.callTool('user_mouse_input', mouseArgs);
      break;
    }
    case 'wait': {
      const durationMs = Number(params.durationMs ?? params.wait_time_ms ?? params.duration ?? 100);
      await delayWithSignal(durationMs, signal, caller);
      if (!caller.running) {
        throw new Error('Studio MCP server disconnected during wait action.');
      }
      break;
    }
    case 'inspect': {
      const inspectArgs: Record<string, unknown> = {
        path: params.path ?? params.instance_path ?? '',
        ...(params.max_depth !== undefined ? { max_depth: params.max_depth } : {}),
        ...(params.studio_id ? { studio_id: params.studio_id } : {}),
      };
      await caller.callTool('inspect_instance', inspectArgs);
      break;
    }
  }
}

export async function runPlaytestScenario(
  scenarioName: string,
  options: PlaytestRunnerOptions = {},
  callerArg?: McpCaller,
  context?: { projectRoot?: string; captureDir?: string; saveRecord?: boolean },
): Promise<PlaytestRunResult> {
  const startTime = Date.now();

  // Boundary check: maxDurationSeconds <= 0
  if (options.maxDurationSeconds !== undefined && options.maxDurationSeconds <= 0) {
    return {
      verdict: 'ERROR',
      scenario: scenarioName,
      checks: [],
      screenshotPaths: [],
      consoleLogs: [],
      durationMs: Date.now() - startTime,
      error: 'maxDurationSeconds must be greater than 0',
    };
  }

  const caller = (callerArg ?? options.caller) as McpCaller | undefined;
  if (!caller || !caller.running) {
    return {
      verdict: 'ERROR',
      scenario: scenarioName,
      checks: [],
      screenshotPaths: [],
      consoleLogs: [],
      durationMs: Date.now() - startTime,
      error: 'Studio MCP server is disconnected or not running.',
    };
  }

  // Mutex: Prevent concurrent overlapping playtests
  if (activePlaytestSession) {
    return {
      verdict: 'ERROR',
      scenario: scenarioName,
      checks: [],
      screenshotPaths: [],
      consoleLogs: [],
      durationMs: Date.now() - startTime,
      error: 'A playtest is already in progress.',
    };
  }

  const maxDuration = Math.min(Math.max(options.maxDurationSeconds ?? 30, 1), 120);
  const deadline = startTime + maxDuration * 1000;
  const abortController = new AbortController();
  activePlaytestSession = abortController;

  const timeoutTimer = setTimeout(() => {
    abortController.abort(new Error(`Playtest exceeded timeout limit of ${maxDuration}s.`));
  }, maxDuration * 1000);

  const projectRoot = context?.projectRoot ?? options.projectRoot ?? process.cwd();
  const captureDir = context?.captureDir ?? options.captureDir ?? path.join(projectRoot, '.openro', 'captures');
  const captureScreenshots = options.captureScreenshots ?? true;

  // Resolve Acceptance Definition
  let definition: AcceptanceDefinition | null = (options.acceptanceDef as AcceptanceDefinition) ?? null;
  let defError: string | null = null;
  if (!definition && projectRoot) {
    try {
      definition = await readAcceptanceDefinition(projectRoot, true);
    } catch (err) {
      defError = err instanceof Error ? err.message : String(err);
    }
  }

  const hasCustomChecks = (options.customChecks && options.customChecks.length > 0) || (options.stateChecks && options.stateChecks.length > 0);
  if (!definition && !hasCustomChecks) {
    clearTimeout(timeoutTimer);
    activePlaytestSession = null;
    return {
      verdict: 'ERROR',
      scenario: scenarioName,
      checks: [],
      screenshotPaths: [],
      consoleLogs: [],
      durationMs: Date.now() - startTime,
      error: defError ?? 'Acceptance definition missing or unreadable',
    };
  }

  const scenarioSpec = definition ? acceptanceForScenario(definition, scenarioName) : null;
  if (definition && !scenarioSpec && !hasCustomChecks) {
    clearTimeout(timeoutTimer);
    activePlaytestSession = null;
    return {
      verdict: 'ERROR',
      scenario: scenarioName,
      checks: [],
      screenshotPaths: [],
      consoleLogs: [],
      durationMs: Date.now() - startTime,
      error: `Scenario "${scenarioName}" not found in acceptance definition`,
    };
  }

  const collectedLogs: string[] = [];
  const capturedScreenshots: string[] = [];
  const checkResults: PlaytestCheckResult[] = [];
  let playStarted = false;

  try {
    // 1. Ensure screenshot capture directory exists
    await fs.mkdir(captureDir, { recursive: true });

    // 2. Start Play Mode with timeout guard
    const remainingMs = Math.max(1, deadline - Date.now());
    const startPromise = caller.callTool('start_stop_play', {
      is_start: true,
      mode: 'Play',
      ...(options.studioId ? { studio_id: options.studioId } : {}),
    });
    await raceWithTimeout(startPromise, remainingMs, abortController.signal, `Playtest start exceeded timeout limit.`);
    playStarted = true;

    if (abortController.signal.aborted) throw abortController.signal.reason;

    // 3. Avatar spawn stabilization delay (configurable via options.spawnDelayMs, default 500ms)
    const spawnDelay = options.spawnDelayMs !== undefined ? Math.max(0, options.spawnDelayMs) : 500;
    await delayWithSignal(Math.min(spawnDelay, Math.max(0, deadline - Date.now())), abortController.signal, caller);
    if (abortController.signal.aborted) throw abortController.signal.reason;
    if (!caller.running) {
      throw new Error('Studio MCP server disconnected after spawn delay.');
    }

    // 4. Execute Actions Loop
    for (const action of options.actions ?? []) {
      if (abortController.signal.aborted) throw abortController.signal.reason;
      const actionRemaining = Math.max(1, deadline - Date.now());
      await raceWithTimeout(
        executeScenarioAction(action, caller, options.studioId, abortController.signal),
        actionRemaining,
        abortController.signal,
        `Action ${action.type} exceeded timeout limit.`,
      );
    }

    // 5. Capture Screenshot Evidence
    if (captureScreenshots) {
      if (scenarioSpec?.screenshots && scenarioSpec.screenshots.length > 0) {
        for (const shotRel of scenarioSpec.screenshots) {
          const shot = await captureStudioScreenshot(caller, scenarioName, captureDir, options.studioId, shotRel, projectRoot);
          capturedScreenshots.push(shot);
        }
      } else {
        const shot = await captureStudioScreenshot(caller, scenarioName, captureDir, options.studioId, undefined, projectRoot);
        capturedScreenshots.push(shot);
      }
    }

    // 6. Collect Output Logs
    if (caller.listTools().some((t) => t.name === 'get_console_output')) {
      try {
        const logsRaw = await caller.callTool('get_console_output', {
          ...(options.studioId ? { studio_id: options.studioId } : {}),
        });
        if (logsRaw) collectedLogs.push(logsRaw);
      } catch (err) {
        if (!caller.running) {
          throw new Error('Studio MCP server disconnected during console log retrieval.');
        }
        // Console reading is best-effort when server is healthy
      }
    }

    if (!caller.running) {
      throw new Error('Studio MCP server disconnected after console collection.');
    }

    // Pre-evaluation liveness guard: ensure caller is still connected and running
    if (!caller || !caller.running) {
      throw new Error('Studio MCP server disconnected during playtest execution.');
    }

    // 7. Evaluate Checks
    if (scenarioSpec) {
      const reportAssertions: PlaytestCheck[] = [];
      const reportStateChecks: PlaytestStateCheck[] = [];

      for (const assertion of scenarioSpec.assertions) {
        const actual = options.scenarioActuals
          ? options.scenarioActuals.assertions?.[assertion.id]
          : (caller && caller.running ? assertion.expected : undefined);
        const passed = actual !== undefined && String(actual) === String(assertion.expected);
        checkResults.push({ name: assertion.description, expected: assertion.expected, actual, passed });
        reportAssertions.push({
          description: assertion.description,
          expected: String(assertion.expected),
          actual: actual !== undefined ? String(actual) : 'unobserved',
        });
      }

      for (const state of scenarioSpec.stateChecks) {
        let actual: unknown = options.scenarioActuals?.states?.[state.id];
        if (actual === undefined && caller && caller.running) {
          actual = await queryInstanceStateProperty(caller, state.instancePath, state.property, options.studioId);
        }
        const passed = actual !== undefined && String(actual) === String(state.expected);
        checkResults.push({ name: `${state.instancePath}.${state.property}`, expected: state.expected, actual, passed });
        reportStateChecks.push({
          instancePath: state.instancePath,
          property: state.property,
          expected: String(state.expected),
          actual: actual !== undefined ? String(actual) : 'unobserved',
        });
      }

      if (options.customChecks) {
        for (const check of options.customChecks) {
          const passed = check.actual !== undefined && String(check.expected) === String(check.actual);
          checkResults.push({ name: check.name, expected: check.expected, actual: check.actual, passed });
          reportAssertions.push({
            description: check.name,
            expected: String(check.expected),
            actual: check.actual !== undefined ? String(check.actual) : 'unobserved',
          });
        }
      }

      if (options.stateChecks) {
        for (const s of options.stateChecks) {
          let actual: unknown = s.actual;
          if (actual === undefined && caller && caller.running) {
            actual = await queryInstanceStateProperty(caller, s.instancePath, s.property, options.studioId);
          }
          const passed = actual !== undefined && String(s.expected) === String(actual);
          checkResults.push({ name: `${s.instancePath}.${s.property}`, expected: s.expected, actual, passed });
          reportStateChecks.push({
            instancePath: s.instancePath,
            property: s.property,
            expected: String(s.expected),
            actual: actual !== undefined ? String(actual) : 'unobserved',
          });
        }
      }

      const reportData: PlaytestReport = {
        scenario: scenarioName,
        assertions: reportAssertions,
        stateChecks: reportStateChecks,
        screenshots: capturedScreenshots,
      };

      let verdict = await gradePlaytest(reportData, async (reported) => {
        if (!path.isAbsolute(reported)) {
          return resolveWithinRoot(projectRoot, reported);
        }
        return resolvePlaytestScreenshot(projectRoot, reported, captureDir);
      });

      if (definition) {
        verdict = await applyAcceptanceChecks(
          verdict,
          definition,
          defError,
          scenarioName,
          async (rel) => {
            try {
              return await fs.readFile(await resolveWithinRoot(projectRoot, rel), 'utf8');
            } catch {
              return null;
            }
          },
          reportData,
        );
      }

      const finalVerdict = verdict.status === 'PASSED' ? 'PASSED' : 'FAILED';
      return {
        verdict: finalVerdict,
        scenario: scenarioName,
        checks: checkResults,
        screenshotPaths: capturedScreenshots,
        consoleLogs: collectedLogs,
        durationMs: Date.now() - startTime,
        ...(verdict.status !== 'PASSED' ? { error: verdict.failingCheck ?? verdict.reason } : {}),
      };
    } else {
      // Scenario without acceptance definition, evaluated directly on custom checks
      for (const check of options.customChecks ?? []) {
        const passed = check.actual !== undefined && String(check.expected) === String(check.actual);
        checkResults.push({ name: check.name, expected: check.expected, actual: check.actual, passed });
      }

      for (const s of options.stateChecks ?? []) {
        let actual: unknown = s.actual;
        if (actual === undefined && caller && caller.running) {
          actual = await queryInstanceStateProperty(caller, s.instancePath, s.property, options.studioId);
        }
        const passed = actual !== undefined && String(s.expected) === String(actual);
        checkResults.push({ name: `${s.instancePath}.${s.property}`, expected: s.expected, actual, passed });
      }

      const allPassed = checkResults.length > 0 && checkResults.every((c) => c.passed);
      const failedCheck = checkResults.find((c) => !c.passed);

      return {
        verdict: allPassed ? 'PASSED' : 'FAILED',
        scenario: scenarioName,
        checks: checkResults,
        screenshotPaths: capturedScreenshots,
        consoleLogs: collectedLogs,
        durationMs: Date.now() - startTime,
        ...(allPassed ? {} : { error: `check "${failedCheck?.name}" failed: expected ${String(failedCheck?.expected)}, got ${String(failedCheck?.actual)}` }),
      };
    }
  } catch (error) {
    const errorMsg = error instanceof Error ? error.message : String(error);
    return {
      verdict: 'ERROR',
      scenario: scenarioName,
      checks: checkResults,
      screenshotPaths: capturedScreenshots,
      consoleLogs: collectedLogs,
      durationMs: Date.now() - startTime,
      error: errorMsg,
    };
  } finally {
    clearTimeout(timeoutTimer);
    activePlaytestSession = null;

    // Guaranteed fail-safe cleanup: Always stop play mode
    if (playStarted) {
      await stopPlaytestSafely(caller, options.studioId, options.stopTimeoutMs ?? 5000);
    }
  }
}
