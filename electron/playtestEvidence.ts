import * as crypto from 'node:crypto';
import { promises as fs } from 'node:fs';
import * as path from 'node:path';
import { resolveWithinRoot } from './pathSafety';

/**
 * Playtest evidence gate (S04). A playtest result is PASSED only when it has
 * at least one assertion, one state check and one readable PNG screenshot, and
 * every check matches. Any mismatch is FAILED and names the failing check.
 * Anything else is NO_EVIDENCE, which is never shown as a pass.
 */

export interface PlaytestCheck {
  description: string;
  expected: string;
  actual: string;
}

export interface PlaytestStateCheck {
  instancePath: string;
  property: string;
  expected: string;
  actual: string;
}

export interface PlaytestReport {
  scenario: string;
  assertions: PlaytestCheck[];
  stateChecks: PlaytestStateCheck[];
  screenshots: string[];
}

export interface PlaytestScreenshotEvidence {
  path: string;
  sha256: string;
  bytes: number;
}

export interface PlaytestVerdict {
  status: 'PASSED' | 'FAILED' | 'NO_EVIDENCE';
  reason: string;
  failingCheck?: string;
  screenshots: PlaytestScreenshotEvidence[];
}

const PNG_SIGNATURE = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
const JPEG_SIGNATURE = Buffer.from([0xff, 0xd8, 0xff]);
const MAX_SCREENSHOT_BYTES = 25_000_000;
const MAX_ITEMS = 50;
const MAX_VALUE = 2_000;

function isImage(bytes: Buffer): boolean {
  const png = bytes.length > PNG_SIGNATURE.length && bytes.subarray(0, PNG_SIGNATURE.length).equals(PNG_SIGNATURE);
  const jpeg = bytes.length > JPEG_SIGNATURE.length && bytes.subarray(0, JPEG_SIGNATURE.length).equals(JPEG_SIGNATURE);
  return png || jpeg;
}

/**
 * Resolve a reported playtest screenshot. Project-relative paths keep every
 * path-safety check; an absolute path may instead point at an existing file
 * inside the managed Studio capture folder (the reviewed capture sink).
 */
export async function resolvePlaytestScreenshot(root: string, reported: string, captureDir: string): Promise<string> {
  if (!path.isAbsolute(reported)) {
    return resolveWithinRoot(root, reported);
  }
  const resolvedBase = path.resolve(captureDir);
  const base = await fs.realpath(resolvedBase).catch(() => resolvedBase);
  const candidate = await fs.realpath(path.resolve(reported)).catch(() => null);
  if (!candidate) {
    throw new Error('Capture path is not an existing file.');
  }
  // realpath canonicalizes case on Windows, so a direct comparison is correct.
  if (!candidate.startsWith(`${base}${path.sep}`)) {
    throw new Error('Path is outside the project and the managed capture folder.');
  }
  const stat = await fs.stat(candidate);
  if (!stat.isFile()) {
    throw new Error('Capture path is not a file.');
  }
  return candidate;
}

function text(value: unknown, field: string): string {
  if (typeof value !== 'string' && typeof value !== 'number' && typeof value !== 'boolean') {
    throw new Error(`Playtest report field ${field} must be a string, number or boolean.`);
  }
  // Never cut a graded value: a difference after the cut would pass. Over-limit values are refused by gradePlaytest.
  return String(value);
}

function list(value: unknown, field: string): unknown[] {
  if (value === undefined) {
    return [];
  }
  if (!Array.isArray(value)) {
    throw new Error(`Playtest report field ${field} must be a list.`);
  }
  return [...value];
}

function record(value: unknown, field: string): Record<string, unknown> {
  if (!value || typeof value !== 'object' || Array.isArray(value)) {
    throw new Error(`Playtest report field ${field} must be an object.`);
  }
  return value as Record<string, unknown>;
}

/** Validate untrusted report input from the agent. Pass/fail is derived from expected vs actual, never self-declared. */
export function parsePlaytestReport(raw: unknown): PlaytestReport {
  const input = record(raw, 'report');
  return {
    scenario: text(input.scenario ?? '', 'scenario'),
    assertions: list(input.assertions, 'assertions').map((item, index) => {
      const entry = record(item, `assertions[${index}]`);
      return { description: text(entry.description, 'description'), expected: text(entry.expected, 'expected'), actual: text(entry.actual, 'actual') };
    }),
    stateChecks: list(input.stateChecks, 'stateChecks').map((item, index) => {
      const entry = record(item, `stateChecks[${index}]`);
      return {
        instancePath: text(entry.instancePath, 'instancePath'),
        property: text(entry.property, 'property'),
        expected: text(entry.expected, 'expected'),
        actual: text(entry.actual, 'actual'),
      };
    }),
    screenshots: list(input.screenshots, 'screenshots').map((item, index) => text(item, `screenshots[${index}]`)),
  };
}

/** Grade a report. `resolveScreenshot` maps a reported path to a safe absolute path or throws. */
export async function gradePlaytest(report: PlaytestReport, resolveScreenshot: (reported: string) => Promise<string>): Promise<PlaytestVerdict> {
  const screenshots: PlaytestScreenshotEvidence[] = [];
  const rejected: string[] = [];
  for (const reported of report.screenshots.slice(0, MAX_ITEMS)) {
    try {
      const target = await resolveScreenshot(reported);
      const stat = await fs.stat(target);
      if (!stat.isFile() || stat.size > MAX_SCREENSHOT_BYTES) {
        throw new Error('not a file under 25 MB');
      }
      const bytes = await fs.readFile(target);
      if (!isImage(bytes)) {
        throw new Error('not a PNG or JPEG');
      }
      screenshots.push({ path: reported, sha256: crypto.createHash('sha256').update(bytes).digest('hex'), bytes: bytes.length });
    } catch (error) {
      rejected.push(`${reported} (${error instanceof Error ? error.message : String(error)})`);
    }
  }

  const failedAssertion = report.assertions.find((check) => check.expected !== check.actual);
  if (failedAssertion) {
    const failingCheck = `assertion "${failedAssertion.description}": expected ${failedAssertion.expected}, got ${failedAssertion.actual}`;
    return { status: 'FAILED', reason: 'An assertion did not match.', failingCheck, screenshots };
  }
  const failedState = report.stateChecks.find((check) => check.expected !== check.actual);
  if (failedState) {
    const failingCheck = `state ${failedState.instancePath}.${failedState.property}: expected ${failedState.expected}, got ${failedState.actual}`;
    return { status: 'FAILED', reason: 'A state check did not match.', failingCheck, screenshots };
  }

  const missing: string[] = [];
  const lists = { assertions: report.assertions.length, stateChecks: report.stateChecks.length, screenshots: report.screenshots.length };
  for (const [field, count] of Object.entries(lists)) {
    if (count > MAX_ITEMS) missing.push(`${field} has ${count} items, over the limit of ${MAX_ITEMS}`);
  }
  const values = [
    report.scenario,
    ...report.assertions.flatMap((check) => [check.description, check.expected, check.actual]),
    ...report.stateChecks.flatMap((check) => [check.instancePath, check.property, check.expected, check.actual]),
    ...report.screenshots,
  ];
  if (values.some((value) => value.length > MAX_VALUE)) missing.push(`a value is longer than ${MAX_VALUE} characters`);
  if (report.assertions.length === 0) missing.push('no assertions');
  if (report.stateChecks.length === 0) missing.push('no state checks');
  if (screenshots.length === 0) missing.push('no readable image screenshot');
  if (rejected.length > 0) missing.push(`unusable screenshots: ${rejected.join(', ')}`);
  if (missing.length > 0) {
    return { status: 'NO_EVIDENCE', reason: `Cannot mark this playtest passed: ${missing.join('; ')}.`, screenshots };
  }
  return {
    status: 'PASSED',
    reason: `${report.assertions.length} assertions, ${report.stateChecks.length} state checks and ${screenshots.length} screenshots matched.`,
    screenshots,
  };
}
